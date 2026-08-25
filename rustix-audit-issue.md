# rustix 0.3.0 全面安全审计报告 —— 宣称"Linux ABI/API 兼容"，实际离可用内核都差得远

> **⚠️ 版本勘误（v2，2026-08-24 更新）**：以下正文全部针对 **旧提交 `6821985`（2026/4/4，gitee 默认分支 master）**。
> 作者已推 **新 main 分支（`dc8ec22 "aaaa"`）**：这是一个**推倒重写的巨型提交**（+38.7k/−43.1k 行），
> 删除了旧的 `src/kernel`、`src/arch`、`src/user`、`src/drivers`，替换为 `src/mm`（真分页/真帧分配器）、
> `src/kernel/scheduler.rs`（约 5000 行真调度器）、`src/x86_interrupts.rs`（真 64 位 IDT + syscall 门）、
> `src/services.rs`、`src/sync.rs`（锁）、`userland/build_busybox.sh`（用 musl 1.2.5 编真 busybox 用户态）等。
> **正文的 C/H/M 结论仅适用于旧代码；新 main 的逐项复核结论见下表。**

### 新 main 分支（dc8ec22）逐项复核结论

| 原编号 | 原漏洞 | 新 main 复核 | 状态 |
|---|---|---|---|
| C1 | 内存池分配地址恒为 0 | `src/mm/frame.rs` 真帧分配器（`allocate() -> Result<PhysicalAddress>`） | ✅ 已修复 |
| C2 | verify_area 逻辑反转 | `src/mm/user.rs` `UserRange`/`checked_user_range`/`USER_ADDRESS_LIMIT` | ✅ 已修复 |
| C3/C4 | 页表复制不拷项/释放泄漏 | `src/mm/paging.rs` `copy_from_user`/`copy_to_user`/`copy_user`（设计重构） | ✅ 已修复 |
| C6 | switch_to 空操作 | `src/kernel/scheduler.rs:5235` `switch_to(state,current,next) -> *mut RegisterFrame` 真切换 | ✅ 已修复 |
| C7 | TSS 32 位布局 | `src/x86_interrupts.rs:47` 用 x86_64 crate `TaskStateSegment` + `privilege_stack_table` | ✅ 已修复 |
| C8 | 32 位 IDT | `init_idt(use_ist: bool)` + 真汇编入口/`iretq`/APIC | ✅ 已修复 |
| C9 | 异常不弹错误码/不 iretq | 新 asm 入口（`x86_interrupts.rs:79-105`）规范处理 | ✅ 已修复 |
| C10 | 用户指针零校验 | `mm/user.rs` + `paging.rs copy_user` 机制齐备 | ✅ 已修复（机制） |
| C11 | 无 syscall 门 | `x86_interrupts.rs:119-131` `vkernel_syscall_entry` + `vkernel_fast_syscall_entry` | ⚠️ 门已建，但 `linux/syscall.rs` 的 `dispatch` 仍一律返回 `ENOSYS`（骨架未落地） |
| C12 | 用户程序是 ring0 函数指针 | `user/` 删除，`userland/build_busybox.sh` 用 musl 编真 busybox；main.rs 有 ring3/user/syscall 计数 | ✅ 重构（未实测） |
| H1 | /dev/random 常数种子 LCG | `services.rs:1866-1876` 改为 **xorshift，种子 = `stable_inode(path)+offset`** | ⚠️ **仍成立**：仍是确定性 PRNG，无熵源，输出可预测，非 CSPRNG |
| H2 | 全局缓冲返回 `&'static` | `PROC_BUFFER` 模式已消失，`static mut` 大幅收敛 | ✅ 已修复 |
| H3 | read_user_cstr 任意内存读 | 由 `mm/user.rs` 范围检查体系替代 | ✅ 已修复 |
| H4 | 无锁 `static mut` + `&'static mut` | 新增 `src/sync.rs` 锁体系；但 **`gui.rs:365/1194`、`mm/paging.rs:1246` 仍保留 `&'static mut` 逃逸** | ⚠️ 部分残留 |
| H5 | schedule 无可运行任务死循环 | `scheduler.rs` 真调度器（含 idle/负载均衡计数） | ✅ 已修复（未逐行验证） |
| H6 | 明文 root/root 密码 | `services.rs` 改 `USER_STORAGE` 目录模型 | ✅ 已重构（认证细节未复核） |
| H7/H8 | 内存池 1000 页/双重记账 | `frame.rs` 真分配器 | ✅ 已修复 |
| H9 | 引导硬编码 0x9799/64KB 回卷 | 新 `src/boot/`（device_tree/memory_map），无 0x9799 魔数 | ✅ 已修复 |
| M1 | 三份 init_idt | 收敛为 `x86_interrupts.rs::init_idt` 单份 | ✅ 已修复 |
| M4 | DRIVER_MANAGER 死代码 | `src/drivers` 整体删除 | ✅ 已修复 |
| M5 | `_start_rust` 不存在符号 | 旧 long_mode_init.asm 删除 | ✅ 已修复 |
| M6 | 多份 main_*.rs | 仅存 `src/main.rs` | ✅ 已修复 |
| M8 | 假 passwd/SSH/集群文件 | 换为 extfs/initramfs/services 新模型 | 🔶 未复核（initramfs 内容未逐一验证） |
| M9 | virt_to_phys 恒等映射 | `mm/paging.rs`（1332 行）真分页 | ✅ 已修复 |
| M10 | log.rs 边界小 bug | log.rs 已删除 | ✅ 已修复 |

**复核总结**：12 项 CRITICAL 中 10 项在新 main 已修复、2 项基本重构；9 项 HIGH 中 6 项已修复，
**2 项仍成立/残留（H1 `/dev/random` 仍是确定性 xorshift、H4 的 `&'static mut` 逃逸仍存在于 gui/paging）**，
C11 的 syscall 分发仍是 `ENOSYS` 骨架。新代码规模约 3 万行，本次仅做定向复核，**未做全量新审计**；
"Linux ABI/API 兼容"的宣称在新 main 上依然没有落地证据（syscall dispatch 未实现任何真实调用）。
新代码另注意到：内核镜像里嵌了 5MB `wallpaper.bin`、`gui.rs` 桌面状态、`vkernel` 命名体系等，值得单独开一轮新审计。

---

> 审计对象（原报告正文）：`github.com/openclaw/rustix`（gitee 镜像 `gitee.com/rustix-association/rustix`，commit `6821985`，v0.3.0）
> 审计范围：`src/` 全部内核代码（约 5400 行 Rust + 744 行汇编）、`user/` 用户程序、构建脚本、`docs/` 与 `api/` 宣称文档
> 审计方法：逐文件通读，对照 Linux x86-64 ABI 与 x86_64 长模式硬件规范
> 严重度分级：🔴 CRITICAL（可导致内核内存破坏 / 任意读写 / 完全不可用）→ 🟠 HIGH（信息泄露 / 提权面 / 死锁）→ 🟡 MEDIUM（健壮性 / 宣称不符）

---

## 〇、摘要

该项目在 `Cargo.toml` 中自称 **"A minimal Linux-compatible kernel written in Rust (Multi-Architecture)"**，
在 `docs/FINAL_SUMMARY.md` 中自称 **"Linux ABI: System calls compatible with Linux applications"**，
在 `docs/LINUX_COMPATIBILITY_PLAN.md` 中自称是 **"potentially leading to safer and more reliable operating systems in the future"**（未来用 Rust 写的 Linux），
`FIXES_SUMMARY.md` 则宣称 **"✅ 所有问题已修复 ✅"**。

经审计，以上宣称与代码现实完全不符，且存在 **至少 12 个致命级（CRITICAL）缺陷、9 个高危（HIGH）缺陷**。核心事实：

1. **没有任何用户态**。"用户程序"（hello/game）是内核里以 `fn(&ArgList) -> isize` 直接调用的函数指针，运行在 **ring 0**，无 MMU 隔离、无 ELF 加载器（ext4.rs 里的 `/bin/hello` 文件内容就是一句 "ELF loader not implemented yet" 的文本）。
2. **syscall 编号是 i386 传统号**（`read=3, write=4, open=5`），与 Linux x86-64（`read=0, write=1, open=2, exit=60`）完全不同；handler 只有 3 个参数通道，Linux 的 6 参 ABI（rdi/rsi/rdx/r10/r8/r9）无法表达。musl/glibc 二进制一个都跑不了。
3. **调度器不调度**：`switch_to()` 只改 `CURRENT` 指针，不做任何栈/寄存器切换。
4. **内存管理器分配地址恒为 0**：内存池初始化时压入的是 `PageFrame::new()`（addr=0）而不是页面地址。
5. **x86_64 上加载的是 32 位格式 IDT**：`IdtEntry` 只有 8 字节（无 IST、offset 高 16 位），handler 地址 `as u32` 被截断；异常处理函数不弹错误码、不 iretq、直接 `loop {}`。
6. **没有 syscall 门**：`system_call_handler` 是空函数，`TrapGate` 结构从未使用，无 `IA32_LSTAR` MSR 配置——`syscall` 指令根本无法进入内核。
7. **"网络栈"、"集群栈 ssh+openmpi+nfs"、"XFS 服务卷"、"ext4 根文件系统"全部是静态文本数组**，没有任何真实实现。

---

## 一、宣称 vs 现实（"吹牛"原文摘录）

| 文件 | 宣称原文 | 现实 |
|---|---|---|
| `Cargo.toml:7` | `A minimal Linux-compatible kernel written in Rust (Multi-Architecture)` | 无用户态、无 ELF、i386 syscall 号 |
| `docs/FINAL_SUMMARY.md:52` | `Linux ABI: System calls compatible with Linux applications` | 与 musl/glibc 的 syscall 号、6 参 ABI 全不兼容 |
| `docs/FINAL_SUMMARY.md:78` | `Linux Compatibility: Maintains compatibility with Linux interfaces` | /proc、/dev、ext4、XFS 全是写死的文本节点 |
| `docs/LINUX_COMPATIBILITY_PLAN.md:148` | `...potentially leading to safer and more reliable operating systems in the future` | 自比未来 Linux |
| `docs/PROJECT_OVERVIEW_EN.md:10` | `Linux Compatibility: Support basic Linux system calls and interfaces` | 无 syscall 门，`syscall` 指令进不了内核 |
| `FIXES_SUMMARY.md:4` | `✅ 所有问题已修复 ✅` | 见下文 12 个 CRITICAL |
| `FIXES_SUMMARY.md:141` | `✅ 支持多架构 (x86_64 和 ARM64)` | aarch64/arm 仅有 `mod.rs` 占位与空目录 |
| `FIXES_SUMMARY.md:130` | `✅ 实现基本的进程调度` | `switch_to()` 是空操作 |
| `src/kernel/fs/ext4.rs:159` | `/bin/hello` 内容：`ELF loader not implemented yet.` | 自己承认没有 ELF 加载器 |
| `src/kernel/mod.rs:47` | `// 初始化网络` → `println!("Network stack initialized")` | 只有一行打印，无任何网络代码 |
| `src/kernel/service.rs` + `ext4.rs` | 集群栈 `ssh + openmpi + nfs skeleton ready`、fake `authorized_keys` | 全部是静态字符串与打印函数 |

> 结论：项目把 **教学演示用的静态文件系统 + 内核内 shell** 包装成了 "Linux 兼容、未来 Linux"，
> 且 `FIXES_SUMMARY.md` 宣称"所有问题已修复"与代码状态严重不符。以下漏洞逐条给出证据。

---

## 二、🔴 CRITICAL —— 致命漏洞

### C1. 内存池初始化 bug：`alloc_page()` 分配到的地址恒为 0（内存管理器彻底失效）

- **位置**：`src/kernel/mm.rs:258-271`、`mm.rs:152-158`、`mm.rs:274-283`
- **代码**：
  ```rust
  // mm.rs:152-158 —— PageFrame::new() 的 addr 是 0
  pub fn new() -> Self { Self { addr: 0, flags: 0, count: 0 } }

  // mm.rs:263-268 —— 算出了 page_addr，却压入 new()！
  for i in 0..core::cmp::min(1000, total_pages) {
      let page_addr = self.memory_start + (i as u32) * PAGE_SIZE as u32;
      if page_addr >= 0x100000 {
          self.free_pages.push(PageFrame::new());   // ← 丢掉了 page_addr，压入 addr=0
      }
  }
  ```
- **影响**：`alloc_page()` / `get_free_page()` 返回的每一页地址都是 **0**。任何用分配结果写内存的地方都会写进物理地址 0 附近（实模式中断向量表 / BIOS 数据区），静默破坏引导数据；`copy_page_tables` 等依赖分配结果的函数全部拿到 0 地址。内存管理在第一次分配时就坏。
- **修复**：`self.free_pages.push(PageFrame::from_addr(page_addr));`

### C2. `verify_area()` 校验逻辑反转：内核地址通过"用户内存校验"

- **位置**：`src/kernel/mm.rs:435-455`
- **代码**：
  ```rust
  let virt_addr = addr as u32;
  if virt_addr < 0x00000000 || virt_addr >= 0xC0000000 {
      // 注释自称"0xC0000000 以下是用户空间"
      return Ok(());          // ← 内核空间地址（>=0xC0000000）直接放行！
  }
  ```
- **影响**：本意是校验用户指针，实际**内核态指针（>=3GB）全部通过**，只对用户区做范围检查。加上 `addr as u32` 把 64 位指针截断，任意内核内存地址都能通过该校验。且该函数在 syscall 路径中根本没有被调用（见 C10）。
- **修复**：`if virt_addr >= 0xC0000000 { return Err(...); }`，并改用 64 位地址比较。

### C3. `copy_page_tables()` 从不复制页表项：目标地址空间全是空页表

- **位置**：`src/kernel/mm.rs:308-344`
- **代码**：
  ```rust
  let from_entry = self.page_directory.get_entry(i);
  if from_entry.is_present() {
      let new_page_table = self.alloc_page().ok_or(...)?;   // 新分配一页
      self.page_directory.set_entry(
          to_page_dir_idx + (i - from_page_dir_idx),        // 目标索引未做 <1024 检查
          PageEntry::new(new_page_table_addr, PAGE_PRESENT | PAGE_RW | PAGE_USER)
      );   // ← 只填了 PDE，源页表的 PTE 一个都没拷！
  }
  ```
- **影响**：声称是 "Linux 0.11 copy_page_tables 的 Rust 版本"，实际复制后目标 PDE 指向**全新的空白页表**，目标地址空间任何访问都触发缺页。`from + size - 1` 有 u32 溢出面；目标索引 `to_page_dir_idx + (i - from_page_dir_idx)` 可能超过 1023 却没有检查。
- **修复**：遍历源页表逐个复制 PTE；对目标索引做边界检查；新页表需清零。

### C4. `free_page_tables()` 只清 PDE 不释放页表页，且结束索引差一

- **位置**：`src/kernel/mm.rs:347-367`
- **代码**：
  ```rust
  let end_page_dir_idx = ((from + size) >> 22) as usize;   // 应再 -1
  for i in start_page_dir_idx..=end_page_dir_idx {
      let entry = self.page_directory.get_entry_mut(i);
      if entry.is_present() { entry.clear(); }              // 页表页本身从未 free
  }
  ```
- **影响**：每次释放都泄漏整张页表页；`end` 索引多算一项。与 C1（分配地址恒 0）叠加，内存管理完全不可用。
- **修复**：把页表页地址从 PDE 读出后归还内存池；修正边界计算。

### C5. 全局 `free_page()` 是空操作：内存只分配永不释放

- **位置**：`src/kernel/mm.rs:428-432`
- **代码**：
  ```rust
  pub fn free_page(_addr: u32) -> Result<(), &'static str> {
      // 在实际实现中，我们会将页面放回空闲列表 —— 这里简化处理
      Ok(())
  }
  ```
- **影响**：所有释放路径都是 no-op，配合 `SimpleVec` 固定 1000 容量（`push` 满则静默丢弃），内存池耗尽后 `alloc_page()` 返回 `None`，调用方 `unwrap`/`expect` 即内核 panic。`MemoryManager::free_page`（mm.rs:286-299）不检查页是否已分配，任意地址都可"释放"回池，制造重复分配（double-alloc）。
- **修复**：实现真正的归还 + 分配记账校验。

### C6. `switch_to()` 是空操作：调度器"调度"但不切换上下文

- **位置**：`src/kernel/sched.rs:380-399`
- **代码**：
  ```rust
  pub fn switch_to(nr: usize) {
      unsafe {
          if nr >= NR_TASKS || TASK[nr].is_null() { return; }
          if TASK[nr] == CURRENT { return; }
          CURRENT = TASK[nr];
          // ...clts 相关...
          // 当前主版本先只做逻辑切换，避免在 x86_64 上依赖旧式硬件任务门/TSS 切换。
      }
  }
  ```
- **影响**：只改了 `CURRENT` 指针，**RSP/寄存器/页表一个不换**。后果链：
  - `schedule()` 返回后代码继续跑在**原任务的栈**上，但 `CURRENT` 已指向别的任务 → `sleep_on`/`interruptible_sleep_on`/`wake_up` 的等待队列逻辑全部错乱；
  - `do_timer()`（时钟中断里，sched.rs:633-635）在 `counter <= 0` 时调用 `schedule()`，中断返回后继续用原 RSP 执行"新任务"的上下文 → 栈被两个任务复用，互相踩踏；
  - 所谓的"多进程"根本不存在，`sys_fork`/`sys_execve` 只是摆设。
- **修复**：实现真正的上下文切换（保存/恢复 callee-saved 寄存器 + RSP + CR3），或诚实删除调度宣称。

---

### C7. `TssStruct` 用 32 位 Linux 0.11 布局：在 x86_64 上结构完全错误

- **位置**：`src/kernel/sched.rs:140-171`、`sched.rs:475-485`
- **代码**：
  ```rust
  pub struct TssStruct {
      pub back_link: u32, pub esp0: u32, pub ss0: u32, ...
      pub cr3: u32, pub eip: u32, pub eflags: u32,
      pub eax: u32, ..., pub esp: u32, ...   // ← 全是 32 位寄存器
      pub i387: [u32; 26],
  }
  // set_tss_desc: desc.type_limit_flags = 0x89; // 任务状态段，386
  ```
- **影响**：x86_64 的 TSS 布局是 `rsp0/rsp1/rsp2/ist1..ist7` 等 64 位字段（共 104 字节），而这里写的是 386 时代布局（`esp/eip/eflags` 32 位）。若真把该 TSS 加载进 GDT（`ltr`），任何特权级切换都会触发 `#TS`（invalid TSS）。同时 `TaskStruct` 里 `filp: [*mut File; 20]`、LDT 等全部是 Linux 0.11 古董结构，与现代内核无关。
- **修复**：使用 x86_64 规范的 TSS 布局（x86_64 crate 的 `TaskStateSegment`）。

### C8. 实际加载的 IDT 是 32 位格式：长模式下 IDT 条目地址被截断，中断即崩溃

- **位置**：`src/kernel/interrupts.rs:47-108`、`interrupts.rs:116-120`、`interrupts.rs:226-232`
- **代码**：
  ```rust
  // 8 字节的 32 位 IDT 条目（x86_64 应为 16 字节，offset 64 位 + IST 字段）
  pub struct IdtEntry { offset_low: u16, selector: u16, zero: u8, flags: u8, offset_high: u16 }
  // IDTR 的 base 是 u32（x86_64 是 64 位）
  pub struct IdtDescriptor { size: u16, offset: u32 }
  // 64 位函数指针被截断成 u32
  IdtEntry::new(divide_error_handler as u32, 0x08, 0x8E)
  ```
- **影响**：`main.rs:139` 调用 `kernel::interrupts::init_interrupts()`，加载的是这一套 32 位 IDT：handler 地址被 `as u32` 截断、IDTR base 只有 32 位。一旦内核镜像/代码地址高于 4GB（`code-model=kernel` 下很常见），任何中断/异常都会跳转到截断后的错误地址 → 立即 #GP → 双故障 → 三故障复位。**当前 IDT 在长模式下不可用**。
- **修复**：换用 16 字节 64 位 IDT 条目（`arch/x86_64/idt.rs` 里那套 64 位格式是正确的，但它从未被调用——见 M1），并让 `init_interrupts` 真正接入它。

### C9. 异常处理函数不弹错误码、不 iretq、死循环：任何异常都变成挂死或崩溃

- **位置**：`src/kernel/interrupts.rs:238-349`（全部 handler）、`arch/x86_64/idt.rs:183-306`（另一套）
- **代码**：
  ```rust
  // kernel/interrupts.rs —— 全部 handler 都是这个形状：
  extern "C" fn general_protection_handler() {
      crate::println!("General Protection Fault (#GP)!");
      loop {}          // ← 死循环；#GP/#PF/#DF 等 CPU 压了错误码，handler 没参数、不弹栈
  }
  // arch/x86_64/idt.rs —— 另一套 handler"继续执行"，但同样没有 iretq：
  extern "C" fn page_fault_handler() { ... 发送 EOI ...; // 继续执行 }
  ```
- **影响**：双重问题：
  1. **错误码不弹**：#DF(8)/#TS(10)/#NP(11)/#SS(12)/#GP(13)/#PF(14)/#AC(17) 会往栈上压错误码，handler 用 `extern "C" fn()`（无参数）进入，栈布局错位；
  2. **没有 iretq**：中断门进入后必须 `iretq` 返回，而这些函数以普通 `ret` 结尾——`ret` 会把 CPU 压入的返回地址当函数返回地址弹出，栈完全错乱。
  kernel/interrupts.rs 那套直接 `loop {}`（异常即永久挂死，键盘/timer 中断除外）；arch/x86_64/idt.rs 那套"继续执行"实际是灾难（回到故障指令 → 无限异常循环）。
- **修复**：handler 使用 `extern "x86-interrupt" fn`（自动处理错误码与 iretq），异常按级别 kill 进程或 panic 打印寄存器，绝不"继续执行"。

### C10. `sys_read` 对用户指针零校验：任意内核内存可读可写（提权面）

- **位置**：`src/kernel/syscall.rs:216-226`
- **代码**：
  ```rust
  pub fn sys_read(_fd: usize, _buf: usize, _count: usize, _arg4: usize) -> isize {
      if _buf == 0 { return ...; }
      let buffer = unsafe { core::slice::from_raw_parts_mut(_buf as *mut u8, _count) };
      match fs::read(_fd as u32, buffer, _count) { ... }
  }
  ```
- **影响**：`_buf`/`_count` 直接来自调用者，**没有任何用户地址范围校验**（`mm::verify_area` 存在但从未被 syscall 层调用，且本身逻辑反转见 C2）。把 `_buf` 指向任意内核地址、`_count` 取大值，就能让内核往任意地址写任意长度数据——标准的内核任意内存写原语。`_count` 过大还会让 `from_raw_parts_mut` 越界 → 内核页错误。`sys_write` 同病。
- **修复**：所有用户指针先过 `verify_area`（修正后的）再做拷贝/建 slice；对 `_count` 做上限限制。

### C11. 没有 syscall 门：`syscall` 指令进不了内核，"Linux ABI" 物理上不可达

- **位置**：`src/kernel/interrupts.rs:519-545`、`src/arch/x86_64/mod.rs:365-380`
- **代码**：
  ```rust
  // interrupts.rs:520 —— 空实现
  pub fn system_call_handler() { /* 这会在system_call.s中被调用 */ }
  // TrapGate 结构定义了但从未被 set/加载
  // arch/x86_64/mod.rs:366 —— 唯一的 syscall 指令用法，是"内核内部"helper
  pub fn syscall(num, arg1, arg2, arg3) -> u64 { asm!("syscall", ...) }
  ```
- **影响**：IDT 里没有注册 0x80 中断门；`IA32_LSTAR`/`IA32_STAR`/`IA32_SFMASK` 没有任何 `wrmsr` 配置。任何程序执行 `syscall` 指令都会 #UD 或跳转到垃圾地址。"Linux 兼容 syscall 接口"从入口就断掉了——实际运行路径是"用户程序"直接调用内核的 `system_call()` 函数（见 C12），这根本不是系统调用。
- **修复**：要么配置 LSTAR + syscall 入口（isr.asm 式），要么注册 int 0x80 中断门；两者都是必需的最小 ABI 基建。

### C12. "用户程序"是 ring 0 函数指针：无用户态、无隔离、无 ELF

- **位置**：`src/user/mod.rs:48-59`、`src/kernel/apps.rs:14-36`
- **代码**：
  ```rust
  pub fn exec(path: &str, argv: &ArgList) -> Result<isize, ExecError> {
      for program in &PROGRAMS {
          if program.path == path {
              return Ok((program.entry)(argv));   // ← 直接在内核上下文调用！
          }
      }
      ...
  }
  ```
- **影响**：`/bin/hello`、`/bin/game` 就是两个 `fn(&ArgList) -> isize`，由内核在**ring 0** 直接调用。`execve` 是"查表 + 调用函数指针"，没有 CR3 切换、没有特权级切换、没有栈隔离、没有 ELF 解析（ext4.rs:159 自认 "ELF loader not implemented yet"）。因此：
  - 任何"用户程序"的 bug 都直接是内核崩溃；
  - `user/runtime.rs` 里的 `write/read/open` 只是薄封装调 `system_call()`，不是真 syscall；
  - 这与 `docs` 宣称的 "Linux-compatible system call interface"、"run existing software"（IMPLEMENTATION_GUIDE.md:646）完全矛盾。
- **修复**：实现 ELF 加载 + 页表隔离 + ring3 切换 + 真实 syscall 门（C11）之前，不应宣称"Linux 应用兼容"。

---

## 三、🟠 HIGH —— 高危漏洞

### H1. `/dev/random` 是常数种子 LCG：可预测的"随机数"（伪随机）

- **位置**：`src/kernel/fs/dev.rs:47-67`
- **代码**：
  ```rust
  static mut RNG_STATE: u32 = 0x1357_9bdf;      // 固定种子
  "random" => {
      for index in 0..SAMPLE_SIZE {
          RNG_STATE = RNG_STATE.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);  // 线性同余
          DEV_BUFFER.buf[index] = (RNG_STATE >> 24) as u8;
      }
  }
  ```
- **影响**：LCG 32 位状态可被 2-3 个输出字节完全还原，种子还是常量。任何基于 `/dev/random` 的密钥/会话生成都**可预测**。Linux 的 `/dev/random` 是内核 CSPRNG（chaCha20）。这是对"Linux 兼容"宣称的又一打击。
- **修复**：接入真实熵源（RDRAND/RDSEED + 混合池），或明确降级为 `getrandom` 之外的占位并标注非安全。

### H2. `PROC_BUFFER`/`DEV_BUFFER` 静态可变缓冲区被当作 `&'static [u8]` 返回：别名 + 数据竞争 + 陈旧数据

- **位置**：`src/kernel/fs/proc.rs:74, 204-226`、`src/kernel/fs/dev.rs:47, 50-82`
- **代码**：
  ```rust
  static mut PROC_BUFFER: ProcBuffer = ProcBuffer::new();   // 全局唯一 4KB 缓冲
  pub fn read(relative_path: &str) -> Result<&'static [u8], FsError> {
      unsafe { PROC_BUFFER.clear(); ...; Ok(PROC_BUFFER.as_slice()) }   // 返回指向全局的 &'static
  }
  ```
- **影响**：所有 `/proc/*`、`/dev/*` 读取共享**同一个全局缓冲区**，返回的 `&'static` 切片与下一次写 `static mut` 形成别名——下一次读取会篡改上一次调用者仍持有的数据（TOCTOU）；两个任务并发读 `/proc` 就是数据竞争（UB）。`RNG_STATE` 也是 `static mut` 无锁更新。
- **修复**：把数据拷入调用者提供的缓冲区（sys_read 已有目标 buffer），禁止返回全局引用。

### H3. `read_user_cstr` / `read_user_argv` 无界读任意内核内存：信息泄露

- **位置**：`src/kernel/fs.rs:746-769`、`src/kernel/apps.rs:81-95`
- **代码**：
  ```rust
  // fs.rs:753 —— 从任意地址逐字节读，直到 NUL 或 128 字节
  let src = ptr as *const u8;
  while len < scratch.len() - 1 {
      let byte = src.add(len).read_volatile();
      ...
  }
  // apps.rs:84 —— 从任意地址读指针数组，无上限
  let argv = argv_ptr as *const usize;
  for index in 0..ArgList::MAX_ARGS {
      let arg_ptr = unsafe { argv.add(index).read_volatile() };
  ```
- **影响**：`execve` 的 path/argv 指针完全不受约束。把指针指向内核地址（如 `g_nosys` 表、任意静态区），内核会**把内核内存当字符串/指针数组读出**——信息泄露原语，且可造成内核崩溃（读到非法地址时 panic）。
- **修复**：所有用户指针先经 `verify_area`（修正后）校验，再 `copy_from_user`。

### H4. 全局 `static mut` + `&'static mut` 逃生舱：无锁数据竞争（Rust UB）

- **位置**：`src/kernel/mm.rs:381-395`（`get_memory_manager_mut() -> &'static mut`）、`src/kernel/sched.rs:198-208`（`static mut TASK/CURRENT/JIFFIES`）、`src/kernel/fs.rs:252`（`static mut VFS`）、`src/kernel/terminal.rs:77`（`static mut TERMINAL`）、`src/drivers/mod.rs:224-248`
- **影响**：整个内核没有一把锁。`get_memory_manager_mut()` 返回 `&'static mut`，调用方可同时持有多个可变引用（Rust 别名规则 UB）；时钟中断（`do_timer` 改 `CURRENT->counter`/`JIFFIES`）与主循环（`terminal::poll_input` 改 `TERMINAL`）并发访问同一 `static mut`。x86_64 单核下暂时"碰巧能跑"，任何多核/抢占尝试都会数据竞争；按 Rust 内存模型这本身就是未定义行为。
- **修复**：用 `Mutex`/关中断临界区包裹全局状态，去掉 `&'static mut` 逃生舱。

### H5. `schedule()` 在所有任务不可运行时死循环（内核挂死）

- **位置**：`src/kernel/sched.rs:346-372`
- **代码**：
  ```rust
  loop {
      c = -1; next = 0;
      for i in (1..NR_TASKS).rev() {
          if TASK[i].is_null() { continue; }
          if task.state == Runnable && task.counter > c { c = task.counter; next = i; }
      }
      if c > 0 { break; }        // 没有任何 Runnable 任务时：永远不 break
      for i in ... { task.counter = (task.counter >> 1) + task.priority; }
  }
  ```
- **影响**：所有任务处于 `Zombie`/`Uninterruptible`/`Stopped` 时，内层循环永远找不到 `c > 0`，外层 `loop` 无限自旋充电——**内核永久挂死**，无看门狗、无 idle 任务兜底。
- **修复**：无 Runnable 任务时进入 idle 任务/`hlt`，而不是自旋。

### H6. 内核登录明文密码 `root/root`，无锁定、无限速、无 hash

- **位置**：`src/kernel/terminal.rs:28-45`、`terminal.rs:578-589`
- **代码**：
  ```rust
  const ACCOUNTS: [Account; 2] = [
      Account { username: "root", password: "root", ... },   // ← 明文
      Account { username: "user", password: "rustix", ... },
  ];
  fn handle_login_password(trimmed: &str) {
      if ACCOUNTS[index].password == trimmed { complete_login(index); ... }
      crate::println!("Login incorrect");   // 无限重试，无锁定
  }
  ```
- **影响**：密码明文存于内核镜像、暴力破解无任何防护；所有"进程"共享同一内核地址空间，登录隔离形同虚设（没有 uid/gid 权限模型，`filp`/`Inode` 全是死结构）。"Linux login" 只是样子货。
- **修复**：密码 hash 存储 + 失败锁定/退避；在真实权限模型落地前不要宣称登录安全。

### H7. 内存池只有前 1000 页 + `SimpleVec::push` 满则静默丢弃

- **位置**：`src/kernel/mm.rs:262-263`（`min(1000, total_pages)`）、`mm.rs:184-189`（`push` 满时静默 return）
- **影响**：声称初始化 256MB 内存（`init_memory_manager(0x100000, 0x10000000)`），实际只登记 1000 页（4MB）；`SimpleVec` 固定 1000 容量，`push` 溢出时**无声丢弃**——内存池耗尽后所有 `alloc_page()` 返回 `None`，上层 `.expect()` 直接 panic。可分配内存远小于宣称，且分配失败路径大多未处理。
- **修复**：按真实内存登记页帧；`push` 溢出返回错误而不是静默。

### H8. `get_free_page()` 双重记账

- **位置**：`src/kernel/mm.rs:274-283` + `mm.rs:408-420`
- **代码**：
  ```rust
  // alloc_page() 内部已经 push 了一次：
  pub fn alloc_page(&mut self) -> Option<PageFrame> {
      let mut page = self.free_pages.pop()?;
      page.count += 1;
      self.allocated_pages.push(page.addr);   // ← 第 1 次
      Some(page)
  }
  // get_free_page() 又 push 一次：
  pub fn get_free_page() -> Option<u32> {
      let page = mm.alloc_page()?;
      let addr = page.addr;
      mm.allocated_pages.push(addr);          // ← 第 2 次（重复条目）
  ```
- **影响**：`allocated_pages` 出现重复地址，`MemoryManager::free_page` 按地址移除会一次删掉多条记录，空闲/已分配计数失真（`free_pages_count`/`allocated_pages_count` 全部不可信，`/proc/meminfo` 数据错误）。
- **修复**：去掉重复 push。

### H9. 引导链：硬编码内核入口 `0x00009799`、默认只读 1 扇区、ES:BX 64KB 回卷

- **位置**：`src/bios_stage2.asm:11-17`、`bios_stage2.asm:46-67`、`bios_stage2.asm:164`
- **代码**：
  ```asm
  %ifndef KERNEL_ENTRY
  %define KERNEL_ENTRY 0x00009799    ; ← 魔法地址，链接不同即跳飞
  %endif
  %ifndef KERNEL_SECTORS
  %define KERNEL_SECTORS 1           ; ← 默认只读 512 字节内核！
  %endif
  ...
  .read_loop:
      add bx, 512                     ; BX 超过 0xFFFF 回卷 → ES:BX 回到 0x7E00 覆盖已加载代码
      dec si
      inc cl
  ```
- **影响**：`KERNEL_SECTORS` 默认 1 扇区，而真实内核远超 512B——按默认构建则跳到 0x9799 处的是垃圾数据；入口地址硬编码（`build_bootsector.sh` 用 `nm` 现算并覆盖，但一旦漏传就是魔法值）；磁盘读取用 `ES:BX` 线性累加，超过 64KB（约 128 扇区）后 BX 回卷**覆盖已加载的内核**。`build_bootsector.sh:57` 虽有 `2880-2` 扇区上限检查，但没检查 64KB 回卷。
- **修复**：用 ES 增量（每 128 扇区 `add es, 0x1000`）跨 64KB 边界；入口地址由链接脚本/符号表强制生成并校验。

---

## 四、🟡 MEDIUM —— 中危 / 健壮性 / 宣称不符

### M1. 三份 `init_idt`，加载的偏偏是 32 位那套；64 位那套从未被调用

- `src/kernel/interrupts.rs:500-517` `init_interrupts()`：初始化 32 位 IDT 并 `lidt`（**main.rs:139 实际调用的是它**）
- `src/arch/x86_64/idt.rs:324-405` `init_idt()`：正确的 16 字节 64 位 IDT，**全仓库无任何调用**
- `src/arch/x86_64/mod.rs:119-126` `init_idt()`：占位函数，只在 VGA 上写个字母 'I'，**也是无人调用**
- 影响：架构层正确实现了 64 位 IDT 却接不上，加载的是错误格式（见 C8）。三套并存、互相矛盾，是典型的"代码重复严重 + 宣称已修复"实锤。

### M2. PIC 初始化执行两遍

- `src/arch/x86_64/mod.rs:96`（arch::init 内 init_pic）+ `src/kernel/interrupts.rs:502`（kernel init 内 init_pic）各跑一遍完整 ICW1-4 + 屏蔽序列。第二遍以全屏蔽（0xFF）收尾，随后 `enable_irq(0/1)` 只放行 timer+keyboard，其余 IRQ 全关——与 `FIXES_SUMMARY.md` "修复了中断处理"的宣称不符。

### M3. 键盘中断处理函数把每个扫描码打到控制台

- `src/kernel/interrupts.rs:365-378`：`keyboard_interrupt_handler` 对每个按键 `println!("Keyboard scancode: 0x{:x}")`——正常打字就是刷屏风暴；且它与 `terminal.rs` 的轮询键盘（`poll_input` 读 0x60）**同时读同一个 PS/2 端口**，中断路径与轮询路径互相抢字节，输入会丢。
- 影响：键盘数据竞争 + 控制台洪泛。

### M4. 统一驱动框架是死代码

- `src/drivers/mod.rs` 定义了 `Console/Display/InterruptController/MemoryManager/Clock` trait + `DRIVER_MANAGER` 全局，但 `grep` 全仓库，**除 drivers/mod.rs 自身外无任何使用**。`FIXES_SUMMARY.md` 声称"驱动框架现在被实际使用"——不实。

### M5. `long_mode_init.asm` 调用不存在的符号 `_start_rust`

- `src/arch/x86_64/boot/long_mode_init.asm:13,41`：`extern _start_rust; call _start_rust`，而 `main.rs` 只定义了 `_start`。任何链接到这个文件的构建必然链接失败，或（若用 `--undefined` 忽略）运行时跳飞。多入口点（`_start` / `_start_asm` / `long_mode_start` / `_start_rust`）各引导文件各叫各的，构建链混乱。

### M6. 多份 `main_*.rs` 内核入口并存

- `src/main.rs`（正式）、`main_fixed.rs`、`main_minimal.rs`、`main_simple.rs`、`minimal_kernel.asm` + `FIXES_SUMMARY.md` 声称的 `target/.../release/rustix 4.9KB` 与 `rustix_fixed.iso` 产物不在仓库中（README 也不存在，Cargo.toml 却 `readme = "README.md"`）。仓库状态与"所有问题已修复"的总结文档对不上。

### M7. devfs 声明 `read_only: false` 但没有写路径

- `src/kernel/fs/dev.rs:16`：`read_only: false`，但 `dev::read()` 之后没有任何 `write` 实现；VFS 的 `open` 也不检查 `flags`（O_RDWR/O_WRONLY 都被接受），写请求会落到 `fs::write` 的 `Unsupported`。声明与实现不符。

### M8. 假系统文件：`/etc/passwd`、SSH `authorized_keys`、集群配置全是静态文本

- `src/kernel/fs/ext4.rs:35-37`：硬编码 `root:x:0:0` / `user:x:1000:1000` 的假 passwd；`ext4.rs:82-86` 硬编码两把 ed25519 公钥进 `authorized_keys`；`service.rs` 的 `SERVICES`/`NODES` 静态表 + `apply_action` 只改内存里的假状态。没有任何用户、组、网络、SSH、NFS、OpenMPI 实现——"集群栈 ready"是虚构的。若有人照此部署会误以为存在这些服务。

### M9. `virt_to_phys` 恒等映射 + 无任何 MMU 隔离

- `src/kernel/mm.rs:302-305`：`virt_to_phys` 直接返回 `Some(virt_addr)`（注释自称"简单线性映射"）；内核从不建立/切换用户页表（`copy_page_tables` 是坏的，C3），`paging.rs` 的 `EmptyFrameAllocator` 永远返回 `None`。整个系统没有虚拟内存概念，"Linux-compatible virtual memory management"（LINUX_COMPATIBILITY_PLAN.md:36）纯属规划文档。

### M10. 日志/缓冲区健壮性小问题

- `src/kernel/log.rs:19`：`if self.len > self.buf.len()` 应为 `>=`（恰好满时下次不重置）；`copy_within(overflow..self.len, 0)` 移位逻辑在 `len` 恰为 0 时行为可疑。
- `src/lib.rs:30-39`：print 缓冲 1024 字节截断时静默，长日志内容丢失且无提示。

---

## 五、结论：Linux ABI/API 兼容性逐项判定

| Linux x86-64 事实 | rustix 现状 | 判定 |
|---|---|---|
| syscall 号（read=0/write=1/open=2/exit=60/…) | i386 号（read=3/write=4/open=5/exit=1/…) | ❌ 不兼容 |
| 6 参传递 rdi/rsi/rdx/r10/r8/r9 | handler 只有 3 参 | ❌ 不兼容 |
| 负 errno 返回 | `-1` / 自定义 FsError 映射 | ❌ 不兼容 |
| `syscall` 指令 / int 0x80 门 + LSTAR | 无门、无 MSR 配置、0x80 未注册 | ❌ 不兼容 |
| ring3 用户态 + 页表隔离 | 无用户态，程序是 ring0 函数指针 | ❌ 不兼容 |
| ELF 加载 | "ELF loader not implemented yet" | ❌ 不兼容 |
| 调度（真实上下文切换） | `switch_to` 空操作 | ❌ 不兼容 |
| 信号/futex/mmap/mprotect | 全部不存在（syscall 表 72 槽仅 42 个） | ❌ 不兼容 |
| /dev/random CSPRNG | 常数种子 LCG | ❌ 不兼容 |
| 多架构支持 | aarch64/arm 仅占位 mod.rs | ❌ 名不副实 |

**判定：该项目不具备任何 Linux ABI/API 兼容性，达不到"最小可用内核"标准，更谈不上"未来的 Linux"。** 建议作者：① 在 README/docs 中撤回全部 Linux 兼容性宣称，如实描述为"教学用 Rust 内核 shell 演示"；② 按本报告 C1-C12 顺序修复致命缺陷；③ `FIXES_SUMMARY.md` 的"所有问题已修复"应改写为真实状态。

## 六、修复优先级建议

1. **P0（不修复不可启动/必然崩溃）**：C1（分配地址为 0）、C8（32 位 IDT）、C9（异常处理错误码/iretq）、C11（无 syscall 门）
2. **P1（安全与正确性）**：C2（verify_area 反转）、C3/C4（页表复制/释放）、C6（switch_to）、C7（TSS 布局）、C10（无指针校验）、H1（伪随机）、H3（信息泄露）、H4（无锁 UB）
3. **P2（健壮性）**：C5（free 空操作）、H2（全局缓冲别名）、H5（调度死循环）、H6（明文密码）、H7/H8（内存池记账）、H9（引导链）、M1-M10
4. **P3（诚实性）**：撤回 Linux 兼容宣称、清理死代码与多入口、删除假系统文件

## 七、审计范围与方法

- 通读文件：`src/kernel/{mm,sched,syscall,fs,fs/*,terminal,apps,service,interrupts,log,mod}.rs`、`src/arch/{mod,x86_64/mod,x86_64/idt,x86_64/drivers/*}.rs`、`src/{main,lib,drivers/mod}.rs`、`src/user/{mod,runtime,hello,game}.rs`、全部 boot 汇编（`src/*.asm`、`src/arch/x86_64/boot/*.asm`）、`build_*.sh`、`Cargo.toml`、`.cargo/config.toml`、`FIXES_SUMMARY.md`、`docs/*.md`、`api/*.md`
- 对照基准：Intel SDM（x86-64 长模式 IDT/TSS/GDT 格式）、Linux x86-64 syscall ABI、musl 汇编层、Linux 0.11 源码（本项目自称的移植蓝本）
- 验证方式：静态代码审计（未运行 QEMU；项目自带构建产物 `kernel.bin` 等与源码不一致，无法直接复现运行时行为，但上述格式/逻辑错误均为确定性代码缺陷）

---
*本报告由对 `rustix` 仓库（commit 6821985）的逐行静态审计生成，所有引用行号对应上述 commit。*




