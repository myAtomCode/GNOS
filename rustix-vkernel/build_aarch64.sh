#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TARGET_DIR="$SCRIPT_DIR/target"
BUILD_DIR="$SCRIPT_DIR/build"
KERNEL_ELF="$TARGET_DIR/aarch64-unknown-none/release/v-kernel"
KERNEL_BIN="$BUILD_DIR/rustix-vkernel-aarch64.bin"

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

mkdir -p "$BUILD_DIR"
cd "$SCRIPT_DIR"

echo "Building Rustix v-kernel ARM64 QEMU virt image..."
cargo build --target aarch64-unknown-none --release --no-default-features --features board-qemu-virt
"$OBJCOPY" -O binary "$KERNEL_ELF" "$KERNEL_BIN"

echo "Built:"
echo "  $KERNEL_ELF"
echo "  $KERNEL_BIN"
echo "Run:"
echo "  qemu-system-aarch64 -machine virt,gic-version=2 -cpu cortex-a72 -smp 4 -m 512M -nographic -kernel $KERNEL_BIN"
