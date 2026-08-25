#[cfg(target_arch = "x86_64")]
#[path = "net_x86_64.rs"]
mod x86_64;

#[cfg(target_arch = "x86_64")]
pub use x86_64::*;

#[cfg(target_arch = "aarch64")]
use core::fmt::Write;

#[cfg(target_arch = "aarch64")]
use crate::fixed::InlineString;

#[cfg(target_arch = "aarch64")]
#[derive(Clone, Copy)]
pub struct NetStatus {
    pub mac: [u8; 6],
    pub ip: [u8; 4],
    pub netmask: [u8; 4],
    pub gateway: [u8; 4],
    pub dns: [u8; 4],
    pub io_base: u16,
    pub up: bool,
}

#[cfg(target_arch = "aarch64")]
#[derive(Clone, Copy)]
pub struct PingReply {
    pub from: [u8; 4],
    pub seq: u16,
    pub ttl: u8,
    pub bytes: usize,
}

#[cfg(target_arch = "aarch64")]
pub fn init() -> bool {
    false
}

#[cfg(target_arch = "aarch64")]
pub fn status() -> Option<NetStatus> {
    None
}

#[cfg(target_arch = "aarch64")]
pub fn ping(_target: [u8; 4]) -> Result<PingReply, &'static str> {
    Err("net: no ARM NIC driver yet")
}

#[cfg(target_arch = "aarch64")]
pub fn parse_ipv4(text: &str) -> Option<[u8; 4]> {
    crate::c_fastpath::parse_ipv4(text)
}

#[cfg(target_arch = "aarch64")]
pub fn format_ipv4<const N: usize>(ip: [u8; 4], out: &mut InlineString<N>) {
    out.clear();
    let _ = write!(out, "{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]);
}

#[cfg(target_arch = "aarch64")]
pub fn format_mac<const N: usize>(mac: [u8; 6], out: &mut InlineString<N>) {
    out.clear();
    let _ = write!(
        out,
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    );
}

#[cfg(target_arch = "aarch64")]
pub const TCP_MSS: usize = 1400;

#[cfg(target_arch = "aarch64")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NetErr {
    Again,
    Down,
    Exhausted,
    Refused,
    TimedOut,
    Reset,
}

#[cfg(target_arch = "aarch64")]
pub fn pump() -> usize {
    0
}

#[cfg(target_arch = "aarch64")]
pub fn tcp_connect(_dst: [u8; 4], _port: u16, _timeout_ms: u64) -> Result<usize, NetErr> {
    Err(NetErr::Down)
}

#[cfg(target_arch = "aarch64")]
pub fn tcp_send(_conn: usize, _data: &[u8]) -> Result<usize, NetErr> {
    Err(NetErr::Down)
}

#[cfg(target_arch = "aarch64")]
pub fn tcp_recv(_conn: usize, _out: &mut [u8]) -> Result<usize, NetErr> {
    Err(NetErr::Down)
}

#[cfg(target_arch = "aarch64")]
pub fn tcp_connected(_conn: usize) -> bool {
    false
}

#[cfg(target_arch = "aarch64")]
pub fn tcp_readable(_conn: usize) -> bool {
    false
}

#[cfg(target_arch = "aarch64")]
pub fn tcp_close(_conn: usize) -> Result<(), NetErr> {
    Err(NetErr::Down)
}
