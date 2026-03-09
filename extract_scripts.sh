#!/bin/bash
set -e

VARIANT="default"

echo "Building extract_scripts..."
cargo build --bin extract_scripts --quiet

echo "Extracting scripts from demo transactions (variant: $VARIANT)..."
./target/debug/extract_scripts "$VARIANT"

echo "Done! Scripts saved in ./scripts/$VARIANT/"
ls -la "./scripts/$VARIANT/" | head -10
echo "..."
echo "Total files: $(ls "./scripts/$VARIANT/"*.hex 2>/dev/null | wc -l)"
