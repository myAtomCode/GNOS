#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ISO_IMAGE="$SCRIPT_DIR/build/rustix-vkernel.iso"
GUI_ISO_IMAGE="$SCRIPT_DIR/build/rustix-vkernel-gui.iso"
BIOS_IMAGE="$SCRIPT_DIR/build/rustix-vkernel-bios.img"
UI="${1:-window}"

if [ "$UI" = "gui" ]; then
    if [ ! -f "$GUI_ISO_IMAGE" ]; then
        echo "GUI ISO not found: $GUI_ISO_IMAGE"
        echo "Build it first with: ./build_iso.sh gui"
        exit 1
    fi
    exec qemu-system-x86_64 \
        -smp 4 -cpu max -m 512M \
        -cdrom "$GUI_ISO_IMAGE" -boot d \
        -serial stdio -display gtk -vga std \
        -no-reboot -no-shutdown
fi

if [ ! -f "$BIOS_IMAGE" ]; then
    echo "BIOS image not found: $BIOS_IMAGE"
    echo "Build it first with: ./build_bios.sh"
    exit 1
fi

QEMU_ARGS=(
    -drive "format=raw,file=$BIOS_IMAGE,if=floppy"
    -boot a
    -smp 4
    -cpu max
    -serial stdio
    -vga std
    -device isa-debug-exit,iobase=0xf4,iosize=0x04
    -no-reboot
    -no-shutdown
    -m 512M
)

if [ "$UI" = "headless" ]; then
    QEMU_ARGS+=(-display none)
else
    QEMU_ARGS+=(-display gtk)
fi

exec qemu-system-x86_64 "${QEMU_ARGS[@]}"
