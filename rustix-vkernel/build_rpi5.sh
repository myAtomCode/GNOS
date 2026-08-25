#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TARGET_DIR="$SCRIPT_DIR/target"
BUILD_DIR="$SCRIPT_DIR/build/rpi5"
KERNEL_ELF="$TARGET_DIR/aarch64-unknown-none/release/v-kernel"
KERNEL_IMAGE="$BUILD_DIR/kernel_2712.img"
DTB_NAME="bcm2712-rpi-5-b.dtb"
DTB_SOURCE="${1:-${RPI5_DTB:-}}"

export CARGO_TARGET_DIR="$TARGET_DIR"

if ! command -v cargo >/dev/null 2>&1; then
    echo "Error: missing required tool: cargo"
    exit 1
fi

if command -v llvm-objcopy >/dev/null 2>&1; then
    OBJCOPY=llvm-objcopy
elif command -v rust-objcopy >/dev/null 2>&1; then
    OBJCOPY=rust-objcopy
elif command -v objcopy >/dev/null 2>&1; then
    OBJCOPY=objcopy
else
    echo "Error: missing required tool: llvm-objcopy, rust-objcopy, or objcopy"
    exit 1
fi

if [ -z "$DTB_SOURCE" ]; then
    for candidate in "/boot/firmware/$DTB_NAME" "/boot/$DTB_NAME"; do
        if [ -f "$candidate" ]; then
            DTB_SOURCE="$candidate"
            break
        fi
    done
fi

mkdir -p "$BUILD_DIR"
cd "$SCRIPT_DIR"

echo "Building Rustix v-kernel Raspberry Pi 5 image..."
RUSTIX_AARCH64_RUSTFLAGS="-C link-arg=-Tlinker-aarch64.ld -C link-arg=--defsym=__kernel_load_addr=0x80000 -C relocation-model=static"
env RUSTFLAGS="$RUSTIX_AARCH64_RUSTFLAGS" \
    cargo build --target aarch64-unknown-none --release \
    --no-default-features --features board-rpi5
"$OBJCOPY" -O binary "$KERNEL_ELF" "$KERNEL_IMAGE"
cp "$SCRIPT_DIR/boards/raspberry-pi-5/config.txt" "$BUILD_DIR/config.txt"

if [ -n "$DTB_SOURCE" ]; then
    if [ ! -f "$DTB_SOURCE" ]; then
        echo "Error: DTB not found: $DTB_SOURCE"
        exit 1
    fi
    cp "$DTB_SOURCE" "$BUILD_DIR/$DTB_NAME"
    echo "  $BUILD_DIR/$DTB_NAME"
else
    echo "Warning: $DTB_NAME was not found; use the copy supplied by Raspberry Pi firmware."
    echo "         Or rebuild with: ./build_rpi5.sh /path/to/$DTB_NAME"
fi

echo "Built firmware files:"
echo "  $KERNEL_IMAGE"
echo "  $BUILD_DIR/config.txt"
