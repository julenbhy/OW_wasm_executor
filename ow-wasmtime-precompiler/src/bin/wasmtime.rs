use std::time::Instant;

use anyhow;
use ow_wasmtime_precompiler::{get_filenames, write_precompiled};
use wasmtime::{Module, Engine};

pub fn precompile_wasmtime(filename: &str) -> anyhow::Result<Vec<u8>> {
    let engine = Engine::default();

    let module = Module::from_file(&engine, filename)?;

    module.serialize()
}

pub fn precompile<F: FnOnce(&str) -> anyhow::Result<Vec<u8>>>(
    filename: &str,
    precompile_fn: F,
    runtime_name: &'static str,
) -> anyhow::Result<()> {
    let precompiled_bytes = precompile_fn(&filename)?;

    write_precompiled(filename, runtime_name, precompiled_bytes)?;

    Ok(())
}

fn main() -> anyhow::Result<()> {
    let filenames = get_filenames();

    for filename in filenames {
        precompile(&filename, precompile_wasmtime, "wasmtime")?;
    }

    Ok(())
}
