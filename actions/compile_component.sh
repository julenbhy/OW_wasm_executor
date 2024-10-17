#!/bin/bash

set -e

WASMTIME=${WASMTIME_PATH:-"wasmtime"}

# Check if the necessary arguments are passed
# Parsers can be args, stdio, memory, or component
if [ "$#" -ne 1 ]; then
    echo "Usage: $0 <filename>"
    exit 1
fi

# Input variables
INPUT_FILE="$1"        # Original filename

# Check if the version matches the required version (21.0.1)
if [ "$($WASMTIME --version)" != "wasmtime-cli 21.0.1 (cedf9aa0f 2024-05-22)" ]; then
    echo "The version of wasmtime is not 21.0.1. Please install the correct version."
    exit 1
fi

FILENAME=$(basename "$INPUT_FILE" .rs) # Filename without the path and extension
BUILDER="action-builder-component"
RUNTIME="wasmtime"

# Check if the file containing the bytes exists
if [ ! -f "$INPUT_FILE" ]; then
    echo "The file '$INPUT_FILE' does not exist."
    exit 1
fi

# Prepare the builder
cp "$BUILDER/Cargo_template.toml" "$BUILDER/Cargo.toml"
# Add the necessary dependencies to the builder
crate_names=$(grep -Eo 'use [a-zA-Z0-9_]+::' "$INPUT_FILE" | awk '{print $2}' | sed 's/::$//' | sort | uniq)
for crate in $crate_names; do
  if ! grep -q "^$crate =" Cargo.toml; then
    echo "Adding dependency $crate to Cargo.toml"
    cargo add --manifest-path "$BUILDER/Cargo.toml"  "$crate"
  fi
done

# concat the "func" function from the input file to $BUILDER/src/lib.rs
cp "$BUILDER/src/lib_template.rs" "$BUILDER/src/lib.rs"
cat "$INPUT_FILE" >> "$BUILDER/src/lib.rs"

# Compile the file with the selected parser feature

echo "Compiling with component parser"
cargo component build --manifest-path ./"$BUILDER"/Cargo.toml --release


# Check if the compilation was successful
if [ $? -ne 0 ]; then
    echo "Compilation failed."
    exit 1
fi

mkdir -p "actions/compiled"

# Compile the WASM to a .cwasm file
$WASMTIME compile "target/wasm32-wasi/release/action_component.wasm" -o "./actions/compiled/$FILENAME.cwasm"

# Package the .cwasm file into a zip
zip "./actions/compiled/$FILENAME.zip" "./actions/compiled/$FILENAME.cwasm"

# Deploy to OpenWhisk
wsk action update --kind wasm:0.1 "$FILENAME" "./actions/compiled/$FILENAME.zip"

echo "Action '$FILENAME' updated with parser '$PARSER'."
