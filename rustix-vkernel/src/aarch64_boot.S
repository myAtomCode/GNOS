.section .text.boot, "ax"
.global _start
.extern _start_rust
.extern _secondary_start_rust
.extern bss_start
.extern bss_end
.extern stack_top
.extern vkernel_aarch64_unexpected_dispatch

.equ RUSTIX_BOOT_DTB, 0x52584454

_start:
    msr daifset, #0xf
    mov x19, x0
    mrs x1, mpidr_el1
    and x1, x1, #0xff
    cbnz x1, vkernel_aarch64_secondary_wait

    mrs x1, CurrentEL
    cmp x1, #0x8
    b.eq 1f
    cmp x1, #0x4
    b.eq 2f
    b vkernel_aarch64_park

1:
    mov x1, #(1 << 31)
    msr hcr_el2, x1
    mov x1, #3
    msr cnthctl_el2, x1
    msr cntvoff_el2, xzr
    msr cptr_el2, xzr
    ldr x1, =stack_top
    msr sp_el1, x1
    mov x1, #0x3c5
    msr spsr_el2, x1
    adr x1, 2f
    msr elr_el2, x1
    eret

2:
    mrs x1, cpacr_el1
    orr x1, x1, #(3 << 20)
    msr cpacr_el1, x1
    isb
    ldr x0, =stack_top
    mov sp, x0

    ldr x0, =bss_start
    ldr x1, =bss_end
    cmp x0, x1
    b.ge 4f

3:
    str xzr, [x0], #8
    cmp x0, x1
    b.lt 3b

4:
    bl vkernel_aarch64_enable_mmu
    ldr x0, =RUSTIX_BOOT_DTB
    mov x1, x19
    bl _start_rust
    b vkernel_aarch64_park

.global vkernel_aarch64_secondary_entry
.type vkernel_aarch64_secondary_entry, %function
vkernel_aarch64_secondary_entry:
    msr daifset, #0xf
    mrs x20, mpidr_el1
    and x20, x20, #0xff
    b vkernel_aarch64_secondary_prepare
.size vkernel_aarch64_secondary_entry, .-vkernel_aarch64_secondary_entry

vkernel_aarch64_secondary_wait:
    mov x20, x1
    ldr x2, =vkernel_aarch64_secondary_release
5:
    ldar w3, [x2]
    cbnz w3, vkernel_aarch64_secondary_prepare
    wfe
    b 5b

vkernel_aarch64_secondary_prepare:
    cmp x20, #4
    b.hs vkernel_aarch64_park
    ldr x1, =vkernel_aarch64_secondary_stacks
    mov x2, #0x10000
    madd x3, x20, x2, x1
    mrs x1, CurrentEL
    cmp x1, #0x8
    b.eq 6f
    cmp x1, #0x4
    b.eq 7f
    b vkernel_aarch64_park

6:
    mov x1, #(1 << 31)
    msr hcr_el2, x1
    mov x1, #3
    msr cnthctl_el2, x1
    msr cntvoff_el2, xzr
    msr cptr_el2, xzr
    msr sp_el1, x3
    mov x1, #0x3c5
    msr spsr_el2, x1
    adr x1, 7f
    msr elr_el2, x1
    eret

7:
    mov sp, x3
    mrs x1, cpacr_el1
    orr x1, x1, #(3 << 20)
    msr cpacr_el1, x1
    isb
    bl vkernel_aarch64_activate_mmu
    mov x0, x20
    bl _secondary_start_rust

vkernel_aarch64_park:
    msr daifset, #0xf
8:
    wfi
    b 8b

.type vkernel_aarch64_enable_mmu, %function
vkernel_aarch64_enable_mmu:
    ldr x0, =vkernel_aarch64_l0_table
    ldr x1, =vkernel_aarch64_l1_table
    orr x2, x1, #3
    str x2, [x0]

    adr x3, _start
    mov x4, #0x40000000
    cmp x3, x4
    b.hs 10f

    mov x4, xzr
    mov x5, #0x705
    mov x6, #16
    mov x8, #0x40000000
9:
    orr x7, x4, x5
    str x7, [x1], #8
    add x4, x4, x8
    subs x6, x6, #1
    b.ne 9b
    ldr x1, =vkernel_aarch64_l1_table
    add x1, x1, #(64 * 8)
    ldr x4, =0x1000000000
    ldr x5, =0x60000000000601
    mov x6, #64
9:
    orr x7, x4, x5
    str x7, [x1], #8
    add x4, x4, x8
    subs x6, x6, #1
    b.ne 9b
    b 11f

10:
    ldr x4, =0x60000000000601
    str x4, [x1]
    mov x4, #0x40000000
    mov x5, #0x705
    orr x4, x4, x5
    str x4, [x1, #8]

11:
    adr x3, _start
    lsr x4, x3, #30
    lsl x5, x4, #30
    ldr x0, =vkernel_aarch64_l1_table
    ldr x1, =vkernel_aarch64_kernel_l2_table
    orr x2, x1, #3
    str x2, [x0, x4, lsl #3]
    mov x6, #512
    mov x7, #0x705
    mov x8, #0x200000
    mov x3, x5
12:
    orr x2, x3, x7
    str x2, [x1], #8
    add x3, x3, x8
    subs x6, x6, #1
    b.ne 12b

    ldr x0, =vkernel_aarch64_kernel_l2_table
    ldr x1, =vkernel_aarch64_kernel_l3_tables
    mov x6, #4
13:
    orr x2, x1, #3
    str x2, [x0], #8
    add x1, x1, #1, lsl #12
    subs x6, x6, #1
    b.ne 13b

    ldr x1, =vkernel_aarch64_kernel_l3_tables
    mov x4, x5
    mov x6, #2048
    ldr x9, =text_start
    ldr x10, =text_end
    ldr x11, =rodata_start
    ldr x12, =rodata_end
    ldr x13, =stack_guard_start
    ldr x14, =stack_guard_end
14:
    cmp x4, #0
    b.eq 17f
    cmp x4, x13
    b.lo 15f
    cmp x4, x14
    b.lo 17f
15:
    ldr x7, =0x60000000000707
    cmp x4, x9
    b.lo 16f
    cmp x4, x10
    b.hs 16f
    ldr x7, =0x40000000000787
    b 18f
16:
    cmp x4, x11
    b.lo 18f
    cmp x4, x12
    b.hs 18f
    ldr x7, =0x60000000000787
    b 18f
17:
    mov x7, xzr
    b 19f
18:
    orr x7, x4, x7
19:
    str x7, [x1], #8
    add x4, x4, #1, lsl #12
    subs x6, x6, #1
    b.ne 14b

    dsb sy
.type vkernel_aarch64_activate_mmu, %function
vkernel_aarch64_activate_mmu:
    ldr x0, =vkernel_aarch64_l0_table
    msr ttbr0_el1, x0
    ldr x1, =0xff04
    msr mair_el1, x1
    ldr x1, =0x200803510
    msr tcr_el1, x1
    isb
    tlbi vmalle1
    dsb sy
    isb
    mrs x1, sctlr_el1
    ldr x2, =0x8101d
    orr x1, x1, x2
    msr sctlr_el1, x1
    isb
    ret
.size vkernel_aarch64_activate_mmu, .-vkernel_aarch64_activate_mmu
.size vkernel_aarch64_enable_mmu, .-vkernel_aarch64_enable_mmu

.section .data.boot, "aw"
.align 3
.global vkernel_aarch64_secondary_release
vkernel_aarch64_secondary_release:
    .word 0

.section .bss.boot, "aw", %nobits
.align 12
vkernel_aarch64_l0_table:
    .skip 4096
vkernel_aarch64_l1_table:
    .skip 4096
vkernel_aarch64_kernel_l2_table:
    .skip 4096
.global vkernel_aarch64_kernel_l3_tables
vkernel_aarch64_kernel_l3_tables:
    .skip 16384
.align 16
vkernel_aarch64_secondary_stacks:
    .skip 0x30000

.section .text.exceptions, "ax"
.align 11
.global vkernel_aarch64_vectors
vkernel_aarch64_vectors:
    b vkernel_aarch64_unexpected
    .balign 128
    b vkernel_aarch64_irq_entry
    .balign 128
    b vkernel_aarch64_unexpected
    .balign 128
    b vkernel_aarch64_unexpected
    .balign 128

    b vkernel_aarch64_unexpected
    .balign 128
    b vkernel_aarch64_irq_entry
    .balign 128
    b vkernel_aarch64_unexpected
    .balign 128
    b vkernel_aarch64_unexpected
    .balign 128

    b vkernel_aarch64_svc_entry
    .balign 128
    b vkernel_aarch64_irq_entry
    .balign 128
    b vkernel_aarch64_unexpected
    .balign 128
    b vkernel_aarch64_unexpected
    .balign 128

    b vkernel_aarch64_unexpected
    .balign 128
    b vkernel_aarch64_unexpected
    .balign 128
    b vkernel_aarch64_unexpected
    .balign 128
    b vkernel_aarch64_unexpected
    .balign 128

.global vkernel_aarch64_svc_entry
.type vkernel_aarch64_svc_entry, %function
vkernel_aarch64_svc_entry:
    sub sp, sp, #800
    stp x0, x1, [sp, #0]
    stp x2, x3, [sp, #16]
    stp x4, x5, [sp, #32]
    stp x6, x7, [sp, #48]
    stp x8, x9, [sp, #64]
    stp x10, x11, [sp, #80]
    stp x12, x13, [sp, #96]
    stp x14, x15, [sp, #112]
    stp x16, x17, [sp, #128]
    stp x18, x19, [sp, #144]
    stp x20, x21, [sp, #160]
    stp x22, x23, [sp, #176]
    stp x24, x25, [sp, #192]
    stp x26, x27, [sp, #208]
    stp x28, x29, [sp, #224]
    str x30, [sp, #240]
    mrs x9, sp_el0
    str x9, [sp, #248]
    mrs x9, elr_el1
    str x9, [sp, #256]
    mrs x9, spsr_el1
    str x9, [sp, #264]
    stp q0, q1, [sp, #272]
    stp q2, q3, [sp, #304]
    stp q4, q5, [sp, #336]
    stp q6, q7, [sp, #368]
    stp q8, q9, [sp, #400]
    stp q10, q11, [sp, #432]
    stp q12, q13, [sp, #464]
    stp q14, q15, [sp, #496]
    stp q16, q17, [sp, #528]
    stp q18, q19, [sp, #560]
    stp q20, q21, [sp, #592]
    stp q22, q23, [sp, #624]
    stp q24, q25, [sp, #656]
    stp q26, q27, [sp, #688]
    stp q28, q29, [sp, #720]
    stp q30, q31, [sp, #752]
    mrs x9, fpcr
    mrs x10, fpsr
    str x9, [sp, #784]
    str x10, [sp, #792]
    mov x0, sp
    bl vkernel_aarch64_svc_dispatch
    ldr x9, [sp, #784]
    ldr x10, [sp, #792]
    msr fpcr, x9
    msr fpsr, x10
    ldp q30, q31, [sp, #752]
    ldp q28, q29, [sp, #720]
    ldp q26, q27, [sp, #688]
    ldp q24, q25, [sp, #656]
    ldp q22, q23, [sp, #624]
    ldp q20, q21, [sp, #592]
    ldp q18, q19, [sp, #560]
    ldp q16, q17, [sp, #528]
    ldp q14, q15, [sp, #496]
    ldp q12, q13, [sp, #464]
    ldp q10, q11, [sp, #432]
    ldp q8, q9, [sp, #400]
    ldp q6, q7, [sp, #368]
    ldp q4, q5, [sp, #336]
    ldp q2, q3, [sp, #304]
    ldp q0, q1, [sp, #272]
    ldr x9, [sp, #248]
    msr sp_el0, x9
    ldr x9, [sp, #256]
    msr elr_el1, x9
    ldr x9, [sp, #264]
    msr spsr_el1, x9
    ldr x30, [sp, #240]
    ldp x28, x29, [sp, #224]
    ldp x26, x27, [sp, #208]
    ldp x24, x25, [sp, #192]
    ldp x22, x23, [sp, #176]
    ldp x20, x21, [sp, #160]
    ldp x18, x19, [sp, #144]
    ldp x16, x17, [sp, #128]
    ldp x14, x15, [sp, #112]
    ldp x12, x13, [sp, #96]
    ldp x10, x11, [sp, #80]
    ldp x8, x9, [sp, #64]
    ldp x6, x7, [sp, #48]
    ldp x4, x5, [sp, #32]
    ldp x2, x3, [sp, #16]
    ldp x0, x1, [sp, #0]
    add sp, sp, #800
    eret
.size vkernel_aarch64_svc_entry, .-vkernel_aarch64_svc_entry

.global vkernel_aarch64_irq_entry
.type vkernel_aarch64_irq_entry, %function
vkernel_aarch64_irq_entry:
    sub sp, sp, #784
    stp x0, x1, [sp, #0]
    stp x2, x3, [sp, #16]
    stp x4, x5, [sp, #32]
    stp x6, x7, [sp, #48]
    stp x8, x9, [sp, #64]
    stp x10, x11, [sp, #80]
    stp x12, x13, [sp, #96]
    stp x14, x15, [sp, #112]
    stp x16, x17, [sp, #128]
    stp x18, x19, [sp, #144]
    stp x20, x21, [sp, #160]
    stp x22, x23, [sp, #176]
    stp x24, x25, [sp, #192]
    stp x26, x27, [sp, #208]
    stp x28, x29, [sp, #224]
    str x30, [sp, #240]
    stp q0, q1, [sp, #256]
    stp q2, q3, [sp, #288]
    stp q4, q5, [sp, #320]
    stp q6, q7, [sp, #352]
    stp q8, q9, [sp, #384]
    stp q10, q11, [sp, #416]
    stp q12, q13, [sp, #448]
    stp q14, q15, [sp, #480]
    stp q16, q17, [sp, #512]
    stp q18, q19, [sp, #544]
    stp q20, q21, [sp, #576]
    stp q22, q23, [sp, #608]
    stp q24, q25, [sp, #640]
    stp q26, q27, [sp, #672]
    stp q28, q29, [sp, #704]
    stp q30, q31, [sp, #736]
    mrs x9, fpcr
    mrs x10, fpsr
    str x9, [sp, #768]
    str x10, [sp, #776]
    bl vkernel_aarch64_irq_dispatch
    ldr x9, [sp, #768]
    ldr x10, [sp, #776]
    msr fpcr, x9
    msr fpsr, x10
    ldp q30, q31, [sp, #736]
    ldp q28, q29, [sp, #704]
    ldp q26, q27, [sp, #672]
    ldp q24, q25, [sp, #640]
    ldp q22, q23, [sp, #608]
    ldp q20, q21, [sp, #576]
    ldp q18, q19, [sp, #544]
    ldp q16, q17, [sp, #512]
    ldp q14, q15, [sp, #480]
    ldp q12, q13, [sp, #448]
    ldp q10, q11, [sp, #416]
    ldp q8, q9, [sp, #384]
    ldp q6, q7, [sp, #352]
    ldp q4, q5, [sp, #320]
    ldp q2, q3, [sp, #288]
    ldp q0, q1, [sp, #256]
    ldr x30, [sp, #240]
    ldp x28, x29, [sp, #224]
    ldp x26, x27, [sp, #208]
    ldp x24, x25, [sp, #192]
    ldp x22, x23, [sp, #176]
    ldp x20, x21, [sp, #160]
    ldp x18, x19, [sp, #144]
    ldp x16, x17, [sp, #128]
    ldp x14, x15, [sp, #112]
    ldp x12, x13, [sp, #96]
    ldp x10, x11, [sp, #80]
    ldp x8, x9, [sp, #64]
    ldp x6, x7, [sp, #48]
    ldp x4, x5, [sp, #32]
    ldp x2, x3, [sp, #16]
    ldp x0, x1, [sp, #0]
    add sp, sp, #784
    eret
.size vkernel_aarch64_irq_entry, .-vkernel_aarch64_irq_entry

vkernel_aarch64_unexpected:
    msr daifset, #0xf
    b vkernel_aarch64_unexpected_dispatch
