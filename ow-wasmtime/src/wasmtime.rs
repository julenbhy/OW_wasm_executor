use std::{sync::Arc, 
    // time::Instant,
};

use dashmap::DashMap;
use anyhow::anyhow;
use ow_common::{ActionCapabilities, WasmAction, WasmRuntime};
use wasmtime::*;
use wasi_common::sync::WasiCtxBuilder;
use wasi_common::WasiCtx;
use wasi_common::pipe::ReadPipe;
use wasi_common::pipe::WritePipe;


#[derive(Clone)]
pub struct Wasmtime {
    pub engine: Engine,
    pub modules: Arc<DashMap<String, WasmAction< InstancePre<WasiCtx> >>>,
}

impl Default for Wasmtime {
    fn default() -> Self {
        Self {
            engine: Engine::default(),
            modules: Arc::new(DashMap::new()),
        }
    }
}

impl WasmRuntime for Wasmtime {
    fn initialize(
        &self,
        container_id: String,
        capabilities: ActionCapabilities,
        module: Vec<u8>,
    ) -> anyhow::Result<()> {
        
        // deserialize could fail due to https://docs.wasmtime.dev/api/wasmtime/struct.Module.html#method.deserialize Unsafety
        // module must've been precompiled with a matching version of wasmtime
        let module = unsafe {
            match Module::deserialize(&self.engine, &module) {
                Ok(module) => module,
                Err(e) => {
                    println!("\x1b[31mError deserializing module: {}\x1b[0m", e);
                    return Err(anyhow!("Error deserializing module"));
                }
            }
        };

        // Add WASI to the linker
        let mut linker: wasmtime::Linker<WasiCtx> = Linker::new(&self.engine);
        wasi_common::sync::add_to_linker(&mut linker, |s| s)?;

        let instance_pre = linker.instantiate_pre(&module)?;

        let action = WasmAction {
            module: instance_pre,
            capabilities,
        };

        // TODO:
        //      This should be replaced checking if the same module has allready been precompiled for another container_id.
        //      Multiple containers should be able to use the same precompiled module.
        self.modules.insert(container_id.clone(), action);

        Ok(())
    }


    fn run(
        &self,
        container_id: &str,
        parameters: serde_json::Value,
    ) -> Result<Result<serde_json::Value, serde_json::Value>, anyhow::Error> {

        let wasm_action = self
            .modules
            .get(container_id)
            .ok_or_else(|| anyhow!(format!("No action named {}", container_id)))?;
        let instance_pre = &wasm_action.module;

        // Manage parameter passing
        let serialized_input = serde_json::to_string(&parameters)?;
        let stdin = ReadPipe::from(serialized_input);
        let stdout = WritePipe::new_in_memory();

        // Create a WASI context and put it in a Store
        let wasi = WasiCtxBuilder::new()
            .stdin(Box::new(stdin.clone()))
            .stdout(Box::new(stdout.clone()))
            .inherit_stderr()
            .inherit_args()?
            .build();

        let mut store = Store::new(&self.engine, wasi);

        let instance = instance_pre.instantiate(&mut store).unwrap();

        let main = instance.get_typed_func::<(), ()>(&mut store, "_start").unwrap();
        
        main.call(&mut store, ())?;

        // Manage output
        drop(store);

        let contents: Vec<u8> = stdout
            .try_into_inner()
            .map_err(|_err| anyhow::Error::msg("sole remaining reference"))?
            .into_inner();

        let output: Output = serde_json::from_slice(&contents)?;

        let response = serde_json::json!({
            "response": output.response
        });

        Ok(Ok(response))
    }

    fn destroy(&self, container_id: &str) {
        if let None = self.modules.remove(container_id) {
            println!("No container with id {} existed.", container_id);
        }
    }
}



// TODO: 
//      Right now, the output is hardcoded to be an integer.
//      This should be changed to a generic type that can be serialized and deserialized.
//      This will allow the user to define the output type in the action's manifest.
use serde::{Serialize, Deserialize};
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Output {
    pub response: i32,
}
