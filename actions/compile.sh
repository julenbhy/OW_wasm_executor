#!/bin/bash

set -e

WASMTIME=${WASMTIME_PATH:-"/opt/wasmtime-v25.0.2-x86_64-linux/wasmtime"}
export WASMTIME


# Supported parsers
SUPPORTED_PARSERS=("args" "stdio" "memory" "component")


# Check if the necessary arguments are passed
# Parsers can be args, stdio, memory, or component
if [ "$#" -ne 2 ]; then
    echo "Usage: $0 <filename> <parser>"
    echo "Supported parsers: ${SUPPORTED_PARSERS[*]}"
    exit 1
fi

# Input variables
INPUT_FILE="$1"        # Original filename
PARSER="$2"            # Parser selected by the user

# Check if the selected parser is valid
if [[ ! " ${SUPPORTED_PARSERS[@]} " =~ " ${PARSER} " ]]; then
    echo "Invalid parser: $PARSER. Supported parsers are: ${SUPPORTED_PARSERS[*]}"
    exit 1
fi

# Check if the version matches the required version (25.0.2)
if [ "$($WASMTIME --version)" != "wasmtime 25.0.2 (52a565bb9 2024-10-09)" ]; then
    echo "The version of wasmtime is not 25.0.2. Please install the correct version."
    exit 1
fi

# If the parser is component, call compile_component.sh $INPUT_FILE
if [ "$PARSER" == "component" ]; then
    ./actions/compile_component.sh "$INPUT_FILE"
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
crate_names=$(grep -Eo 'use [a-zA-Z0-9_]+::' "$INPUT_FILE" | awk '{print $2}' | sed 's/::$//' | sort | uniq)
for crate in $crate_names; do
  if ! grep -q "^$crate =" Cargo.toml; then
    echo "Adding dependency $crate to Cargo.toml"
    cargo add --manifest-path "$BUILDER/Cargo.toml"  "$crate"
  fi
done

# Copy the file to the builder
mkdir -p "$BUILDER/examples/"
cp "$INPUT_FILE" "$BUILDER/examples/"

# Add the parser feature to the builder
sed -i "1i action_builder::${PARSER}_parser!(func);" "$BUILDER/examples/$FILENAME.rs"


# Determine the feature based on the selected parser
FEATURE="${PARSER}_parser"

# Compile the file with the selected parser feature
echo "Compiling with $PARSER parser"
cargo build --manifest-path ./"$BUILDER"/Cargo.toml --release --example "$FILENAME" --target wasm32-wasi


# Check if the compilation was successful
if [ $? -ne 0 ]; then
    echo "Compilation failed."
    exit 1
fi

mkdir -p "actions/compiled"

# Compile the WASM to a .cwasm file
$WASMTIME compile "target/wasm32-wasi/release/examples/$FILENAME.wasm" -o "./actions/compiled/$FILENAME.cwasm"

# Package the .cwasm file into a zip
zip "./actions/compiled/$FILENAME.zip" "./actions/compiled/$FILENAME.cwasm"

# Deploy to OpenWhisk
wsk action update --kind wasm:0.1 "$FILENAME" "./actions/compiled/$FILENAME.zip"

echo "Action '$FILENAME' updated with parser '$PARSER'."
