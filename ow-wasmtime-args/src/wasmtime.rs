use std::{sync::Arc, time::Duration,};
use dashmap::DashMap;
use timedmap::TimedMap;
use anyhow::anyhow;
use ow_common::{ActionCapabilities, WasmAction, WasmRuntime};

use wasmtime::*;
use wasi_common::sync::WasiCtxBuilder;
use wasi_common::WasiCtx;
use wasi_common::pipe::{ReadPipe, WritePipe};

#[derive(Clone)]
pub struct Wasmtime {
    pub engine: Engine,
    pub instance_pres: Arc<DashMap<String, WasmAction< InstancePre<WasiCtx> >>>,
    pub instance_pre_cache: Arc<TimedMap<u64, InstancePre<WasiCtx>>>,
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
            wasi_common::sync::add_to_linker(&mut linker, |s| s)?;

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

        // Manage parameter passing
        let serialized_input = serde_json::to_string(&parameters)?;
        //println!("Input: {:?}", serialized_input);

        // Create a WASI context and put it in a Store
        let wasi = WasiCtxBuilder::new()
            .inherit_stdio()
            .args(&[serialized_input])?
            .build();

        let mut store = Store::new(&self.engine, wasi);

        let instance = instance_pre.instantiate(&mut store).unwrap();

        let main = instance.get_typed_func::<(), ()>(&mut store, "_start").unwrap();
        
        main.call(&mut store, ())?;

        // Get the result from the WASM module execution
        let Some(memory) = instance.get_memory(&mut store, "memory") else { anyhow::bail!("Failed to get WASM memory"); };

        let Ok(get_result_len) = instance.get_typed_func::<(), u32>(&mut store, "get_result_len") else { anyhow::bail!("Failed to get get_result_len");};
        let length = get_result_len.call(&mut store, ())? as usize;

        let Ok(get_result) = instance.get_typed_func::<(), u32>(&mut store, "get_result") else { anyhow::bail!("Failed to get get_result");};
        let content_ptr = get_result.call(&mut store, ())? as usize;

        let content = memory.data(&store)[content_ptr..(content_ptr + length)].to_vec();
        let result = String::from_utf8(content)?;
        let response = serde_json::from_str(&result)?;

        Ok(Ok(response))
    }

    fn destroy(&self, container_id: &str) {
        if let None = self.instance_pres.remove(container_id) {
            println!("No container with id {} existed.", container_id);
        }
    }
}