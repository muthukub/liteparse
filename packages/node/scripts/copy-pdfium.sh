#!/usr/bin/env bash
#
# Copy the pdfium shared library next to the .node file so it can be found
# at runtime via @loader_path (macOS) or $ORIGIN (Linux).
#
# Usage: ./scripts/copy-pdfium.sh [output-dir]
#   output-dir: directory containing the .node file (default: .)
#
# The script auto-detects the pdfium library location from:
#   1. PDFIUM_LIB_PATH env var (set by CI or user)
#   2. The pdfium-sys build cache (~/.cache/pdfium-rs or ~/Library/Caches/pdfium-rs)

set -euo pipefail

OUTPUT_DIR="${1:-.}"

# Determine OS and library filename
case "$(uname -s)" in
    Darwin*)  DYLIB="libpdfium.dylib" ;;
    Linux*)   DYLIB="libpdfium.so" ;;
    MINGW*|MSYS*|CYGWIN*) DYLIB="pdfium.dll" ;;
    *)        echo "Unsupported OS: $(uname -s)" >&2; exit 1 ;;
esac

# Find the pdfium library
find_pdfium() {
    # 1. Explicit env var
    if [ -n "${PDFIUM_LIB_PATH:-}" ] && [ -f "${PDFIUM_LIB_PATH}/${DYLIB}" ]; then
        echo "${PDFIUM_LIB_PATH}/${DYLIB}"
        return
    fi

    # 2. Vendor directory
    local vendor="$(dirname "$0")/../../vendor/pdfium/release/lib/${DYLIB}"
    if [ -f "$vendor" ]; then
        echo "$vendor"
        return
    fi

    # 3. Build cache
    local cache_base
    case "$(uname -s)" in
        Darwin*) cache_base="$HOME/Library/Caches/pdfium-rs" ;;
        *)       cache_base="${XDG_CACHE_HOME:-$HOME/.cache}/pdfium-rs" ;;
    esac

    # Find most recent cached pdfium
    if [ -d "$cache_base" ]; then
        local found
        found=$(find "$cache_base" -name "$DYLIB" -type f 2>/dev/null | head -1)
        if [ -n "$found" ]; then
            echo "$found"
            return
        fi
    fi

    echo ""
}

PDFIUM_PATH=$(find_pdfium)

if [ -z "$PDFIUM_PATH" ]; then
    echo "Error: Could not find ${DYLIB}. Set PDFIUM_LIB_PATH to the directory containing it." >&2
    exit 1
fi

cp "$PDFIUM_PATH" "${OUTPUT_DIR}/${DYLIB}"
echo "Copied ${PDFIUM_PATH} -> ${OUTPUT_DIR}/${DYLIB}"
