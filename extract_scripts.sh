#!/bin/bash
set -e

# Optional: pass features to match how the demo was built
# Usage:
#   ./extract_scripts.sh                  # default (no features)
#   ./extract_scripts.sh assume-op-mul    # with assume-op-mul feature
FEATURES="${1:-}"

# Derive variant name for output subdirectory
VARIANT="${FEATURES:-default}"

echo "Building extract_scripts..."
cargo build --bin extract_scripts --quiet

echo "Extracting scripts from demo transactions (variant: $VARIANT)..."
./target/debug/extract_scripts "$VARIANT"

echo "Done! Scripts saved in ./scripts/$VARIANT/"
ls -la "./scripts/$VARIANT/" | head -10
echo "..."
echo "Total files: $(ls "./scripts/$VARIANT/"*.hex 2>/dev/null | wc -l)"
