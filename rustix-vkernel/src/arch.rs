#[cfg(target_arch = "x86_64")]
#[path = "arch_x86_64.rs"]
mod x86_64;

#[cfg(target_arch = "aarch64")]
#[path = "arch_aarch64.rs"]
mod aarch64;

#[cfg(target_arch = "x86_64")]
pub use x86_64::*;

#[cfg(target_arch = "aarch64")]
pub use aarch64::*;

pub enum ConsoleInput {
    Byte(u8),
    Key { code: u8, translated: Option<u8> },
    MouseByte(u8),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ClockTime {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

pub struct PlatformInfo {
    pub name: &'static str,
    pub shell_banner: &'static str,
    pub uname: &'static str,
    pub proc_version: &'static str,
    pub ipc_trace_supported: bool,
    pub backspace_echo: &'static str,
}

pub fn arch_name() -> &'static str {
    platform_info().name
}

pub fn shell_banner() -> &'static str {
    platform_info().shell_banner
}

pub fn uname() -> &'static str {
    platform_info().uname
}

pub fn proc_version() -> &'static str {
    platform_info().proc_version
}

pub fn supports_ipc_trace() -> bool {
    platform_info().ipc_trace_supported
}

pub fn backspace_echo() -> &'static str {
    platform_info().backspace_echo
}

pub fn poll_console_input() -> Option<ConsoleInput> {
    #[cfg(target_arch = "x86_64")]
    {
        x86_64::poll_console_input()
    }

    #[cfg(target_arch = "aarch64")]
    {
        aarch64::poll_console_input()
    }
}

pub fn read_console_byte() -> Option<u8> {
    match poll_console_input()? {
        ConsoleInput::Byte(byte) => Some(byte),
        ConsoleInput::Key {
            translated: Some(byte),
            ..
        } => Some(byte),
        ConsoleInput::Key {
            translated: None, ..
        }
        | ConsoleInput::MouseByte(_) => None,
    }
}

pub fn read_clock_time() -> Option<ClockTime> {
    #[cfg(target_arch = "x86_64")]
    {
        x86_64::read_clock_time()
    }

    #[cfg(target_arch = "aarch64")]
    {
        aarch64::read_clock_time()
    }
}

fn platform_info() -> &'static PlatformInfo {
    #[cfg(target_arch = "x86_64")]
    {
        &x86_64::PLATFORM_INFO
    }

    #[cfg(target_arch = "aarch64")]
    {
        &aarch64::PLATFORM_INFO
    }
}
