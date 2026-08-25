#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KERNEL_BIN="$SCRIPT_DIR/build/rustix-vkernel-aarch64.bin"

if [ ! -f "$KERNEL_BIN" ]; then
    echo "ARM64 image not found: $KERNEL_BIN"
    echo "Build it first with: ./build_aarch64.sh"
    exit 1
fi

exec qemu-system-aarch64 \
    -machine virt,gic-version=2 \
    -cpu cortex-a72 \
    -smp "${RUSTIX_CPUS:-4}" \
    -m 512M \
    -nographic \
    -no-reboot \
    -no-shutdown \
    -kernel "$KERNEL_BIN"
