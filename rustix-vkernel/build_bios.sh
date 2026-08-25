#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TARGET_DIR="$SCRIPT_DIR/target"
BUILD_DIR="$SCRIPT_DIR/build"
IMAGE="$BUILD_DIR/rustix-vkernel-bios.img"
BOOT_BIN="$BUILD_DIR/boot_sector.bin"
STAGE2_BIN="$BUILD_DIR/bios_stage2.bin"
KERNEL_BIN="$BUILD_DIR/vkernel.bin"
KERNEL_ELF="$TARGET_DIR/x86_64-unknown-none/release/v-kernel"

export CARGO_TARGET_DIR="$TARGET_DIR"

if [ "${RUSTIX_COMPAT_MONITOR:-0}" = "1" ]; then
    echo "Error: the GUI kernel is larger than the BIOS low-memory staging area."
    echo "Build and run the Multiboot2 GUI image instead:"
    echo "  ./build_iso.sh gui"
    echo "  ./run_qemu.sh gui"
    exit 2
fi

for tool in cargo nasm objcopy readelf dd stat; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "Error: missing required tool: $tool"
        exit 1
    fi
done

mkdir -p "$BUILD_DIR"
cd "$SCRIPT_DIR"

echo "Building Rustix v-kernel BIOS image..."
"$SCRIPT_DIR/userland/build_busybox.sh"
export RUSTIX_BUSYBOX_PATH="$BUILD_DIR/userland/busybox"
FEATURE_ARGS=(--features busybox)
cargo build --target x86_64-unknown-none --release "${FEATURE_ARGS[@]}"

# BIOS stage2 already supplies its own low trampoline and page tables. Keeping
# the ELF-only Multiboot sections out of the flat payload saves low-memory
# staging space without changing the GRUB image.
objcopy -O binary \
    --only-section=.text \
    --only-section=.rodata \
    --only-section=.data \
    --only-section=.got \
    "$KERNEL_ELF" "$KERNEL_BIN"
KERNEL_SIZE=$(stat -c%s "$KERNEL_BIN")
KERNEL_BUFFER_CAPACITY=$((0xA0000 - 0x2000))
if [ "$KERNEL_SIZE" -gt "$KERNEL_BUFFER_CAPACITY" ]; then
    echo "Error: kernel binary exceeds BIOS staging buffer: $KERNEL_SIZE > $KERNEL_BUFFER_CAPACITY bytes"
    exit 1
fi
KERNEL_SECTORS=$(( (KERNEL_SIZE + 511) / 512 ))
KERNEL_ENTRY=$(readelf -s "$KERNEL_ELF" | awk '/ _start$/ { print "0x"$2; exit }')
KERNEL_LOAD_ADDR=$(readelf -s "$KERNEL_ELF" | awk '/ kernel_phys_start$/ { print "0x"$2; exit }')
KERNEL_BSS_START=$(readelf -s "$KERNEL_ELF" | awk '/ kernel_phys_bss_start$/ { print "0x"$2; exit }')
KERNEL_BSS_END=$(readelf -s "$KERNEL_ELF" | awk '/ kernel_phys_end$/ { print "0x"$2; exit }')
KERNEL_STACK_GUARD=$(readelf -s "$KERNEL_ELF" | awk '/ __stack_chk_guard$/ { print "0x"$2; exit }')

STAGE2_SECTORS=1
while true; do
    nasm -f bin "$SCRIPT_DIR/bios_stage2.asm" -o "$STAGE2_BIN" \
        -d KERNEL_SECTORS="$KERNEL_SECTORS" \
        -d KERNEL_ENTRY="$KERNEL_ENTRY" \
        -d KERNEL_LOAD_ADDR="$KERNEL_LOAD_ADDR" \
        -d KERNEL_BSS_START="$KERNEL_BSS_START" \
        -d KERNEL_BSS_END="$KERNEL_BSS_END" \
        -d KERNEL_STACK_GUARD="$KERNEL_STACK_GUARD" \
        -d STAGE2_SECTORS="$STAGE2_SECTORS"

    STAGE2_SIZE=$(stat -c%s "$STAGE2_BIN")
    NEXT_STAGE2_SECTORS=$(( (STAGE2_SIZE + 511) / 512 ))
    if [ "$NEXT_STAGE2_SECTORS" -eq "$STAGE2_SECTORS" ]; then
        break
    fi
    STAGE2_SECTORS="$NEXT_STAGE2_SECTORS"
done

echo "  kernel size: $KERNEL_SIZE bytes"
echo "  kernel sectors: $KERNEL_SECTORS"
echo "  kernel entry: $KERNEL_ENTRY"
echo "  kernel load address: $KERNEL_LOAD_ADDR"
echo "  stage2 sectors: $STAGE2_SECTORS"

nasm -f bin "$SCRIPT_DIR/src/boot_sector.asm" -o "$BOOT_BIN" -d STAGE2_SECTORS="$STAGE2_SECTORS"

dd if=/dev/zero of="$IMAGE" bs=512 count=2880 status=none
dd if="$BOOT_BIN" of="$IMAGE" conv=notrunc status=none
dd if="$STAGE2_BIN" of="$IMAGE" bs=512 seek=1 conv=notrunc status=none
dd if="$KERNEL_BIN" of="$IMAGE" bs=512 seek=$((1 + STAGE2_SECTORS)) conv=notrunc status=none

echo "Built:"
echo "  $IMAGE"
echo "Run:"
echo "  ./run_qemu.sh headless"
