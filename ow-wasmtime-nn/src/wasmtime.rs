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
        let mut metrics = HashMap::new();

        let wasm_action = self
            .instance_pres
            .get(container_id)
            .ok_or_else(|| anyhow!(format!("No action named {}", container_id)))?;
        let instance_pre = &wasm_action.module;

        let mut store = create_store(&self.engine);

        // Replace the image URLs with their base64-encoded contents (if needed)
        let start_time = Instant::now();
        handle_replace_images(&mut parameters);
        metrics.insert("download_images_time", start_time.elapsed().as_secs_f64());

        let instance = instance_pre.instantiate(&mut store).unwrap();

        // Write the input to the WASM memory
        pass_input(&instance, &mut store, &parameters)?;

        // Write the model to the WASM memory
        let start_time = Instant::now();
        pass_model(&instance, &mut store, &parameters, &self.model_cache)?;
        metrics.insert("pass_model_time", start_time.elapsed().as_secs_f64());

        // Call the _start function
        let main = instance.get_typed_func::<(), ()>(&mut store, "_start").unwrap();
        main.call(&mut store, ())?;

        // Retrieve the result from the WASM memory
        let mut result = retrieve_result(&instance, &mut store)?;

        // Add executor_metrics to the response
        result["executor_metrics"] = serde_json::json!(metrics);
        Ok(Ok(result))
    }

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
    parameters: &Value,
    model_cache: &Arc<TimedMap<String, Vec<u8>>>
) -> Result<(), anyhow::Error> {

    let model_key = parameters["model"].as_str().ok_or_else(|| anyhow!("From embedder: 'model' not found in JSON"))?.to_string();

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

    println!("Replacing images with method: {}", replace_images);

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
    if let Some(image_value) = parameters.get_mut("image") {
        match image_value {
            serde_json::Value::Array(image_urls) => {
                // Shared results container
                let encoded_images = Arc::new(Mutex::new(Vec::new()));
                let mut handles = vec![];
                // Print number of images
                for image in image_urls.clone() {
                    if let Some(image_url) = image.as_str() {
                        let encoded_images = Arc::clone(&encoded_images);
                        let image_url = image_url.to_string();
                        // Spawn a thread for each image URL
                        let handle = std::thread::spawn(move || -> anyhow::Result<()> {
                            let image_bytes = reqwest::blocking::get(&image_url)?.bytes()?.to_vec();
                            let encoded_image = serde_json::Value::String(base64::encode(&image_bytes));
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

    if let Some(image_value) = parameters.get_mut("image") {
        match image_value {
            Value::Array(image_uris) => {
                let encoded_images: Result<Vec<Value>, anyhow::Error> = image_uris
                    .par_iter() // Parallel iterator
                    .map(|image| {
                        if let Some(image_url) = image.as_str() {
                            runtime
                                .block_on(download_image_s3(&client, image_url))
                                .map(|encoded_image| Value::String(encoded_image))
                        } else {
                            Err(anyhow!("From embedder: 'image_uris' list contains a non-string value"))
                        }
                    })
                    .collect();
               parameters["image"] = Value::Array(encoded_images?);
            }
            _ => {return Err(anyhow!("From embedder: 'image' is not a list"));}
        }
    } else {
        return Err(anyhow!("From embedder: 'image' key not found in JSON"));
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






