<div align="center">
  <h1>WebAssembly-flavored OpenWhisk</h1>

<strong>A WebAssembly-based container runtime for the Apache OpenWhisk serverless platform.
</strong>
</div>

This repository is an updated version of [wow](https://github.com/PhilippGackstatter/wow/tree/master), now using Wasmtime 20 and including new capabilities such as `instance_pre`.

AAA
## Crates Overview

The project is split into multiple crates, which are:

- `ow-common` contains common types such as the `WasmRuntime` trait or types that represent OpenWhisk payloads.
- `ow-executor` implements the actual container runtime and the OpenWhisk runtime protocol.
- `ow-wasmtime` implements the `WasmRuntime` trait for [Wasmtime](https://github.com/bytecodealliance/wasmtime).
- `ow-wasm-action` contains abstractions for building WebAssembly serverless functions ("actions" in jOpenWhisk terminology) and has a number of example actions.
- `ow-wasmtime-precompiler` implements Ahead-of-Time compilation for `wasmtime`.

## Tutorial with Wasmtime

As a small tutorial, let's build the wasmtime executor and run one of the examples.

1. To build the executor with wasmtime run the following command from the root of this repository:

```sh
cargo build --manifest-path ./ow-executor/Cargo.toml --release --features wasmtime_rt
```

2. Next we build the `add` example for the `wasm32-wasi` target with:

```sh
cargo build --manifest-path ./ow-wasm-action/Cargo.toml --release --example add --target wasm32-wasi --no-default-features --features wasm

# Optional step to optimize the compiled Wasm if `wasm-opt` is installed
# On Ubuntu it can be installed with `sudo apt install binaryen`
wasm-opt -O4 -o ./target/wasm32-wasi/release/examples/add.wasm ./target/wasm32-wasi/release/examples/add.wasm
```

3. Precompile the example for efficient execution with wasmtime:

```sh
./wasmtime_precompile.sh target/wasm32-wasi/release/examples/add.wasm
# The module has to be precompiled with the same version of wasmtime that the embedder uses (wasmtime 21.0.1)
```

4. Install wsk-cli from https://github.com/apache/openwhisk-cli/releases/tag/1.2.0

5. Clone the openwhisk repo, checkout the appropriate branch and run OpenWhisk in a separate terminal:

```sh
git clone git@github.com:PhilippGackstatter/openwhisk.git
git checkout burst-openwasm
./gradlew core:standalone:bootRun
```

This will print something like the following:

```
[ WARN  ] Configure wsk via below command to connect to this server as [guest]

wsk property set --apihost 'http://172.17.0.1:3233' --auth '23bc46b1-71f6-4ed5-8c54-816aa4f8c502:123zO3xZCLrMN6v2BKK1dXYFpXlPkccOFqm12CdAsMgRU4VrNZ9lyGVCGuMDGIwP'
```

Execute this command.

6. Run the executor in a separate terminal. OpenWhisk will forward execution requests for Wasm to it:

```sh
./target/release/executor
```

7. Upload the example zip to OpenWhisk:

```sh
wsk action create --kind wasm:0.1 add ./target/wasm32-wasi/release/examples/add-wasmtime.zip
```

8. Run the test_client to call a burst action:

```sh
python parallel_action_client.py
```
