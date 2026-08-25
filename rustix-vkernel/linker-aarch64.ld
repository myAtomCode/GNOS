ENTRY(_start)

SECTIONS
{
    . = DEFINED(__kernel_load_addr) ? __kernel_load_addr : 0x40080000;
    kernel_start = .;

    .text : ALIGN(4096)
    {
        text_start = .;
        *(.text.boot)
        *(.text .text.*)
        text_end = .;
    }

    .rodata : ALIGN(4096)
    {
        rodata_start = .;
        *(.rodata .rodata.*)
        rodata_end = .;
    }

    .data : ALIGN(4096)
    {
        data_start = .;
        *(.data .data.*)
        data_end = .;
    }

    .bss (NOLOAD) : ALIGN(4096)
    {
        bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        bss_end = .;
    }

    .stack (NOLOAD) : ALIGN(4096)
    {
        stack_guard_start = .;
        . += 0x1000;
        stack_guard_end = .;
        stack_bottom = .;
        . += 0x10000;
        stack_top = .;
    }

    kernel_end = .;
    kernel_phys_end = kernel_end;

    /DISCARD/ :
    {
        *(.comment)
        *(.eh_frame)
        *(.eh_frame_hdr)
        *(.note .note.*)
    }
}
