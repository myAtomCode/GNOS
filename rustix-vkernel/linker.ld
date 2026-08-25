OUTPUT_FORMAT(elf64-x86-64)
ENTRY(_start_asm)

KERNEL_BASE = 0xffffffff80000000;
BOOT_PHYSICAL_BASE = 1M;

SECTIONS
{
    /* Multiboot enters in 32-bit mode, so its trampoline must remain below 4 GiB. */
    . = BOOT_PHYSICAL_BASE;
    boot_start = .;

    .multiboot_header ALIGN(8) :
    {
        multiboot_start = .;
        KEEP(*(.multiboot_header))
        multiboot_end = .;
    }

    .boot.text ALIGN(4096) :
    {
        *(.boot.text .boot.text.*)
    }

    .boot.rodata ALIGN(4096) :
    {
        *(.boot.rodata .boot.rodata.*)
    }

    .boot.data ALIGN(4096) :
    {
        *(.boot.data .boot.data.*)
    }

    .boot.bss ALIGN(4096) (NOLOAD) :
    {
        *(.boot.bss .boot.bss.*)
    }
    boot_end = .;

    /* Kernel VMAs are high-half; AT keeps the flat BIOS image physically contiguous. */
    . = KERNEL_BASE + ALIGN(boot_end, 4096);
    kernel_start = .;
    kernel_phys_start = kernel_start - KERNEL_BASE;

    .text ALIGN(4096) : AT(ADDR(.text) - KERNEL_BASE)
    {
        text_start = .;
        *(.text .text.*)
        . = ALIGN(4096);
        text_end = .;
    }

    .rodata ALIGN(4096) : AT(ADDR(.rodata) - KERNEL_BASE)
    {
        rodata_start = .;
        *(.rodata .rodata.*)
        . = ALIGN(4096);
        rodata_end = .;
    }

    .data ALIGN(4096) : AT(ADDR(.data) - KERNEL_BASE)
    {
        data_start = .;
        *(.data .data.*)
        . = ALIGN(4096);
        data_end = .;
    }

    .got ALIGN(4096) : AT(ADDR(.got) - KERNEL_BASE)
    {
        got_start = .;
        *(.got .got.*)
        . = ALIGN(4096);
        got_end = .;
    }

    .bss ALIGN(4096) (NOLOAD) : AT(ADDR(.bss) - KERNEL_BASE)
    {
        bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        bss_end = .;
    }

    kernel_end = .;
    kernel_phys_bss_start = bss_start - KERNEL_BASE;
    kernel_phys_end = kernel_end - KERNEL_BASE;

    /DISCARD/ :
    {
        *(.comment)
        *(.eh_frame)
        *(.eh_frame_hdr)
    }
}
