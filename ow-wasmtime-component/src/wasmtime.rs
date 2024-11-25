use std::{sync::Arc, time::Duration,};
use dashmap::DashMap;
use timedmap::TimedMap;
use anyhow::anyhow;

use ow_common::{ActionCapabilities, WasmAction, WasmRuntime};

use wasmtime::{Engine, Store};
use wasmtime::component::{Linker, Component, InstancePre};
use wasmtime_wasi::{WasiCtx, WasiView, WasiCtxBuilder, ResourceTable};





#[derive(Clone)]
pub struct Wasmtime {
    pub engine: Engine,
    pub instance_pres: Arc<DashMap<String, WasmAction< InstancePre<MyState> >>>,
    pub instance_pre_cache: Arc<TimedMap<u64, InstancePre<MyState>>>, // TODO: Remove unused instance_pres after an unusedTimeout
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
        parameters: serde_json::Value,
    ) -> Result<Result<serde_json::Value, serde_json::Value>, anyhow::Error> {

        let wasm_action = self
            .instance_pres
            .get(container_id)
            .ok_or_else(|| anyhow!(format!("No action named {}", container_id)))?;
        let instance_pre = &wasm_action.module;

        let mut store = create_store(&self.engine);

        let instance = instance_pre.instantiate(&mut store).unwrap();

        // Manage parameter passing
        let input = serde_json::to_string(&parameters)?;
        let mut output = [wasmtime::component::Val::String("".into())];

        // Call the `func-wrapper` function
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

    let wasi_state = MyState { 
        ctx: wasi, 
        table: ResourceTable::new() 
    };

    Store::new(engine, wasi_state)
}


fn link_host_functions(
    linker: &mut Linker<MyState>
) -> Result<(), anyhow::Error> {
    wasmtime_wasi::add_to_linker_sync(linker)?;
    Ok(())
}