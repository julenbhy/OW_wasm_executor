#!/bin/bash

# Check if the necessary arguments are passed
if [ "$#" -ne 1 ]; then
    echo "Usage: $0 <filename> "
    exit 1
fi

# Check if the version matches the required version (21.0.1)
if [ "$(wasmtime --version)" != "wasmtime-cli 21.0.1 (cedf9aa0f 2024-05-22)" ]; then
    echo "The version of wasmtime is not 0.21.0. Please install the correct version."
    exit 1
fi

# Input variables
filename="$1"        # Original filename
runtime_name="wasmtime"

# Check if the file containing the bytes exists
if [ ! -f "$filename" ]; then
    echo "The file with bytes '$filename' does not exist."
    exit 1
fi

# Get the path of the original file and the base name
file_path=$(dirname "$filename")
file_stem=$(basename "$filename" .wasm)
file_cwasm="$file_path/$file_stem-$runtime_name.cwasm"
file_zip="$file_path/$file_stem-$runtime_name.zip"


wasmtime compile "$filename" -o "$file_cwasm"

zip "$file_zip" "$file_cwasm"

# Display message
echo "Zip file created at: $file_zip"
