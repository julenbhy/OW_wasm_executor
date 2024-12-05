use std::{sync::Arc, time::Duration, sync::Mutex};
use dashmap::DashMap;
use timedmap::TimedMap;
use anyhow::anyhow;
use serde_json::Value;
use reqwest;
use base64;

use ow_common::{ActionCapabilities, WasmAction, WasmRuntime};

use wasmtime::{Engine, Linker, Module, Store, InstancePre};
use wasmtime_wasi::{WasiCtxBuilder};
use wasmtime_wasi::preview1::{self, WasiP1Ctx};
use std::collections::HashMap;
use wasmtime_wasi_nn::witx::WasiNnCtx;
//use wasmtime_wasi_nn::backend::pytorch::PytorchBackend;
use std::time::Instant;
use rayon::prelude::*;

use aws_sdk_s3::Client;
use aws_config::meta::region::RegionProviderChain;
use base64::encode;
use tokio::runtime::Runtime;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct Wasmtime {
    pub engine: Engine,
    pub instance_pres: Arc<DashMap<String, WasmAction< InstancePre<WasmCtx> >>>,
    pub instance_pre_cache: Arc<TimedMap<u64, InstancePre<WasmCtx>>>,
    pub model_cache: Arc<TimedMap<String, Vec<u8>>>,
}

impl Default for Wasmtime {
    fn default() -> Self {
        Self {
            engine: Engine::default(),
            instance_pres: Arc::new(DashMap::new()),
            instance_pre_cache: Arc::new(TimedMap::new()),
            model_cache: Arc::new(TimedMap::new()),
        }
    }
}

const CACHE_TTL: Duration = Duration::from_secs(60);


pub struct WasmCtx {
    wasi: WasiP1Ctx,
    wasi_nn: WasiNnCtx,
}

impl WasmCtx {
    pub fn wasi(&mut self) -> &mut WasiP1Ctx {
        &mut self.wasi
    }

    fn wasi_nn(&mut self) -> &mut WasiNnCtx {
        &mut self.wasi_nn
    }
}


impl WasmRuntime for Wasmtime {
    fn initialize(
        &self,
        container_id: String,
        capabilities: ActionCapabilities,
        module: Vec<u8>,
    ) -> anyhow::Result<()> {

        let module_hash = fasthash::metro::hash64(&module);

        // Check if the preinstance of the module is already in the cache
        let instance_pre = if let Some(pre) = self.instance_pre_cache.get(&module_hash) {
            self.instance_pre_cache.refresh(&module_hash, CACHE_TTL);
            println!("Module found in cache. Using cached module...");
            pre.clone()
        } else {
            println!("Module not found in cache. Preinstantiating module...");
            // deserialize could fail due to https://docs.wasmtime.dev/api/wasmtime/struct.Module.html#method.deserialize Unsafety
            // module must've been precompiled with a matching version of wasmtime
            let module = unsafe { match Module::deserialize(&self.engine, &module) {
                                    Ok(module) => module,
                                    Err(e) => {
                                        println!("\x1b[31mError deserializing module: {}\x1b[0m", e);
                                        return Err(anyhow!("Error deserializing module"));
                                    }
                                }};

            // Add WASI to the linker
            let mut linker: Linker<WasmCtx> = Linker::new(&self.engine);
            link_host_functions(&mut linker)?;

            let instance_pre = linker.instantiate_pre(&module)?;

            self.instance_pre_cache.insert(module_hash, instance_pre.clone(), CACHE_TTL);
            instance_pre
        };

        let action = WasmAction {
            module: instance_pre,
            capabilities,
        };

        self.instance_pres.insert(container_id.clone(), action);

        Ok(())
    }

    fn run(
        &self,
        container_id: &str,
        mut parameters: Value,
    ) -> Result<Result<Value, Value>, anyhow::Error> {
        let wasm_action = self
            .instance_pres
            .get(container_id)
            .ok_or_else(|| anyhow!(format!("No action named {}", container_id)))?;
        let instance_pre = &wasm_action.module;
        handle_replace_images(&mut parameters);

        let model_keys = parameters["models"]
            .as_array()
            .ok_or_else(|| anyhow!("From embedder: 'model' not found in JSON or is not an array"))?;
        let results = Arc::new(Mutex::new(Value::Object(serde_json::Map::new())));

        let start_functions_time = Instant::now();
        let mut handles = vec![];

        for model_key in model_keys {
            let model_key = model_key
                .as_str()
                .ok_or_else(|| anyhow!("From embedder: 'model' contains a non-string value"))?
                .to_string();
            let parameters = parameters.clone();
            let results = Arc::clone(&results);
            let instance_pre = instance_pre.clone();
            let engine = self.engine.clone();
            let model_cache = self.model_cache.clone();

            let handle = std::thread::spawn(move || -> Result<(), anyhow::Error> {
                let thread_start = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs_f64();
                let start_time = Instant::now();

                let mut store = create_store(&engine);

                let instance = instance_pre.instantiate(&mut store).unwrap();

                // Write the input to the WASM memory
                pass_input(&instance, &mut store, &parameters)?;

                let start_pass_model_time = Instant::now();
                let pass_model_start = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs_f64();
                pass_model(&instance, &mut store, model_key.clone(), &model_cache)?;
                let pass_model_end = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs_f64();
                let pass_model_duration = start_pass_model_time.elapsed().as_secs_f64();

                // Call the _start function
                let main = instance.get_typed_func::<(), ()>(&mut store, "_start").unwrap();
                main.call(&mut store, ())?;

                // Retrieve the result from the WASM memory
                let mut result = retrieve_result(&instance, &mut store)?;

                let thread_end = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs_f64();

                // Add to the metrics the time taken to process the model
                let duration = start_time.elapsed().as_secs_f64();

                // Add timing metrics
                result["metrics"]["func_time"] =
                    serde_json::Value::Number(serde_json::Number::from_f64(duration).unwrap());
                result["metrics"]["pass_model_time"] =
                    serde_json::Value::Number(serde_json::Number::from_f64(pass_model_duration).unwrap());
                result["metrics"]["thread_start"] =
                    serde_json::Value::Number(serde_json::Number::from_f64(thread_start).unwrap());
                result["metrics"]["thread_end"] =
                    serde_json::Value::Number(serde_json::Number::from_f64(thread_end).unwrap());
                result["metrics"]["pass_model_start"] =
                    serde_json::Value::Number(serde_json::Number::from_f64(pass_model_start).unwrap());
                result["metrics"]["pass_model_end"] =
                    serde_json::Value::Number(serde_json::Number::from_f64(pass_model_end).unwrap());



                println!("Model {} returned: {}", model_key, result);

                // Store the result
                let mut results_lock = results.lock().unwrap();
                results_lock
                    .as_object_mut()
                    .unwrap()
                    .insert(model_key, result);

                Ok(())
            });

            handles.push(handle);
        }

        // Wait for all threads to finish
        for handle in handles {
            handle.join().map_err(|e| anyhow!("Thread error: {:?}", e))??;
        }

        let functions_duration = start_functions_time.elapsed().as_secs_f64();

        let mut final_results = Arc::try_unwrap(results)
            .unwrap_or_else(|_| Mutex::new(Value::Object(serde_json::Map::new())))
            .into_inner()
            .unwrap();
        // Add functions_duration to the metrics
        final_results["metrics"]["functions_duration"] =
            serde_json::Value::Number(serde_json::Number::from_f64(functions_duration).unwrap());

        Ok(Ok(final_results))
    }

//     fn run(
//         &self,
//         container_id: &str,
//         mut parameters: Value,
//     ) -> Result<Result<Value, Value>, anyhow::Error> {
//         let wasm_action = self
//             .instance_pres
//             .get(container_id)
//             .ok_or_else(|| anyhow!(format!("No action named {}", container_id)))?;
//         let instance_pre = &wasm_action.module;
//         handle_replace_images(&mut parameters);
//
//         let model_keys = parameters["models"].as_array().ok_or_else(|| anyhow!("From embedder: 'model' not found in JSON or is not an array"))?;
//         let results = Arc::new(Mutex::new(Value::Object(serde_json::Map::new())));
//
//         let start_functions_time = Instant::now();
//         // Do a parallel iter over the model keys
//         model_keys.par_iter().try_for_each(|model_key| {
//             let start_time = Instant::now();
//
//             let model_key_str = model_key.as_str().ok_or_else(|| anyhow!("From embedder: 'model' contains a non-string value"))?;
//
//             let mut store = create_store(&self.engine);
//
//             // Replace the image URLs with their base64-encoded contents (if needed)
//
//             let instance = instance_pre.instantiate(&mut store).unwrap();
//
//             // Write the input to the WASM memory
//             pass_input(&instance, &mut store, &parameters)?;
//
//             let start_pass_model_time = Instant::now();
//             pass_model(&instance, &mut store, model_key_str.to_string(), &self.model_cache)?;
//             let pass_model_duration = start_pass_model_time.elapsed().as_secs_f64();
//
//             // Call the _start function
//             let main = instance.get_typed_func::<(), ()>(&mut store, "_start").unwrap();
//             main.call(&mut store, ())?;
//
//             // Retrieve the result from the WASM memory
//             let mut result = retrieve_result(&instance, &mut store)?;
//
//             // Add to the metrics the time taken to process the model
//             let duration = start_time.elapsed().as_secs_f64();
//
//             // result is a json, add func_time to it
//             result["metrics"]["func_time"] = serde_json::Value::Number(serde_json::Number::from_f64(duration).unwrap());
//             result["metrics"]["pass_model_time"] = serde_json::Value::Number(serde_json::Number::from_f64(pass_model_duration).unwrap());
//             println!("Model {} returned: {}", model_key_str, result);
//             let mut results_lock = results.lock().unwrap();
//             results_lock.as_object_mut().unwrap().insert(model_key_str.to_string(), result);
//
//             // Return the result
//             Ok::<(), anyhow::Error>(())
//         });
//         let functions_duration = start_functions_time.elapsed().as_secs_f64();
//
//         let mut final_results = Arc::try_unwrap(results)
//         .unwrap_or_else(|_| Mutex::new(Value::Object(serde_json::Map::new())))
//         .into_inner()
//         .unwrap();
//         // Add functions_duration to the metrics
//         final_results["metrics"]["functions_duration"] = serde_json::Value::Number(serde_json::Number::from_f64(functions_duration).unwrap());
//
//         Ok(Ok(final_results))
//     }

    fn destroy(
        &self,
        container_id: &str
    ) {
        if let None = self.instance_pres.remove(container_id) {
            println!("No container with id {} existed.", container_id);
        }
    }

}


fn create_store(
    engine: &Engine
) -> Store<WasmCtx> {
    let wasi = WasiCtxBuilder::new()
        .inherit_stdio()
        .inherit_stderr()
        .build_p1();

    //let graph = vec![("pytorch".to_string(), "models".to_string())]; // Convert to Vec<(String, String)>
    let (backends, registry) = wasmtime_wasi_nn::preload(&[]).unwrap();
    let wasi_nn = WasiNnCtx::new(backends, registry);

    let wasm_ctx = WasmCtx {
        wasi,
        wasi_nn,
    };

    Store::new(engine, wasm_ctx)
}

fn link_host_functions(
    linker: &mut Linker<WasmCtx>
) -> Result<(), anyhow::Error> {
    preview1::add_to_linker_sync(linker, WasmCtx::wasi)?;
    wasmtime_wasi_nn::witx::add_to_linker(linker, WasmCtx::wasi_nn)?;
    Ok(())
}

fn pass_input(
    instance: &wasmtime::Instance,
    store: &mut Store<WasmCtx>,
    parameters: &Value
) -> Result<(), anyhow::Error> {

    let input = parameters.to_string();
    // Access the WASM memory
    let memory = instance
        .get_memory(&mut *store, "memory")
        .ok_or_else(|| anyhow::anyhow!("Failed to get WASM memory"))?;

    // Obtain the pointer to the input with set_input
    let set_input = instance
        .get_typed_func::<u32, u32>(&mut *store, "set_input")
        .map_err(|_| anyhow::anyhow!("Failed to get set_input"))?;
    let input_ptr = set_input.call(&mut *store, input.len() as u32)? as usize;

    // Write the input to the WASM memory
    let content = input.as_bytes();
    memory.data_mut(&mut *store)[input_ptr..(input_ptr + content.len())].copy_from_slice(content);

    Ok(())
}


fn retrieve_result(
    instance: &wasmtime::Instance,
    store: &mut Store<WasmCtx>
) -> Result<Value, anyhow::Error> {
    // Access the WASM memory
    let memory = instance
        .get_memory(&mut *store, "memory")
        .ok_or_else(|| anyhow::anyhow!("Failed to get WASM memory"))?;

    // Get the length of the result with get_result_len
    let get_result_len = instance
        .get_typed_func::<(), u32>(&mut *store, "get_result_len")
        .map_err(|_| anyhow::anyhow!("Failed to get get_result_len"))?;
    let length = get_result_len.call(&mut *store, ())? as usize;

    // Get the pointer to the result with get_result
    let get_result = instance
        .get_typed_func::<(), u32>(&mut *store, "get_result")
        .map_err(|_| anyhow::anyhow!("Failed to get get_result"))?;
    let content_ptr = get_result.call(&mut *store, ())? as usize;

    // Read the result from the WASM memory
    let content = memory.data(&store)[content_ptr..(content_ptr + length)].to_vec();


    let result = String::from_utf8(content)?;
    let json_result: Value = serde_json::from_str(&result)?;

    Ok(json_result)
}


fn pass_model(
    instance: &wasmtime::Instance,
    store: &mut Store<WasmCtx>,
    model_key: String,
    model_cache: &Arc<TimedMap<String, Vec<u8>>>
) -> Result<(), anyhow::Error> {

    // Check if the model is already in the cache
    let model_bytes = if let Some(cached_bytes) = model_cache.get(&model_key) {
        println!("Model found in cache. Using cached model...");
        model_cache.refresh(&model_key, CACHE_TTL);
        cached_bytes.clone()
    } else {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::new(120, 0))
            .build()?;
        println!("Model not found in cache. Downloading model...");
        let response = client.get(&model_key).send()?.error_for_status()?;
        let downloaded_bytes = response.bytes()?.to_vec();
        model_cache.insert(model_key.clone(), downloaded_bytes.clone(), CACHE_TTL);
        downloaded_bytes
    };

    // Access the WASM memory
    let memory = instance
        .get_memory(&mut *store, "memory")
        .ok_or_else(|| anyhow::anyhow!("Failed to get WASM memory"))?;

    // Obtain the pointer to the model with set_model
    let set_model = instance
        .get_typed_func::<u32, u32>(&mut *store, "set_model")
        .map_err(|_| anyhow::anyhow!("Failed to get set_model"))?;
    let model_ptr = set_model.call(&mut *store, model_bytes.len() as u32)? as usize;

    // Write the model to the WASM memory
    memory.data_mut(&mut *store)[model_ptr..(model_ptr + model_bytes.len())].copy_from_slice(&model_bytes);

    Ok(())
}


fn handle_replace_images(parameters: &mut Value) -> Result<(), Box<dyn std::error::Error>> {
    let replace_images = parameters
        .get("replace_images")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match replace_images {
        "URL" => replace_image_urls_parallel(parameters)?,
        "S3" => replace_image_urls_s3_parallel(parameters)?,
        _ => {
            // Handle default case or log a warning if necessary
            println!("No valid replacement option provided");
        },
    }

    Ok(())
}

// Unused
fn replace_image_urls(
    parameters: &mut Value
) -> anyhow::Result<()> {
    if let Some(image_value) = parameters.get_mut("image") {
        match image_value {
            // Caso: 'image' es una cadena
            Value::String(image) => {
                let image_url = image.as_str(); // Tomamos una referencia inmutable
                let image_bytes = reqwest::blocking::get(image_url)?.bytes()?.to_vec();
                *image_value = Value::String(base64::encode(&image_bytes));
            }
            // Caso: 'image' es una lista de cadenas
            Value::Array(images) => {
                let mut encoded_images = Vec::new();
                for image in images.iter() {
                    if let Some(image_url) = image.as_str() {
                        let image_bytes = reqwest::blocking::get(image_url)?.bytes()?.to_vec();
                        encoded_images.push(Value::String(base64::encode(&image_bytes)));
                    } else {
                        return Err(anyhow!("From embedder: 'image' list contains a non-string value"));
                    }
                }
                *image_value = Value::Array(encoded_images);
            }
            // Caso: 'image' no es ni una cadena ni una lista
            _ => {
                return Err(anyhow!("From embedder: 'image' is not a string or a list of strings"));
            }
        }
    } else {
        return Err(anyhow!("From embedder: 'image' not found in JSON"));
    }
    Ok(())
}


fn replace_image_urls_parallel(
    parameters: &mut serde_json::Value,
) -> anyhow::Result<()> {
    if let Some(image_value) = parameters.get_mut("image_urls") {
        match image_value {
            serde_json::Value::Array(image_urls) => {
                // Shared results container
                let encoded_images = Arc::new(Mutex::new(Vec::new()));
                let mut handles = vec![];

                for image in image_urls.clone() {
                    if let Some(image_url) = image.as_str() {
                        let encoded_images = Arc::clone(&encoded_images);
                        let image_url = image_url.to_string();

                        // Spawn a thread for each image URL
                        let handle = std::thread::spawn(move || -> anyhow::Result<()> {
                            println!("Downloading image from URL: {}", image_url);
                            let image_bytes = reqwest::blocking::get(&image_url)?.bytes()?.to_vec();
                            println!("Image downloaded. Now encoding...");
                            let encoded_image = serde_json::Value::String(base64::encode(&image_bytes));
                            println!("Image downloaded and encoded");

                            // Safely append the encoded image to the results
                            let mut lock = encoded_images.lock().unwrap();
                            lock.push(encoded_image);
                            Ok(())
                        });

                        handles.push(handle);
                    } else {
                        return Err(anyhow::anyhow!("From embedder: 'image_urls' list contains a non-string value"));
                    }
                }

                // Wait for all threads to complete
                for handle in handles {
                    handle.join().map_err(|e| anyhow::anyhow!("Thread error: {:?}", e))??;
                }

                // Replace the "image" field in parameters with the results
                let final_results = Arc::try_unwrap(encoded_images)
                    .unwrap_or_else(|_| Mutex::new(Vec::new()))
                    .into_inner()
                    .unwrap();
                parameters["image"] = serde_json::Value::Array(final_results);
            }
            _ => {
                return Err(anyhow::anyhow!("From embedder: 'image_urls' is not a list"));
            }
        }
    } else {
        return Err(anyhow::anyhow!("From embedder: 'image_urls' key not found in JSON"));
    }
    Ok(())
}

fn replace_image_urls_s3_parallel(parameters: &mut Value) -> Result<(), anyhow::Error> {
    // Configure the S3 client
    let runtime = Runtime::new()?;
    let region_provider = RegionProviderChain::default_provider().or_else("eu-west-1");
    let config = runtime.block_on(aws_config::from_env().region(region_provider).load());
    let client = Client::new(&config);

    if let Some(image_value) = parameters.get_mut("image_uris") {
        match image_value {
            Value::Array(image_uris) => {
                // Shared results container
                let encoded_images = Arc::new(Mutex::new(Vec::new()));
                let mut handles = vec![];

                for image in image_uris.clone() {
                    if let Some(image_url) = image.as_str() {
                        let client = client.clone();
                        let image_url = image_url.to_string();
                        let encoded_images = Arc::clone(&encoded_images);
                        let runtime = runtime.handle().clone();

                        // Spawn a thread for each S3 URI
                        let handle = std::thread::spawn(move || -> Result<(), anyhow::Error> {
                            let encoded_image = runtime.block_on(download_image_s3(&client, &image_url))?;
                            let mut lock = encoded_images.lock().unwrap();
                            lock.push(Value::String(encoded_image));
                            Ok(())
                        });

                        handles.push(handle);
                    } else {
                        return Err(anyhow!("From embedder: 'image_uris' list contains a non-string value"));
                    }
                }

                // Wait for all threads to complete
                for handle in handles {
                    handle.join().map_err(|e| anyhow!("Thread error: {:?}", e))??;
                }

                // Replace the "image" field in parameters with the results
                let final_results = Arc::try_unwrap(encoded_images)
                    .unwrap_or_else(|_| Mutex::new(Vec::new()))
                    .into_inner()
                    .unwrap();
                parameters["image"] = Value::Array(final_results);
            }
            _ => {
                return Err(anyhow!("From embedder: 'image_uris' is not a list"));
            }
        }
    } else {
        return Err(anyhow!("From embedder: 'image_uris' key not found in JSON"));
    }

    Ok(())
}



async fn download_image_s3(client: &Client, s3_url: &str) -> Result<String, anyhow::Error> {
    // Parse the S3 URL (assumes format s3://bucket/key)
    let parts: Vec<&str> = s3_url.trim_start_matches("s3://").splitn(2, '/').collect();
    if parts.len() != 2 {
        return Err(anyhow!("Invalid S3 URL format"));
    }
    let bucket = parts[0];
    let key = parts[1];

    // Download the object from S3
    let response = client.get_object().bucket(bucket).key(key).send().await?;
    let body = response.body.collect().await?;
    let bytes = body.into_bytes();

    // Encode as Base64 and return
    Ok(encode(&bytes))
}





