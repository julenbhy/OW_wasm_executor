<div align="center">
  <h1>WebAssembly-flavored OpenWhisk</h1>

<strong>A WebAssembly-based container runtime for the Apache OpenWhisk serverless platform.
</strong>
</div>

This repository is an updated version of [wow](https://github.com/PhilippGackstatter/wow/tree/master), now using Wasmtime 25 and including new capabilities such as `instance_pre`.

## Crates Overview

The project is split into multiple crates, which are:

- `ow-common` contains common types such as the `WasmRuntime` trait or types that represent OpenWhisk payloads.
- `ow-executor` implements the actual container runtime and the OpenWhisk runtime protocol.
- `ow-wasmtime` implements the `WasmRuntime` trait for [Wasmtime](https://github.com/bytecodealliance/wasmtime).

## Tutorial with Wasmtime

As a small tutorial, let's build the wasmtime executor and run one of the examples.

1. Install wsk-cli from https://github.com/apache/openwhisk-cli/releases/tag/1.2.0


2. Clone the openwhisk repo, checkout the appropriate branch and run OpenWhisk in a separate terminal:

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

3. In a new terminal, run the desired wasmtime executor with the following command from the root of this repository (change "wasmtime_memory" for the desired execution model):

```sh
cargo run --manifest-path ./ow-executor/Cargo.toml --release --features wasmtime_memory
```

4. Next, build the `add` example with:

```sh
./actions/compile.sh actions/add.rs memory
```

This will add all the required dependencies for the selected execution model and compile it using the action builder crate. The script will also add the function to OpenWhisk.

Note that the precompilation step performed by the script requires wasmtime-cli 25.0.2 to be installed

5. Run the test_client to call a burst action:

```sh
python tests/simple_action_client.py
```

6. For benchmarking a function, use the following benchmarking tool:
```sh
python bench/bench.py -f add -s '{"param1": 3, "param2": 1}' -o bench/out.csv
```