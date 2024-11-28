use std::{sync::Arc, time::Duration,};
use dashmap::DashMap;
use timedmap::TimedMap;
use anyhow::anyhow;
use serde_json::Value;
use reqwest;
use base64;


use ow_common::{ActionCapabilities, WasmAction, WasmRuntime};

use wasmtime::{Engine, Store};
use wasmtime::component::{Linker, Component, InstancePre};
use wasmtime_wasi::{WasiCtx, WasiView, WasiCtxBuilder, ResourceTable};

use wasmtime_wasi_nn::wit::{WasiNnCtx, WasiNnView};




#[derive(Clone)]
pub struct Wasmtime {
    pub engine: Engine,
    pub instance_pres: Arc<DashMap<String, WasmAction< InstancePre<MyState> >>>,
    pub instance_pre_cache: Arc<TimedMap<u64, InstancePre<MyState>>>,
}

impl Default for Wasmtime {
    fn default() -> Self {
        Self {
            engine: Engine::default(),
            instance_pres: Arc::new(DashMap::new()),
            instance_pre_cache: Arc::new(TimedMap::new()),
        }
    }
}

const CACHE_TTL: Duration = Duration::from_secs(60);


pub struct MyState {
    ctx: WasiCtx,
    wasi_nn: WasiNnCtx,
    table: ResourceTable,
}

impl WasiView for MyState {
    fn ctx(&mut self) -> &mut WasiCtx { 
        &mut self.ctx 
    }

    fn table(&mut self) -> &mut ResourceTable { 
        &mut self.table 
    }
}

impl MyState {
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
            let module = unsafe { match Component::deserialize(&self.engine, &module) {
                                    Ok(module) => module,
                                    Err(e) => {
                                        println!("\x1b[31mError deserializing module: {}\x1b[0m", e);
                                        return Err(anyhow!("Error deserializing module"));
                                    }
                                }};

            // Add WASI to the linker
            let mut linker = Linker::<MyState>::new(&self.engine);
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

        let wasm_action = self
            .instance_pres
            .get(container_id)
            .ok_or_else(|| anyhow!(format!("No action named {}", container_id)))?;
        let instance_pre = &wasm_action.module;

        let mut store = create_store(&self.engine);

        let instance = instance_pre.instantiate(&mut store).unwrap();

        // Manage parameter passing
        println!("Replacing model URL with actual data...");
        replace_model_url(&mut parameters)?;
        println!("Replacing image URLs with actual data...");
        replace_image_urls(&mut parameters)?;

        let input = serde_json::to_string(&parameters)?;
        let mut output = [wasmtime::component::Val::String("".into())];

        // Call the `func-wrapper` function
        println!("Calling func-wrapper function...");
        let func = instance.get_func(&mut store, "func-wrapper").expect("func-wrapper export not found");
        func.call(&mut store, &[wasmtime::component::Val::String(input.into())], &mut output)?;

        // Manage output
        let response = match &output[0] {
            wasmtime::component::Val::String(s) => serde_json::from_str(s).unwrap(),
            _ => serde_json::Value::Null,
        };

        Ok(Ok(response))
    }

    fn destroy(&self, container_id: &str) {
        if let None = self.instance_pres.remove(container_id) {
            println!("No container with id {} existed.", container_id);
        }
    }
}


fn create_store(
    engine: &Engine,
) -> Store<MyState> {

    let wasi = WasiCtxBuilder::new()
        .inherit_stdio()
        .inherit_stderr()
        .build();

    let (backends, registry) = wasmtime_wasi_nn::preload(&[]).unwrap();
    let wasi_nn = WasiNnCtx::new(backends, registry);

    let wasi_state = MyState { 
        ctx: wasi, 
        table: ResourceTable::new(),
        wasi_nn: wasi_nn,
    };

    Store::new(engine, wasi_state)
}


fn link_host_functions(
    linker: &mut Linker<MyState>
) -> Result<(), anyhow::Error> {
    wasmtime_wasi::add_to_linker_sync(linker)?;
    wasmtime_wasi_nn::wit::add_to_linker(linker, |state: &mut MyState| WasiNnView::new(&mut state.table, &mut state.wasi_nn))?;
    Ok(())
}



// This just replaces the 'model' URL with the actual model bytes (always a single model)
fn replace_model_url(
    parameters: &mut Value
) -> anyhow::Result<()> {
    if let Some(model_value) = parameters.get_mut("model") {
        if let Some(model_url) = model_value.as_str() {
            let model_bytes = reqwest::blocking::get(model_url)?.bytes()?.to_vec();
            *model_value = Value::String(base64::encode(&model_bytes));
        } else {
            return Err(anyhow!("From embedder: 'model' is not a string"));
        }
    } else {
        return Err(anyhow!("From embedder: 'model' not found in JSON"));
    }
    Ok(())
}



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