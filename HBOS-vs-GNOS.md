# HBOS vs GNOS 全方位激烈吐槽

## 基本盘

| | HBOS | GNOS |
|---|---|---|
| 仓库 | HeBitOS/HBOS（286 提交） | 本仓库（11 提交，但每个都很大） |
| 语言 | C + NASM | C + GAS asm |
| 架构 | x86_64 长模式 | x86_64 + SMP |
| 代码量 | **70k 行全自研**（src 59,447 C + 9,928 H + 1,078 asm） | **215k 行**（含 159k 移植 Linux DRM；内核自研约 41k） |
| 启动 | Multiboot2 BIOS（GRUB2）+ Limine UEFI 双启动 | Limine（BIOS+UEFI 双模式，static-pie 高半区） |
| 构建 | Makefile + version.mk（交互式 `make ver`）+ genhax.py + 多套脚本 | 单 Makefile + gcc-13 + xorriso + mke2fs |
| 定位 | "小而可用"全自研 hobby OS，自带桌面/编译器/浏览器梦 | 教学型 OS：让**真 Linux 用户态**（musl/Bash/coreutils）直接跑 |
| 版本 | v0.1-beta5-pre6，版本号工程化（version.mk） | 无版本号，commit 即版本 |

## 完成度对比

| 能力 | HBOS | GNOS |
|---|---|---|
| 调度器 | 循环链表**轮转**（MAX_TASKS 64，spinlock） | **EEVDF**（Linux 6.6 同款：vruntime + 虚拟截止时间 + 二叉最小堆，slice=5 ticks） |
| SMP | ✅ 8 核（lapic + trampoline 0x8000 + spinlock） | ✅ 16 核（Limine SMP，每核独立 GS/GDT/TSS） |
| 系统调用 | int 0x80（自称兼容 Linux 旧 ABI），137 个分发 | **Linux/x86-64 ABI**（syscall 指令，158 个，负 errno），编号头文件内核/用户共享 |
| 用户空间 | 自造 HAX 应用格式 + 自研 libc + ldso 动态链接 + **内嵌 TinyCC**（cc 命令即编译器） | 完整 ELF + musl 动态/静态链接，**真跑 Bash 5.3 / coreutils 9.9 / BusyBox 1.38** |
| 文件系统 | ramfs + FAT32 + **HBFS（自造格式）** + ext2（居然支持写）+ devfs | ext2 读写 + tmpfs + procfs + FAT + epoll + anonfd |
| 显示 | flanterm + CJK 字体 + **HIVE 桌面**（合成器+窗口管理器+8 个内置应用+浏览器） | fbcon/fbdev（/dev/fb0 可 mmap）+ **移植 Linux DRM 全套**（atomic/GEM/connector） |
| 输入 | PS/2 键鼠 + **USB xHCI 键盘/鼠标/存储** | evdev（event0/1）+ libinput 移植 |
| 网络 | E1000/PCnet，DHCP/ARP/ICMP/DNS/TCP/HTTP + **TLS**（mbedtls shim + secure_https） | e1000 + 自研 tcp/sock/net 栈 |
| 加密 | ✅ ChaCha20-Poly1305 / SHA-256 / X25519 / P256 / AES-GCM + 自测 | ❌ |
| 音频 | AC97 | AC97/HDA 只有头文件 |
| 内核模块 | ❌（全静态链接） | ✅ ELF 内核模块动态加载（insmod + moddemo） |
| 浏览器 | browser_backend + **Chromium 兼容路线图**（browser/chromium/ 目录） | ❌（Wayland 桌面 labwc 交叉编译中） |
| 测试 | selftest.c + Python 脚本（GUI 截图/USB/网络自动化） | **开机逐条断言**（/etc/rc 跑 bashtest.sh + coreutilstest.sh）+ `make test` 无头 QEMU |

## 子系统深挖

### 调度器：纸飞机 vs 民航客机

HBOS 的 `sched_next()` 是教科书式循环链表轮转：从 current 出发绕一圈找第一个 READY。老实、够用、一眼看懂。
GNOS 直接上 **EEVDF**——全局虚拟时钟、进程虚拟运行时间、虚拟截止时间 = vtime + slice、二叉最小堆运行队列、timer 中断里做截止时间抢占判定，连注释都写着"与 Linux 6.6+ 同款"。
一个还在"下一个就绪的"层面，一个在"下一个截止时间最近的"层面。差距不是 64 个任务槽位能弥补的。

### 系统调用哲学：自洽 vs 契约

HBOS 用 int 0x80（自称"与 Linux 旧式 ABI 兼容"），137 个调用号，errno 负值返回——形似而神不似，musl 往上一放就露馅。
GNOS 把 **Linux/x86-64 ABI 当唯一契约**：编号、寄存器约定、负 errno 全对齐，`src/shared/sysnum.h` 内核和用户态共用一份头文件，"二者不会漂移"。结果就是 GNOS 能直接喂 musl、BusyBox、Bash、coreutils 这些真货，HBOS 只能自产自销 HAX 格式应用 + 自研 libc。HBOS 靠内嵌 TinyCC 在 OS 里写 C 很酷，但这是"没有外部生态"的补偿机制。

### 用户态：自己玩 vs 真货优先

HBOS：HAX 应用格式 + 自己的 libc + 自己的动态链接器 + 内嵌 TinyCC + HPT 包管理种子——一整套自洽生态，浪漫，但外面没人用这套格式。
GNOS：`bash_cv_termcap_lib=gnutermcap`、`linux-stub`——为了把成熟第三方用户栈搬上来做的手工配置，README 直言"与其自造玩具用户态，不如把内核做对"。
浪漫主义者 vs 实用主义者。HBOS 的生态是花房，GNOS 直接住在 Linux 生态里。

### 网络：HBOS 有 TLS，GNOS 没有

HBOS 的 net.c 1549 行协议栈 + mbedtls shim + secure_https，能跑 HTTPS；GNOS 网络栈是 e1000 + 自研 tcp/sock/net，没有加密层。但 GNOS 自研 TCP 的意义在于"内核里长出来的"，HBOS 的 TLS 是移植 mbedtls 的（net/mbedtls_shim 目录）。一个自研到传输层，一个自研到应用层再借第三方做 TLS。各有半瓶水。

### 驱动：USB vs DRM

HBOS 实打实实现了 **USB xHCI 控制器 + HID 键鼠 + MSC 存储**，这是硬骨头，含金量高。GNOS 没有 USB，但有**移植的 Linux DRM 全套**（drm_atomic/connector/crtc/gem/mm/rbtree/idr/hashtab）——160k 行第三方移植进来的能力，和自研的含金量是两回事。GNOS 的 /dev 设备注册（dri/card0、input/event0/1）和真实驱动一一对应，不是摆设。

## HBOS 的亮点与暴击点

**亮点：**
- 70k 行全自研，从引导到桌面到浏览器后端到内嵌编译器，闭环完整，osdev 精神拉满
- USB xHCI 三件套（键盘/鼠标/存储）是真功夫，很多 hobby OS 一辈子不敢碰
- 文档工程化惊人：每个子系统一份 `*.agent.md`、API 文档 HTML/PDF、Chromium 兼容基线、LINUX_KDE_COMPAT 路线图——比很多开源项目的文档还齐全
- `make ver`/`make ask` 交互式版本管理 + 多构建形态开关（GUI/nogui/apps 选择），工程洁癖
- 自带壁纸/字体（鸿蒙字体+CJK 点阵）/截图，成品感强

**暴击点：**
- int 0x80 + 自造 HAX 格式 + 自研 libc = 生态孤岛；Chromium 兼容路线图基本是画饼——Chromium 要沙箱、GPU 进程、几十个 syscall 和内存特性，v0.1 内核离那还有几光年
- 合成器+窗口管理器 164+381 行，这规模叫"桌面"不如叫"会移动的窗口"
- ext2 居然支持写——对着真实文件系统格式写数据，勇气可嘉，坏盘风险自负
- MAX_TASKS 64、轮转调度，多任务深度和 GNOS 的 EEVDF 完全不是一个时代
- HBFS 是什么自造文件系统，除了作者没人知道；版本号精确到 pre6 但 git 提交信息是 "fix: black console"

## GNOS 的亮点与暴击点

**亮点：**
- 真跑 Bash 5.3 + coreutils 9.9 + BusyBox，开机自动断言，`make test` 无头验证——测试自动化比大多数 hobby OS 强一个量级
- EEVDF + 每核私有 GDT/TSS/GS 的 SMP，调度器直接对齐 Linux 6.6，教科书没有这么深的
- syscall ABI 对齐 + 共享编号头文件的设计，是"让真软件跑起来"的关键，值得所有教学 OS 抄
- ELF 内核模块动态加载、epoll、anonfd、ptrace——Linux 特性学了个遍

**暴击点：**
- 215k 行里 159k 是移植的 Linux DRM——"内核"水分不小，自研部分约 56k 行
- 为了跑 Wayland 手写假 libudev：设备表硬编码 3 个节点，热插拔是根永远读不到数据的管道——"桌面"是硬凑的
- 音频只有头文件没有实现；网络没有 TLS；Wayland 桌面还在交叉编译路上（libffi/wayland/libinput 血泪史）
- 11 个提交每个都是巨无霸，没有小步提交习惯；README 自称"最强的小学生开发的操作系统"，营销味重
- 私有 syscall 号挤在 400-441（gethostname/signal/ttyinject/inputinject...），是 ABI 设计上的补丁式补丁

## 总结陈词

- **HBOS** 是"浪漫主义全自研"的集大成者：从零长出一个带桌面、编译器、浏览器梦、TLS 和 USB 的小宇宙，文档工程化一流，但生态自洽到只有自己能跑，Chromium 路线图是给投资人看的。
- **GNOS** 是"实用主义契约派"：把 Linux ABI 当圣经，让真用户态跑起来，调度器/测试/SMP 深度碾压，但靠 160k 行移植撑门面，GUI 还在借 Wayland 的壳。
- 一句话：**HBOS 在造自己的车（连轮子都自己铸），GNOS 在给 Linux 的轮子造一辆能装上的车。** 前者浪漫，后者能跑。都还到不了"成熟"，但各自在各自的路线上都跑得很认真。
