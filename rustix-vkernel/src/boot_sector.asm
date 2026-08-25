; boot_sector.asm - x86 BIOS stage-1 boot sector
;
; This file must stay within 512 bytes including the 0x55AA signature.
; It only does the minimum required work:
; 1. initialize a known real-mode environment
; 2. load the stage-2 loader from disk into low memory
; 3. jump to the loaded second stage

bits 16
org 0x7C00

STAGE2_LOAD_ADDR equ 0x0600
%ifndef STAGE2_SECTORS
%define STAGE2_SECTORS 1
%endif

start:
    cli
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00

    mov [boot_drive], dl

    call init_serial

    mov si, boot_msg
    call print_string

    mov bx, STAGE2_LOAD_ADDR
    mov ah, 0x02
    mov al, STAGE2_SECTORS
    mov ch, 0x00
    mov cl, 0x02
    mov dh, 0x00
    mov dl, [boot_drive]
    int 0x13
    jc disk_error

    mov si, ok_msg
    call print_string

    jmp 0x0000:STAGE2_LOAD_ADDR

disk_error:
    xor ah, ah
    mov dl, [boot_drive]
    int 0x13

    mov si, err_msg
    call print_string

.hang:
    hlt
    jmp .hang

print_string:
    lodsb
    test al, al
    jz .done

    push ax
    mov ah, 0x0E
    mov bx, 0x0007
    int 0x10

    pop ax
    call serial_putc

    jmp print_string

.done:
    ret

init_serial:
    mov dx, 0x3FB
    mov al, 0x80
    out dx, al

    mov dx, 0x3F8
    mov al, 0x01
    out dx, al

    mov dx, 0x3F9
    xor al, al
    out dx, al

    mov dx, 0x3FB
    mov al, 0x03
    out dx, al

    mov dx, 0x3FA
    mov al, 0xC7
    out dx, al

    mov dx, 0x3FC
    mov al, 0x03
    out dx, al
    ret

serial_putc:
    push ax
    push dx
    mov ah, al
.wait:
    mov dx, 0x3FD
    in al, dx
    test al, 0x20
    jz .wait

    mov dx, 0x3F8
    mov al, ah
    out dx, al
    pop dx
    pop ax
    ret

boot_drive db 0
boot_msg   db "Rustix BIOS boot", 0x0D, 0x0A, 0
ok_msg     db "Loading stage 2", 0x0D, 0x0A, 0
err_msg    db "Disk read error", 0x0D, 0x0A, 0

times 510 - ($ - $$) db 0
dw 0xAA55
