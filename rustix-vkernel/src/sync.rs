use core::cell::UnsafeCell;
use core::hint::spin_loop;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

/// Stable storage for memory that hardware or early BSP-only code addresses
/// through a raw pointer. Access is deliberately unsafe: the owning driver
/// must hold its lock (or still be in single-CPU boot) for the whole access.
pub struct StaticCell<T> {
    value: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for StaticCell<T> {}

impl<T> StaticCell<T> {
    pub const fn new(value: T) -> Self {
        Self {
            value: UnsafeCell::new(value),
        }
    }

    pub const fn get(&self) -> *mut T {
        self.value.get()
    }
}

/// Minimal no_std mutex for short, non-sleeping kernel critical sections.
pub struct SpinMutex<T> {
    locked: AtomicBool,
    value: UnsafeCell<T>,
}

// The atomic lock gives exclusive access before UnsafeCell is dereferenced.
unsafe impl<T: Send> Sync for SpinMutex<T> {}

impl<T> SpinMutex<T> {
    pub const fn new(value: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    pub fn lock(&self) -> SpinMutexGuard<'_, T> {
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            while self.locked.load(Ordering::Relaxed) {
                spin_loop();
            }
        }
        SpinMutexGuard { mutex: self }
    }
}

pub struct SpinMutexGuard<'a, T> {
    mutex: &'a SpinMutex<T>,
}

impl<T> Deref for SpinMutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // A guard exists only after acquiring the lock.
        unsafe { &*self.mutex.value.get() }
    }
}

impl<T> DerefMut for SpinMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // Only one guard can exist while `locked` is true.
        unsafe { &mut *self.mutex.value.get() }
    }
}

impl<T> Drop for SpinMutexGuard<'_, T> {
    fn drop(&mut self) {
        self.mutex.locked.store(false, Ordering::Release);
    }
}

/// Spin lock for data shared with interrupt context.
///
/// Ranks must be acquired in strictly increasing order. Interrupt state is
/// restored to the exact state observed by the outermost guard.
pub struct IrqSpinMutex<T, const RANK: u8> {
    locked: AtomicBool,
    value: UnsafeCell<T>,
}

unsafe impl<T: Send, const RANK: u8> Sync for IrqSpinMutex<T, RANK> {}

impl<T, const RANK: u8> IrqSpinMutex<T, RANK> {
    pub const fn new(value: T) -> Self {
        assert!(RANK != 0, "IRQ lock rank zero is reserved");
        Self {
            locked: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    pub fn lock(&self) -> IrqSpinMutexGuard<'_, T, RANK> {
        let interrupt_state = InterruptState::disable();
        if !crate::arch::lock_order_allows(RANK) {
            panic!(
                "lock order violation: cpu={} rank={}",
                crate::arch::current_cpu_id(),
                RANK
            );
        }
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            while self.locked.load(Ordering::Relaxed) {
                spin_loop();
            }
        }
        if !crate::arch::push_lock_rank(RANK) {
            self.locked.store(false, Ordering::Release);
            panic!(
                "lock tracking overflow: cpu={} rank={}",
                crate::arch::current_cpu_id(),
                RANK
            );
        }
        IrqSpinMutexGuard {
            mutex: self,
            interrupt_state,
        }
    }
}

pub struct IrqSpinMutexGuard<'a, T, const RANK: u8> {
    mutex: &'a IrqSpinMutex<T, RANK>,
    interrupt_state: InterruptState,
}

impl<T, const RANK: u8> Deref for IrqSpinMutexGuard<'_, T, RANK> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.mutex.value.get() }
    }
}

impl<T, const RANK: u8> DerefMut for IrqSpinMutexGuard<'_, T, RANK> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.mutex.value.get() }
    }
}

impl<T, const RANK: u8> Drop for IrqSpinMutexGuard<'_, T, RANK> {
    fn drop(&mut self) {
        if !crate::arch::pop_lock_rank(RANK) {
            panic!(
                "lock release order violation: cpu={} rank={}",
                crate::arch::current_cpu_id(),
                RANK
            );
        }
        self.mutex.locked.store(false, Ordering::Release);
        self.interrupt_state.restore();
    }
}

struct InterruptState {
    state: usize,
    restored: bool,
}

impl InterruptState {
    fn disable() -> Self {
        let state = disable_interrupts_save();
        crate::arch::irq_disabled_enter();
        Self {
            state,
            restored: false,
        }
    }

    fn restore(&mut self) {
        if self.restored {
            return;
        }
        self.restored = true;
        if !crate::arch::irq_disabled_exit() {
            panic!("interrupt-disable depth underflow");
        }
        restore_interrupts(self.state);
    }
}

impl Drop for InterruptState {
    fn drop(&mut self) {
        self.restore();
    }
}

pub fn irq_lock_self_test() -> bool {
    static OUTER: IrqSpinMutex<u32, 10> = IrqSpinMutex::new(0);
    static INNER: IrqSpinMutex<u32, 20> = IrqSpinMutex::new(0);

    let enabled_before = interrupts_enabled();
    {
        let mut outer = OUTER.lock();
        if interrupts_enabled() || crate::arch::irq_disable_depth() != 1 {
            return false;
        }
        *outer += 1;
        {
            let mut inner = INNER.lock();
            if interrupts_enabled()
                || crate::arch::irq_disable_depth() != 2
                || crate::arch::lock_order_allows(10)
                || !crate::arch::lock_order_allows(30)
            {
                return false;
            }
            *inner += 1;
        }
    }
    interrupts_enabled() == enabled_before
        && crate::arch::irq_disable_depth() == 0
        && *OUTER.lock() == 1
        && *INNER.lock() == 1
}

#[cfg(target_arch = "x86_64")]
fn disable_interrupts_save() -> usize {
    let flags: usize;
    unsafe {
        core::arch::asm!(
            "pushfq",
            "pop {}",
            "cli",
            out(reg) flags,
            options(nomem)
        )
    };
    flags
}

#[cfg(target_arch = "aarch64")]
fn disable_interrupts_save() -> usize {
    let state: usize;
    unsafe {
        core::arch::asm!(
            "mrs {}, daif",
            "msr daifset, #0xf",
            out(reg) state,
            options(nomem, nostack)
        )
    };
    state
}

#[cfg(target_arch = "x86_64")]
fn restore_interrupts(state: usize) {
    if state & (1 << 9) != 0 {
        unsafe { core::arch::asm!("sti", options(nomem, nostack, preserves_flags)) };
    }
}

#[cfg(target_arch = "aarch64")]
fn restore_interrupts(state: usize) {
    unsafe { core::arch::asm!("msr daif, {}", in(reg) state, options(nomem, nostack)) };
}

#[cfg(target_arch = "x86_64")]
fn interrupts_enabled() -> bool {
    let flags: usize;
    unsafe { core::arch::asm!("pushfq", "pop {}", out(reg) flags, options(nomem)) };
    flags & (1 << 9) != 0
}

#[cfg(target_arch = "aarch64")]
fn interrupts_enabled() -> bool {
    let state: usize;
    unsafe { core::arch::asm!("mrs {}, daif", out(reg) state, options(nomem, nostack)) };
    state & (1 << 7) == 0
}
