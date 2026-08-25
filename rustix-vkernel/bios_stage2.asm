bits 16
org 0x0600

%ifndef KERNEL_SECTORS
%define KERNEL_SECTORS 1
%endif

%ifndef STAGE2_SECTORS
%define STAGE2_SECTORS 1
%endif

%ifndef KERNEL_ENTRY
%define KERNEL_ENTRY 0x00100000
%endif
%ifndef KERNEL_LOAD_ADDR
%define KERNEL_LOAD_ADDR 0x00100000
%endif
%ifndef KERNEL_BSS_START
%define KERNEL_BSS_START 0
%endif
%ifndef KERNEL_BSS_END
%define KERNEL_BSS_END 0
%endif
%ifndef KERNEL_STACK_GUARD
%define KERNEL_STACK_GUARD 0
%endif

KERNEL_SEG_START   equ 0x0200
STACK_TOP          equ 0x1800
SECTORS_PER_TRACK  equ 18
KERNEL_START_SECTOR equ STAGE2_SECTORS + 2
KERNEL_BUFFER      equ 0x00002000
STACK32_TOP        equ 0x00090000
STACK64_TOP        equ 0x01000000
KERNEL_STACK_TOP   equ 0xffffffff80800000
PML4_ADDR          equ 0x00001000
PDPT_ADDR          equ 0x00002000
PD0_ADDR           equ 0x00003000
PD1_ADDR           equ 0x00004000
PD2_ADDR           equ 0x00005000
PD3_ADDR           equ 0x00006000
KERNEL_PD_ADDR     equ 0x00008000
BOOT_INFO_SCRATCH equ 0x00001000
BOOT_VIDEO_INFO_ADDR equ BOOT_INFO_SCRATCH
VBE_MODE_INFO_ADDR equ BOOT_INFO_SCRATCH + 0x0200
BIOS_HANDOFF_ADDR equ BOOT_INFO_SCRATCH + 0x03C0
E820_BUFFER_ADDR equ BOOT_INFO_SCRATCH + 0x0400
BOOT_INFO_FINAL equ 0x00090000
BIOS_HANDOFF_FINAL equ BOOT_INFO_FINAL + 0x03C0
E820_BUFFER_FINAL equ BOOT_INFO_FINAL + 0x0400
BOOT_INFO_COPY_SIZE equ 0x0A00
E820_MAX_ENTRIES equ 64
BOOT_VIDEO_MAGIC   equ 0x52564944
PROTOCOL_BIOS      equ 0x52584249
IA32_EFER          equ 0xC0000080
VESA_MODE          equ 0x0112
VESA_MODE_LFB      equ 0x4112

start:
    cli
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, STACK_TOP
    mov [boot_drive], dl

    mov ax, KERNEL_SEG_START
    mov es, ax
    xor bx, bx
    mov si, KERNEL_SECTORS
    xor ch, ch
    xor dh, dh
    mov cl, KERNEL_START_SECTOR

.read_loop:
    test si, si
    jz .kernel_loaded

    mov ah, 0x02
    mov al, 0x01
    mov dl, [boot_drive]
    int 0x13
    jc disk_error

    mov ax, es
    add ax, 0x20
    mov es, ax
    dec si
    inc cl
    cmp cl, SECTORS_PER_TRACK + 1
    jne .read_loop

    mov cl, 0x01
    xor dh, 0x01
    jnz .read_loop

    inc ch
    jmp .read_loop

.kernel_loaded:
    call init_memory_map
    call init_video
    call enable_a20
    lgdt [gdt_descriptor]

    mov eax, cr0
    or eax, 1
    mov cr0, eax
    jmp 0x08:protected_mode

disk_error:
    mov si, disk_error_msg
    call print_string
.hang:
    hlt
    jmp .hang

print_string:
    lodsb
    test al, al
    jz .done
    mov ah, 0x0E
    mov bx, 0x0007
    int 0x10
    jmp print_string
.done:
    ret

init_video:
    mov dword [BOOT_VIDEO_INFO_ADDR], BOOT_VIDEO_MAGIC
    mov dword [BOOT_VIDEO_INFO_ADDR + 4], 0x000A0000
    mov word [BOOT_VIDEO_INFO_ADDR + 8], 320
    mov word [BOOT_VIDEO_INFO_ADDR + 10], 200
    mov word [BOOT_VIDEO_INFO_ADDR + 12], 320
    mov byte [BOOT_VIDEO_INFO_ADDR + 14], 8
    mov byte [BOOT_VIDEO_INFO_ADDR + 15], 0
    mov byte [BOOT_VIDEO_INFO_ADDR + 16], 0
    mov byte [BOOT_VIDEO_INFO_ADDR + 17], 0
    mov byte [BOOT_VIDEO_INFO_ADDR + 18], 0
    mov byte [BOOT_VIDEO_INFO_ADDR + 19], 0
    mov byte [BOOT_VIDEO_INFO_ADDR + 20], 0
    mov byte [BOOT_VIDEO_INFO_ADDR + 21], 0

    mov ax, 0x4F01
    mov cx, VESA_MODE
    xor bx, bx
    mov es, bx
    mov di, VBE_MODE_INFO_ADDR
    int 0x10
    cmp ax, 0x004F
    jne video_fallback

    test word [VBE_MODE_INFO_ADDR], 0x0080
    jz video_fallback

    mov ax, 0x4F02
    mov bx, VESA_MODE_LFB
    int 0x10
    cmp ax, 0x004F
    jne video_fallback

    mov eax, [VBE_MODE_INFO_ADDR + 40]
    mov [BOOT_VIDEO_INFO_ADDR + 4], eax
    mov ax, [VBE_MODE_INFO_ADDR + 18]
    mov [BOOT_VIDEO_INFO_ADDR + 8], ax
    mov ax, [VBE_MODE_INFO_ADDR + 20]
    mov [BOOT_VIDEO_INFO_ADDR + 10], ax
    mov ax, [VBE_MODE_INFO_ADDR + 16]
    mov [BOOT_VIDEO_INFO_ADDR + 12], ax
    mov al, [VBE_MODE_INFO_ADDR + 25]
    mov [BOOT_VIDEO_INFO_ADDR + 14], al
    mov al, [VBE_MODE_INFO_ADDR + 31]
    mov [BOOT_VIDEO_INFO_ADDR + 16], al
    mov al, [VBE_MODE_INFO_ADDR + 32]
    mov [BOOT_VIDEO_INFO_ADDR + 17], al
    mov al, [VBE_MODE_INFO_ADDR + 33]
    mov [BOOT_VIDEO_INFO_ADDR + 18], al
    mov al, [VBE_MODE_INFO_ADDR + 34]
    mov [BOOT_VIDEO_INFO_ADDR + 19], al
    mov al, [VBE_MODE_INFO_ADDR + 35]
    mov [BOOT_VIDEO_INFO_ADDR + 20], al
    mov al, [VBE_MODE_INFO_ADDR + 36]
    mov [BOOT_VIDEO_INFO_ADDR + 21], al
    ret

init_memory_map:
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov dword [BIOS_HANDOFF_ADDR + 0], 0x4D454D31
    mov dword [BIOS_HANDOFF_ADDR + 4], 0x52555354
    mov dword [BIOS_HANDOFF_ADDR + 8], 1
    mov dword [BIOS_HANDOFF_ADDR + 12], 24
    mov dword [BIOS_HANDOFF_ADDR + 16], 0
    mov dword [BIOS_HANDOFF_ADDR + 20], 0
    mov dword [BIOS_HANDOFF_ADDR + 24], E820_BUFFER_FINAL
    mov dword [BIOS_HANDOFF_ADDR + 28], 0

    xor ebx, ebx
    mov di, E820_BUFFER_ADDR
.next:
    cmp dword [BIOS_HANDOFF_ADDR + 16], E820_MAX_ENTRIES
    jae .done
    mov eax, 0xE820
    mov edx, 0x534D4150
    mov ecx, 24
    mov dword [di + 20], 1
    int 0x15
    jc .done
    cmp eax, 0x534D4150
    jne .done
    cmp ecx, 20
    jb .done
    inc dword [BIOS_HANDOFF_ADDR + 16]
    add di, 24
    test ebx, ebx
    jnz .next
.done:
    ret

video_fallback:
    mov ax, 0x0013
    int 0x10
    ret

enable_a20:
    in al, 0x92
    or al, 0x02
    and al, 0xFE
    out 0x92, al
    ret

boot_drive     db 0
disk_error_msg db "v-kernel read error", 0x0D, 0x0A, 0

align 8
gdt_start:
    dq 0x0000000000000000
    dq 0x00CF9A000000FFFF
    dq 0x00CF92000000FFFF
    dq 0x00AF9A000000FFFF
    dq 0x00AF92000000FFFF
gdt_end:

gdt_descriptor:
    dw gdt_end - gdt_start - 1
    dd gdt_start

bits 32
protected_mode:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov esp, STACK32_TOP

    mov esi, KERNEL_BUFFER
    mov edi, KERNEL_LOAD_ADDR
    mov ecx, KERNEL_SECTORS * 128
    rep movsd

    ; Firmware calls need real mode, but writing their results into KERNEL_BUFFER
    ; corrupts the flat kernel payload. Preserve the scratch records only after
    ; the payload has reached its final physical address.
    mov esi, BOOT_INFO_SCRATCH
    mov edi, BOOT_INFO_FINAL
    mov ecx, BOOT_INFO_COPY_SIZE / 4
    rep movsd

    mov edi, KERNEL_BSS_START
    mov ecx, (KERNEL_BSS_END - KERNEL_BSS_START + 3) / 4
    xor eax, eax
    rep stosd

    xor eax, eax
    mov edi, PML4_ADDR
    mov ecx, 6144
    rep stosd

    mov dword [PML4_ADDR], PDPT_ADDR | 0x03
    mov dword [PML4_ADDR + 4], 0
    mov dword [PML4_ADDR + 256 * 8], PDPT_ADDR | 0x03
    mov dword [PML4_ADDR + 256 * 8 + 4], 0
    mov dword [PML4_ADDR + 511 * 8], PDPT_ADDR | 0x03
    mov dword [PML4_ADDR + 511 * 8 + 4], 0

    mov dword [PDPT_ADDR + 0], PD0_ADDR | 0x03
    mov dword [PDPT_ADDR + 4], 0
    mov dword [PDPT_ADDR + 8], PD1_ADDR | 0x03
    mov dword [PDPT_ADDR + 12], 0
    mov dword [PDPT_ADDR + 16], PD2_ADDR | 0x03
    mov dword [PDPT_ADDR + 20], 0
    mov dword [PDPT_ADDR + 24], PD3_ADDR | 0x03
    mov dword [PDPT_ADDR + 28], 0
    mov dword [PDPT_ADDR + 510 * 8], KERNEL_PD_ADDR | 0x03
    mov dword [PDPT_ADDR + 510 * 8 + 4], 0

    mov edi, KERNEL_PD_ADDR
    mov ecx, 1024
    rep stosd

    xor ecx, ecx
.map0:
    mov eax, ecx
    shl eax, 21
    or eax, 0x83
    mov dword [PD0_ADDR + ecx * 8], eax
    mov dword [PD0_ADDR + ecx * 8 + 4], 0
    inc ecx
    cmp ecx, 512
    jne .map0

    xor ecx, ecx
.map1:
    mov eax, ecx
    add eax, 512
    shl eax, 21
    or eax, 0x83
    mov dword [PD1_ADDR + ecx * 8], eax
    mov dword [PD1_ADDR + ecx * 8 + 4], 0
    inc ecx
    cmp ecx, 512
    jne .map1

    xor ecx, ecx
.map2:
    mov eax, ecx
    add eax, 1024
    shl eax, 21
    or eax, 0x83
    mov dword [PD2_ADDR + ecx * 8], eax
    mov dword [PD2_ADDR + ecx * 8 + 4], 0
    inc ecx
    cmp ecx, 512
    jne .map2

    xor ecx, ecx
.map3:
    mov eax, ecx
    add eax, 1536
    shl eax, 21
    or eax, 0x83
    mov dword [PD3_ADDR + ecx * 8], eax
    mov dword [PD3_ADDR + ecx * 8 + 4], 0
    inc ecx
    cmp ecx, 512
    jne .map3

    xor ecx, ecx
.map_kernel:
    mov eax, ecx
    shl eax, 21
    or eax, 0x83
    mov dword [KERNEL_PD_ADDR + ecx * 8], eax
    mov dword [KERNEL_PD_ADDR + ecx * 8 + 4], 0
    inc ecx
    cmp ecx, 512
    jne .map_kernel

    mov eax, PML4_ADDR
    mov cr3, eax

    mov eax, cr4
    or eax, (1 << 5) | (1 << 4)
    mov cr4, eax

    mov ecx, IA32_EFER
    rdmsr
    or eax, 1 << 8
    wrmsr

    mov eax, cr0
    or eax, 0x80000000
    mov cr0, eax

    jmp 0x18:long_mode

bits 64
long_mode:
    mov ax, 0x20
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov rax, KERNEL_STACK_TOP
    mov rsp, rax
    rdtsc
    shl rdx, 32
    or rax, rdx
    mov rbx, 0xa5a55a5ac3c33c3c
    xor rax, rbx
    mov rbx, KERNEL_STACK_GUARD
    mov [rbx], rax
    mov edi, PROTOCOL_BIOS
    mov esi, BIOS_HANDOFF_FINAL
    mov rax, KERNEL_ENTRY
    jmp rax
