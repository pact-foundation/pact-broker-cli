#!/bin/bash

if [ -z "$1" ]; then
    echo "Usage: $0 <target> [--binary-name <name>]"
    exit 1
fi

TARGET=$1
BINARY_NAME="pact-broker-cli"
OUTPUT_DIR="dist"

while [[ $# -gt 0 ]]; do
    key="$1"
    case $key in
        --binary-name)
            BINARY_NAME="$2"
            shift
            shift
            ;;
        --output-dir)
            OUTPUT_DIR="$2"
            shift
            shift
            ;;
        *)
            shift
            ;;
    esac
done
# Rename targets to friendlier names end user names
DIST_TARGET_NAME=${TARGET}
DIST_TARGET_NAME=${DIST_TARGET_NAME//-unknown-/-}
DIST_TARGET_NAME=${DIST_TARGET_NAME//-pc-/-}
DIST_TARGET_NAME=${DIST_TARGET_NAME//-apple-darwin/-macos}

echo "DIST_TARGET_NAME: ${DIST_TARGET_NAME}"
mkdir -p ${OUTPUT_DIR}
## Process executables
echo "Processing executables"
    cp target/${TARGET}/release/${BINARY_NAME} ${OUTPUT_DIR}/${BINARY_NAME}-${DIST_TARGET_NAME}

# Check if files exist in dist folder
echo "Checking dist folder for files"
if [ ! -f "${OUTPUT_DIR}/${BINARY_NAME}-${DIST_TARGET_NAME}" ]; then
    echo "Error: ${BINARY_NAME}-${DIST_TARGET_NAME} does not exist in ${OUTPUT_DIR}"
    exit 1
fi

for file in ${OUTPUT_DIR}/*; do
    if [ ! -f "$file" ]; then
        echo "Error: $file does not exist in ${OUTPUT_DIR}"
        exit 1
    fi
done

echo DIST_TARGET_NAME=${DIST_TARGET_NAME} >> $GITHUB_ENV