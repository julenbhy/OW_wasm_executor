use std::{sync::Arc, time::Duration,};
use dashmap::DashMap;
use timedmap::TimedMap;
use anyhow::anyhow;
use ow_common::{ActionCapabilities, WasmAction, WasmRuntime};

use wasmtime::*;
use wasi_common::sync::WasiCtxBuilder;
use wasi_common::WasiCtx;

#[derive(Clone)]
pub struct Wasmtime {
    pub engine: Engine,
    pub instance_pres: Arc<DashMap<String, WasmAction< InstancePre<WasiCtx> >>>,
    pub instance_pre_cache: Arc<TimedMap<u64, InstancePre<WasiCtx>>>
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
            let mut linker: wasmtime::Linker<WasiCtx> = Linker::new(&self.engine);
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
        parameters: serde_json::Value,
    ) -> Result<Result<serde_json::Value, serde_json::Value>, anyhow::Error> {

        let wasm_action = self
            .instance_pres
            .get(container_id)
            .ok_or_else(|| anyhow!(format!("No action named {}", container_id)))?;
        let instance_pre = &wasm_action.module;

        let mut store = create_store(&self.engine);

        let instance = instance_pre.instantiate(&mut store).unwrap();

        // Write the input to the WASM memory
        pass_input(&instance, &mut store, &parameters)?;

        // Call the _start function
        let main = instance.get_typed_func::<(), ()>(&mut store, "_start").unwrap();
        main.call(&mut store, ())?;

        // Retrieve the result from the WASM memory
        let result = retrieve_result(&instance, &mut store)?;

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
) -> Store<WasiCtx> {
    let wasi = WasiCtxBuilder::new()
        .inherit_stdio()
        .inherit_stderr()
        .build();

    Store::new(engine, wasi)
}


fn link_host_functions(
    linker: &mut wasmtime::Linker<WasiCtx>
) -> Result<(), anyhow::Error> {
    wasi_common::sync::add_to_linker(linker, |s| s)?;
    Ok(())
}


fn pass_input(
    instance: &wasmtime::Instance, 
    store: &mut Store<WasiCtx>, 
    parameters: &serde_json::Value
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
    store: &mut Store<WasiCtx>
) -> Result<serde_json::Value> {

    // Acces the WASM memory
    let memory = instance
        .get_memory(&mut *store, "memory")
        .ok_or_else(|| anyhow::anyhow!("Failed to get WASM memory"))?;

    // Obtain the length of the result with get_result_len
    let get_result_len = instance
        .get_typed_func::<(), u32>(&mut *store, "get_result_len")
        .map_err(|_| anyhow::anyhow!("Failed to get get_result_len"))?;
    let length = get_result_len.call(&mut *store, ())? as usize;

    // Obtain the pointer to the result with get_result
    let get_result = instance
        .get_typed_func::<(), u32>(&mut *store, "get_result")
        .map_err(|_| anyhow::anyhow!("Failed to get get_result"))?;
    let content_ptr = get_result.call(&mut *store, ())? as usize;

    // Read the result from the WASM memory
    let content = memory.data(&store)[content_ptr..(content_ptr + length)].to_vec();
    let result = String::from_utf8(content)?;

    let json_result: serde_json::Value = serde_json::from_str(&result)?;

    Ok(json_result)
}