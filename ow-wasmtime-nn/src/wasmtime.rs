use std::{sync::Arc, time::Duration,};
use dashmap::DashMap;
use timedmap::TimedMap;
use anyhow::anyhow;
use ow_common::{ActionCapabilities, WasmAction, WasmRuntime};

use wasmtime::{Engine, Linker, Module, Store, InstancePre};
use wasmtime_wasi::{WasiCtxBuilder, DirPerms, FilePerms};
use wasmtime_wasi::preview1::{self, WasiP1Ctx};

use wasmtime_wasi_nn::witx::WasiNnCtx;
//use wasmtime_wasi_nn::backend::pytorch::PytorchBackend;

use reqwest;
use base64;


#[derive(Clone)]
pub struct Wasmtime {
    pub engine: Engine,
    pub instance_pres: Arc<DashMap<String, WasmAction< InstancePre<WasmCtx> >>>,
    pub instance_pre_cache: Arc<TimedMap<u64, InstancePre<WasmCtx>>>, // TODO: Remove unused instance_pres after an unusedTimeout
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

fn create_store(engine: &Engine) -> Store<WasmCtx> {
    let wasi = WasiCtxBuilder::new()
        .inherit_stdio()
        .inherit_stderr()
        .build_p1();

    //let graph = [("pytorch", "pytorch_fixtures/mobilenet")];
    let (backends, registry) = wasmtime_wasi_nn::preload(&[]).unwrap();
    let wasi_nn = WasiNnCtx::new(backends, registry);

    let wasm_ctx = WasmCtx {
        wasi,
        wasi_nn,
    };

    Store::new(engine, wasm_ctx)
}

fn link_host_functions(linker: &mut Linker<WasmCtx>) -> Result<(), anyhow::Error> {
    preview1::add_to_linker_sync(linker, WasmCtx::wasi)?;
    wasmtime_wasi_nn::witx::add_to_linker(linker, WasmCtx::wasi_nn)?;
    Ok(())
}

fn pass_input(instance: &wasmtime::Instance, store: &mut Store<WasmCtx>, input: &str) -> Result<(), anyhow::Error> {
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

fn pass_model(instance: &wasmtime::Instance, store: &mut Store<WasmCtx>, model_bytes: &[u8]) -> Result<(), anyhow::Error> {
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
    memory.data_mut(&mut *store)[model_ptr..(model_ptr + model_bytes.len())].copy_from_slice(model_bytes);

    Ok(())
}

fn retrieve_result(instance: &wasmtime::Instance, store: &mut Store<WasmCtx>) -> Result<String, anyhow::Error> {
    // Accede a la memoria del módulo WASM
    let memory = instance
        .get_memory(&mut *store, "memory")
        .ok_or_else(|| anyhow::anyhow!("Failed to get WASM memory"))?;

    // Obtiene la longitud del resultado con get_result_len
    let get_result_len = instance
        .get_typed_func::<(), u32>(&mut *store, "get_result_len")
        .map_err(|_| anyhow::anyhow!("Failed to get get_result_len"))?;
    let length = get_result_len.call(&mut *store, ())? as usize;

    // Obtiene el puntero al resultado con get_result
    let get_result = instance
        .get_typed_func::<(), u32>(&mut *store, "get_result")
        .map_err(|_| anyhow::anyhow!("Failed to get get_result"))?;
    let content_ptr = get_result.call(&mut *store, ())? as usize;

    // Lee el contenido de la memoria en el rango especificado
    let content = memory.data(&store)[content_ptr..(content_ptr + length)].to_vec();

    // Convierte el contenido a String UTF-8
    let result = String::from_utf8(content)?;

    Ok(result)
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
        mut parameters: serde_json::Value,
    ) -> Result<Result<serde_json::Value, serde_json::Value>, anyhow::Error> {
        
        // Replace the model download url with the model bytes  
        let model_key = parameters["model"].as_str().ok_or_else(|| anyhow!("From embedder: 'model' not found in JSON"))?.to_string();
        
        let model_bytes = if let Some(cached_bytes) = self.model_cache.get(&model_key) {
            println!("Model found in cache. Using cached model...");
            self.model_cache.refresh(&model_key, CACHE_TTL);
            cached_bytes.clone()
        } else {
            println!("Model not found in cache. Downloading model...");
            let downloaded_bytes = reqwest::blocking::get(&model_key)?.bytes()?.to_vec();
            self.model_cache.insert(model_key.clone(), downloaded_bytes.clone(), CACHE_TTL);
            downloaded_bytes
        };
        //parameters["model"] = serde_json::Value::String(base64::encode(model_bytes));

        let wasm_action = self
            .instance_pres
            .get(container_id)
            .ok_or_else(|| anyhow!(format!("No action named {}", container_id)))?;
        let instance_pre = &wasm_action.module;

        // Manage parameter passing
        let serialized_input = serde_json::to_string(&parameters)?;
        let input = serialized_input.clone();
        //println!("Input: {:?}", input);

        let mut store = create_store(&self.engine);

        let instance = instance_pre.instantiate(&mut store).unwrap();

        // Write the input to the WASM memory
        pass_input(&instance, &mut store, &input)?;

        // Write the model bytes to the WASM memory
        pass_model(&instance, &mut store, &model_bytes)?;

        // Call the _start function
        let main = instance.get_typed_func::<(), ()>(&mut store, "_start").unwrap();
        main.call(&mut store, ())?;

        // Retrieve the result from the WASM memory
        let result = retrieve_result(&instance, &mut store)?;
        let response = serde_json::from_str(&result)?;

        Ok(Ok(response))
    }

    fn destroy(&self, container_id: &str) {
        if let None = self.instance_pres.remove(container_id) {
            println!("No container with id {} existed.", container_id);
        }
    }

}

