#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TARGET_DIR="$SCRIPT_DIR/target"
BUILD_DIR="$SCRIPT_DIR/build"
ISO_ROOT="$BUILD_DIR/iso_root"
MODE="${1:-headless}"
case "$MODE" in
    gui)
        ISO_ROOT="$BUILD_DIR/iso_root_gui"
        ISO_IMAGE="$BUILD_DIR/rustix-vkernel-gui.iso"
        FEATURES=(--features busybox --features compat-monitor)
        PROFILE_OPT_LEVEL="${CARGO_PROFILE_RELEASE_OPT_LEVEL:-z}"
        ;;
    headless)
        ISO_IMAGE="$BUILD_DIR/rustix-vkernel.iso"
        FEATURES=()
        PROFILE_OPT_LEVEL="${CARGO_PROFILE_RELEASE_OPT_LEVEL:-s}"
        ;;
    *)
        echo "Usage: $0 [headless|gui]"
        exit 2
        ;;
esac
KERNEL_ELF="$TARGET_DIR/x86_64-unknown-none/release/v-kernel"

export CARGO_TARGET_DIR="$TARGET_DIR"

for tool in cargo grub-file grub-mkrescue qemu-system-x86_64; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "Error: missing required tool: $tool"
        exit 1
    fi
done

cd "$SCRIPT_DIR"

echo "Building Rustix v-kernel..."
export CARGO_PROFILE_RELEASE_OPT_LEVEL="$PROFILE_OPT_LEVEL"
if [ "$MODE" = "gui" ]; then
    "$SCRIPT_DIR/userland/build_busybox.sh"
    export RUSTIX_BUSYBOX_PATH="$BUILD_DIR/userland/busybox"
fi
cargo build --target x86_64-unknown-none --release "${FEATURES[@]}"

if [ ! -f "$KERNEL_ELF" ]; then
    echo "Error: missing kernel ELF: $KERNEL_ELF"
    exit 1
fi

if ! grub-file --is-x86-multiboot2 "$KERNEL_ELF"; then
    echo "Error: $KERNEL_ELF is not a valid Multiboot2 image"
    exit 1
fi

rm -rf "$ISO_ROOT"
mkdir -p "$ISO_ROOT/boot/grub"

if [ "$MODE" = "gui" ]; then
    cat > "$ISO_ROOT/boot/grub/grub.cfg" <<'GRUBEOF'
insmod all_video
set gfxmode=640x480x32
set gfxpayload=keep
terminal_input serial
terminal_output gfxterm

set timeout=0
set default=0

menuentry "Rustix v-kernel GUI" {
    multiboot2 /boot/kernel.elf
    boot
}
GRUBEOF
else
    cat > "$ISO_ROOT/boot/grub/grub.cfg" <<'GRUBEOF'
serial --unit=0 --speed=115200 --word=8 --parity=no --stop=1
terminal_input serial
terminal_output serial

set timeout=0
set default=0

menuentry "Rustix v-kernel" {
    multiboot2 /boot/kernel.elf
    boot
}
GRUBEOF
fi

cp "$KERNEL_ELF" "$ISO_ROOT/boot/kernel.elf"

echo "Creating ISO image..."
grub-mkrescue -o "$ISO_IMAGE" "$ISO_ROOT" >/dev/null 2>&1

echo "Built:"
echo "  $ISO_IMAGE"
echo "Run:"
if [ "$MODE" = "gui" ]; then
    echo "  ./run_qemu.sh gui"
else
    echo "  qemu-system-x86_64 -smp 4 -cpu max -cdrom $ISO_IMAGE -serial stdio -display none -m 512M"
fi
