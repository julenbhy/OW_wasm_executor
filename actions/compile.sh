#!/bin/bash

set -e

WASMTIME=${WASMTIME_PATH:-"/opt/wasmtime-v27.0.0-x86_64-linux/wasmtime"}
export WASMTIME


# Supported methods
INPUT_METHODS=("memory" "memory_nn" "component" "component_nn" "memory_nn_parallel")


# Check if the necessary arguments are passed
# methods can be args, stdio, memory, or component
if [ "$#" -ne 2 ]; then
    echo "Usage: $0 <filename> <input_method>"
    echo "Supported argument passing methods: ${INPUT_METHODS[*]}"
    exit 1
fi

# Input variables
INPUT_FILE="$1"        # Original filename
METHOD="$2"            # Method selected by the user

# Check if the selected method is valid
if [[ ! " ${INPUT_METHODS[@]} " =~ " ${METHOD} " ]]; then
    echo "Invalid method: $METHOD. Supported methods are: ${INPUT_METHODS[*]}"
    exit 1
fi

# Check if the version matches the required version (27.0.0)
if [ "$($WASMTIME --version)" != "wasmtime 27.0.0 (8eefa236f 2024-11-20)" ]; then
    echo "The version of wasmtime is not 27.0.0. Please install the correct version."
    exit 1
fi

# If the METHOD is component or component_nn, call compile_component.sh $INPUT_FILE
if [ "$METHOD" == "component" ] || [ "$METHOD" == "component_nn" ]; then
    ./actions/compile_component.sh "$INPUT_FILE" "$METHOD"
    exit 0
fi

FILENAME=$(basename "$INPUT_FILE" .rs) # Filename without the path and extension
BUILDER="action-builder"

# Check if the file containing the bytes exists
if [ ! -f "$INPUT_FILE" ]; then
    echo "The file '$INPUT_FILE' does not exist."
    exit 1
fi

# Prepare the builder
cp "$BUILDER/Cargo_template.toml" "$BUILDER/Cargo.toml"

# Add the necessary dependencies to the builder
crate_names=$(grep -Eo 'use [a-zA-Z0-9_]+(::)?' "$INPUT_FILE" | awk '{print $2}' | sed 's/::$//' | sort | uniq)
pwd
echo "Detected dependencies: $crate_names"
for crate in $crate_names; do
  if ! grep -q "^$crate =" $BUILDER/Cargo.toml; then
    echo "Adding dependency $crate to Cargo.toml"
    if ! cargo add --manifest-path "$BUILDER/Cargo.toml" "$crate"; then
      echo "Failed to add crate '$crate'. It may not be compatible or required."
    fi
  else
    echo "Dependency $crate already added to Cargo.toml" 
  fi
done

# Copy the file to the builder
mkdir -p "$BUILDER/examples/"
cp "$INPUT_FILE" "$BUILDER/examples/"

# Add the METHOD feature to the builder
sed -i "1i action_builder::${METHOD}_method!(func);" "$BUILDER/examples/$FILENAME.rs"


# Determine the feature based on the selected METHOD
FEATURE="${METHOD}_method"

# Compile the file with the selected METHOD feature
echo "Compiling with $METHOD method."
cargo build --manifest-path ./"$BUILDER"/Cargo.toml --release --example "$FILENAME" --target wasm32-wasip1


# Check if the compilation was successful
if [ $? -ne 0 ]; then
    echo "Compilation failed."
    exit 1
fi

mkdir -p "actions/compiled"

# Compile the WASM to a .cwasm file
$WASMTIME compile "target/wasm32-wasip1/release/examples/$FILENAME.wasm" -o "./actions/compiled/$FILENAME.cwasm"

# Package the .cwasm file into a zip
zip "./actions/compiled/$FILENAME.zip" "./actions/compiled/$FILENAME.cwasm"

# Deploy to OpenWhisk
wsk action update --kind wasm:0.1 "$FILENAME" "./actions/compiled/$FILENAME.zip"

echo "Action '$FILENAME' updated with '$METHOD' argument passing method."
