use core::fmt::Write;
use core::sync::atomic::{compiler_fence, Ordering};

use crate::fixed::InlineString;
use crate::sync::{SpinMutex, StaticCell};

const RTL8139_VENDOR_ID: u16 = 0x10EC;
const RTL8139_DEVICE_ID: u16 = 0x8139;

const ETHERTYPE_ARP: u16 = 0x0806;
const ETHERTYPE_IPV4: u16 = 0x0800;
const ARP_HTYPE_ETHERNET: u16 = 1;
const ARP_PTYPE_IPV4: u16 = 0x0800;
const ARP_OP_REQUEST: u16 = 1;
const ARP_OP_REPLY: u16 = 2;
const ICMP_ECHO_REQUEST: u8 = 8;
const ICMP_ECHO_REPLY: u8 = 0;

const RTL_REG_IDR0: u16 = 0x00;
const RTL_REG_TSD0: u16 = 0x10;
const RTL_REG_TSAD0: u16 = 0x20;
const RTL_REG_RBSTART: u16 = 0x30;
const RTL_REG_CR: u16 = 0x37;
const RTL_REG_CAPR: u16 = 0x38;
const RTL_REG_CBR: u16 = 0x3A;
const RTL_REG_IMR: u16 = 0x3C;
const RTL_REG_ISR: u16 = 0x3E;
const RTL_REG_TCR: u16 = 0x40;
const RTL_REG_RCR: u16 = 0x44;
const RTL_REG_CONFIG1: u16 = 0x52;

const RTL_CR_RESET: u8 = 0x10;
const RTL_CR_RX_ENABLE: u8 = 0x08;
const RTL_CR_TX_ENABLE: u8 = 0x04;
const RTL_CR_BUFFER_EMPTY: u8 = 0x01;

const RTL_ISR_ROK: u16 = 0x0001;
const RTL_ISR_TOK: u16 = 0x0004;
const RTL_ISR_RX_ERR: u16 = 0x0002;
const RTL_ISR_RX_OVW: u16 = 0x0010;

const RTL_TSD_TOK: u32 = 1 << 15;
const RTL_TSD_TABT: u32 = 1 << 30;

const RX_RING_LEN: usize = 8192;
const RX_ALLOC_LEN: usize = RX_RING_LEN + 16 + 1500;
const TX_BUFFER_LEN: usize = 2048;
const MAX_FRAME_LEN: usize = 1600;
const DEFAULT_TTL: u8 = 64;
const ICMP_IDENTIFIER: u16 = 0x5258;

const DEFAULT_IP: [u8; 4] = [10, 0, 2, 15];
const DEFAULT_NETMASK: [u8; 4] = [255, 255, 255, 0];
const DEFAULT_GATEWAY: [u8; 4] = [10, 0, 2, 2];
const DEFAULT_DNS: [u8; 4] = [10, 0, 2, 3];

#[repr(align(16))]
struct AlignedRx([u8; RX_ALLOC_LEN]);

#[repr(align(16))]
struct AlignedTx([u8; TX_BUFFER_LEN]);

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

#[derive(Clone, Copy)]
pub struct PingReply {
    pub from: [u8; 4],
    pub seq: u16,
    pub ttl: u8,
    pub bytes: usize,
}

#[derive(Clone, Copy)]
struct NetworkState {
    io_base: u16,
    mac: [u8; 6],
    ip: [u8; 4],
    netmask: [u8; 4],
    gateway: [u8; 4],
    dns: [u8; 4],
    tx_slot: usize,
    rx_offset: usize,
    icmp_seq: u16,
    arp_ip: [u8; 4],
    arp_mac: [u8; 6],
    arp_valid: bool,
    ready: bool,
}

impl NetworkState {
    const fn new() -> Self {
        Self {
            io_base: 0,
            mac: [0; 6],
            ip: DEFAULT_IP,
            netmask: DEFAULT_NETMASK,
            gateway: DEFAULT_GATEWAY,
            dns: DEFAULT_DNS,
            tx_slot: 0,
            rx_offset: 0,
            icmp_seq: 1,
            arp_ip: [0; 4],
            arp_mac: [0; 6],
            arp_valid: false,
            ready: false,
        }
    }
}

static STATE: SpinMutex<NetworkState> = SpinMutex::new(NetworkState::new());
static RX_BUFFER: StaticCell<AlignedRx> = StaticCell::new(AlignedRx([0; RX_ALLOC_LEN]));
static TX_BUFFERS: StaticCell<[AlignedTx; 4]> = StaticCell::new([
    AlignedTx([0; TX_BUFFER_LEN]),
    AlignedTx([0; TX_BUFFER_LEN]),
    AlignedTx([0; TX_BUFFER_LEN]),
    AlignedTx([0; TX_BUFFER_LEN]),
]);

pub fn init() -> bool {
    let Some(io_base) = rtl8139_find() else {
        return false;
    };

    rtl_write8(io_base, RTL_REG_CONFIG1, 0x00);
    rtl_write8(io_base, RTL_REG_CR, RTL_CR_RESET);
    for _ in 0..100_000 {
        if rtl_read8(io_base, RTL_REG_CR) & RTL_CR_RESET == 0 {
            break;
        }
    }
    if rtl_read8(io_base, RTL_REG_CR) & RTL_CR_RESET != 0 {
        return false;
    }

    {
        let mut state = STATE.lock();
        state.io_base = io_base;
        state.tx_slot = 0;
        state.rx_offset = 0;
        state.arp_valid = false;
        state.ready = false;
    }

    let Some(rx_address) = dma_physical(RX_BUFFER.get().cast()) else {
        return false;
    };
    rtl_write32(io_base, RTL_REG_RBSTART, rx_address);
    rtl_write16(io_base, RTL_REG_IMR, 0x0000);
    rtl_write16(io_base, RTL_REG_ISR, 0xFFFF);
    rtl_write32(io_base, RTL_REG_TCR, 0x0300_0700);
    rtl_write32(io_base, RTL_REG_RCR, 0x0000_E78A);
    rtl_write8(io_base, RTL_REG_CR, RTL_CR_RX_ENABLE | RTL_CR_TX_ENABLE);
    rtl_write16(io_base, RTL_REG_CAPR, 0xFFF0);

    let mut mac = [0u8; 6];
    for (index, byte) in mac.iter_mut().enumerate() {
        *byte = rtl_read8(io_base, RTL_REG_IDR0 + index as u16);
    }

    let mut state = STATE.lock();
    state.mac = mac;
    state.ready = true;

    true
}

pub fn status() -> Option<NetStatus> {
    let state = STATE.lock();
    if !state.ready {
        return None;
    }
    Some(NetStatus {
        mac: state.mac,
        ip: state.ip,
        netmask: state.netmask,
        gateway: state.gateway,
        dns: state.dns,
        io_base: state.io_base,
        up: state.ready,
    })
}

pub fn format_ipv4(ip: [u8; 4], out: &mut InlineString<16>) {
    out.clear();
    let _ = write!(out, "{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]);
}

pub fn format_mac(mac: [u8; 6], out: &mut InlineString<24>) {
    out.clear();
    let _ = write!(
        out,
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    );
}

pub fn parse_ipv4(text: &str) -> Option<[u8; 4]> {
    crate::c_fastpath::parse_ipv4(text)
}

pub fn ping(target: [u8; 4]) -> Result<PingReply, &'static str> {
    let (src_mac, src_ip, gateway, mask, seq) = {
        let mut state = STATE.lock();
        if !state.ready {
            return Err("network is down");
        }
        let seq = state.icmp_seq;
        state.icmp_seq = state.icmp_seq.wrapping_add(1);
        (state.mac, state.ip, state.gateway, state.netmask, seq)
    };

    let next_hop = if same_subnet(src_ip, target, mask) {
        target
    } else {
        gateway
    };

    let dst_mac = arp_resolve(next_hop)?;
    let mut frame = [0u8; 98];
    let payload = b"rustix-net";
    let frame_len =
        build_icmp_echo_frame(&mut frame, src_mac, dst_mac, src_ip, target, seq, payload);

    send_frame(&frame[..frame_len])?;

    let mut recv = [0u8; MAX_FRAME_LEN];
    for _ in 0..200 {
        if let Some(len) = receive_frame(&mut recv) {
            if let Some(reply) = handle_frame_for_ping(&recv[..len], target, seq) {
                return Ok(reply);
            }
        } else {
            crate::arch::delay(200_000);
        }
    }

    Err("ping timeout")
}

fn handle_frame_for_ping(frame: &[u8], target: [u8; 4], seq: u16) -> Option<PingReply> {
    if frame.len() < 14 {
        return None;
    }

    let ethertype = read_u16_be(&frame[12..14]);
    if ethertype == ETHERTYPE_ARP {
        if let Some((sender_ip, sender_mac)) = parse_arp_reply(frame) {
            cache_arp(sender_ip, sender_mac);
        }
        return None;
    }

    if ethertype != ETHERTYPE_IPV4 || frame.len() < 14 + 20 {
        return None;
    }

    let ip = &frame[14..];
    let ihl = ((ip[0] & 0x0F) as usize) * 4;
    if ip.len() < ihl + 8 || ip[9] != 1 {
        return None;
    }

    let src_ip = [ip[12], ip[13], ip[14], ip[15]];
    if src_ip != target {
        return None;
    }

    let icmp = &ip[ihl..];
    if icmp[0] != ICMP_ECHO_REPLY {
        return None;
    }
    let reply_id = read_u16_be(&icmp[4..6]);
    let reply_seq = read_u16_be(&icmp[6..8]);
    if reply_id != ICMP_IDENTIFIER || reply_seq != seq {
        return None;
    }

    Some(PingReply {
        from: src_ip,
        seq: reply_seq,
        ttl: ip[8],
        bytes: icmp.len().saturating_sub(8),
    })
}

fn arp_resolve(target_ip: [u8; 4]) -> Result<[u8; 6], &'static str> {
    {
        let state = STATE.lock();
        if state.arp_valid && state.arp_ip == target_ip {
            return Ok(state.arp_mac);
        }
    }

    let (src_mac, src_ip) = {
        let state = STATE.lock();
        if !state.ready {
            return Err("network is down");
        }
        (state.mac, state.ip)
    };

    let mut frame = [0u8; 64];
    let frame_len = build_arp_request_frame(&mut frame, src_mac, src_ip, target_ip);
    send_frame(&frame[..frame_len])?;

    let mut recv = [0u8; MAX_FRAME_LEN];
    for _ in 0..200 {
        if let Some(len) = receive_frame(&mut recv) {
            if let Some((sender_ip, sender_mac)) = parse_arp_reply(&recv[..len]) {
                cache_arp(sender_ip, sender_mac);
                if sender_ip == target_ip {
                    return Ok(sender_mac);
                }
            }
        } else {
            crate::arch::delay(100_000);
        }
    }

    Err("arp timeout")
}

fn build_arp_request_frame(
    frame: &mut [u8; 64],
    src_mac: [u8; 6],
    src_ip: [u8; 4],
    target_ip: [u8; 4],
) -> usize {
    frame[..6].fill(0xFF);
    frame[6..12].copy_from_slice(&src_mac);
    write_u16_be(&mut frame[12..14], ETHERTYPE_ARP);
    write_u16_be(&mut frame[14..16], ARP_HTYPE_ETHERNET);
    write_u16_be(&mut frame[16..18], ARP_PTYPE_IPV4);
    frame[18] = 6;
    frame[19] = 4;
    write_u16_be(&mut frame[20..22], ARP_OP_REQUEST);
    frame[22..28].copy_from_slice(&src_mac);
    frame[28..32].copy_from_slice(&src_ip);
    frame[32..38].fill(0);
    frame[38..42].copy_from_slice(&target_ip);
    42.max(60)
}

fn build_icmp_echo_frame(
    frame: &mut [u8; 98],
    src_mac: [u8; 6],
    dst_mac: [u8; 6],
    src_ip: [u8; 4],
    dst_ip: [u8; 4],
    seq: u16,
    payload: &[u8],
) -> usize {
    frame[0..6].copy_from_slice(&dst_mac);
    frame[6..12].copy_from_slice(&src_mac);
    write_u16_be(&mut frame[12..14], ETHERTYPE_IPV4);

    let ip = &mut frame[14..34];
    ip.fill(0);
    ip[0] = 0x45;
    let total_len = (20 + 8 + payload.len()) as u16;
    write_u16_be(&mut ip[2..4], total_len);
    write_u16_be(&mut ip[4..6], seq);
    write_u16_be(&mut ip[6..8], 0);
    ip[8] = DEFAULT_TTL;
    ip[9] = 1;
    ip[12..16].copy_from_slice(&src_ip);
    ip[16..20].copy_from_slice(&dst_ip);
    let checksum = internet_checksum(ip);
    write_u16_be(&mut ip[10..12], checksum);

    let icmp_start = 34;
    let icmp_end = icmp_start + 8 + payload.len();
    let icmp = &mut frame[icmp_start..icmp_end];
    icmp.fill(0);
    icmp[0] = ICMP_ECHO_REQUEST;
    write_u16_be(&mut icmp[4..6], ICMP_IDENTIFIER);
    write_u16_be(&mut icmp[6..8], seq);
    icmp[8..].copy_from_slice(payload);
    let icmp_checksum = internet_checksum(icmp);
    write_u16_be(&mut icmp[2..4], icmp_checksum);

    icmp_end.max(60)
}

fn parse_arp_reply(frame: &[u8]) -> Option<([u8; 4], [u8; 6])> {
    if frame.len() < 42 {
        return None;
    }
    if read_u16_be(&frame[12..14]) != ETHERTYPE_ARP {
        return None;
    }
    if read_u16_be(&frame[20..22]) != ARP_OP_REPLY {
        return None;
    }

    let sender_mac = [
        frame[22], frame[23], frame[24], frame[25], frame[26], frame[27],
    ];
    let sender_ip = [frame[28], frame[29], frame[30], frame[31]];
    Some((sender_ip, sender_mac))
}

fn cache_arp(ip: [u8; 4], mac: [u8; 6]) {
    let mut state = STATE.lock();
    state.arp_ip = ip;
    state.arp_mac = mac;
    state.arp_valid = true;
}

fn same_subnet(a: [u8; 4], b: [u8; 4], mask: [u8; 4]) -> bool {
    for index in 0..4 {
        if (a[index] & mask[index]) != (b[index] & mask[index]) {
            return false;
        }
    }
    true
}

fn send_frame(frame: &[u8]) -> Result<(), &'static str> {
    let (io_base, slot) = {
        let state = STATE.lock();
        if !state.ready {
            return Err("network is down");
        }
        (state.io_base, state.tx_slot)
    };

    if frame.len() > TX_BUFFER_LEN {
        return Err("frame too large");
    }

    unsafe {
        let buffers = &mut *TX_BUFFERS.get();
        buffers[slot].0[..frame.len()].copy_from_slice(frame);
        compiler_fence(Ordering::Release);
        let Some(addr) = dma_physical(core::ptr::addr_of!(buffers[slot].0).cast()) else {
            return Err("DMA buffer is outside 32-bit physical memory");
        };
        rtl_write32(io_base, RTL_REG_TSAD0 + (slot as u16) * 4, addr);
        rtl_write32(
            io_base,
            RTL_REG_TSD0 + (slot as u16) * 4,
            frame.len() as u32,
        );
    }

    for _ in 0..100_000 {
        let status = rtl_read32(io_base, RTL_REG_TSD0 + (slot as u16) * 4);
        if status & RTL_TSD_TOK != 0 {
            STATE.lock().tx_slot = (slot + 1) % 4;
            return Ok(());
        }
        if status & RTL_TSD_TABT != 0 {
            return Err("rtl8139 transmit failed");
        }
    }

    Err("rtl8139 transmit timeout")
}

fn receive_frame(out: &mut [u8; MAX_FRAME_LEN]) -> Option<usize> {
    let io_base = {
        let state = STATE.lock();
        if !state.ready {
            return None;
        }
        state.io_base
    };

    let isr = rtl_read16(io_base, RTL_REG_ISR);
    if isr & (RTL_ISR_ROK | RTL_ISR_RX_ERR | RTL_ISR_RX_OVW) == 0
        && rtl_read8(io_base, RTL_REG_CR) & RTL_CR_BUFFER_EMPTY != 0
    {
        return None;
    }

    compiler_fence(Ordering::Acquire);
    let offset = STATE.lock().rx_offset;
    let status = rx_read_u16(offset);
    let packet_len = rx_read_u16(offset + 2) as usize;
    if packet_len < 4 || packet_len > MAX_FRAME_LEN + 4 || status & 0x0001 == 0 {
        rtl_write16(io_base, RTL_REG_ISR, 0xFFFF);
        STATE.lock().rx_offset = 0;
        rtl_write16(io_base, RTL_REG_CAPR, 0xFFF0);
        return None;
    }

    let payload_len = packet_len - 4;
    for index in 0..payload_len.min(out.len()) {
        out[index] = rx_read_u8(offset + 4 + index);
    }

    let mut next = offset + packet_len + 4;
    next = (next + 3) & !3;
    next %= RX_RING_LEN;

    STATE.lock().rx_offset = next;
    rtl_write16(io_base, RTL_REG_CAPR, (next as u16).wrapping_sub(0x10));
    rtl_write16(
        io_base,
        RTL_REG_ISR,
        RTL_ISR_ROK | RTL_ISR_RX_ERR | RTL_ISR_RX_OVW,
    );
    Some(payload_len.min(out.len()))
}

fn rx_read_u8(offset: usize) -> u8 {
    let index = offset % RX_RING_LEN;
    unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*RX_BUFFER.get()).0[index])) }
}

fn dma_physical(pointer: *const u8) -> Option<u32> {
    let virtual_address = pointer as u64;
    let physical_address = if virtual_address >= crate::mm::address::KERNEL_BASE {
        virtual_address.checked_sub(crate::mm::address::KERNEL_BASE)?
    } else {
        virtual_address
    };
    u32::try_from(physical_address).ok()
}

fn rx_read_u16(offset: usize) -> u16 {
    let lo = rx_read_u8(offset) as u16;
    let hi = rx_read_u8(offset + 1) as u16;
    lo | (hi << 8)
}

fn rtl8139_find() -> Option<u16> {
    for bus in 0u16..=255 {
        for slot in 0u16..32 {
            for function in 0u16..8 {
                let id = pci_read32(bus as u8, slot as u8, function as u8, 0x00);
                if id == 0xFFFF_FFFF {
                    continue;
                }
                let vendor = (id & 0xFFFF) as u16;
                let device = (id >> 16) as u16;
                if vendor != RTL8139_VENDOR_ID || device != RTL8139_DEVICE_ID {
                    continue;
                }

                let mut command_status = pci_read32(bus as u8, slot as u8, function as u8, 0x04);
                command_status |= 0x0005;
                pci_write32(bus as u8, slot as u8, function as u8, 0x04, command_status);

                let bar0 = pci_read32(bus as u8, slot as u8, function as u8, 0x10);
                if bar0 & 0x1 == 0 {
                    return None;
                }
                return Some((bar0 & !0x3) as u16);
            }
        }
    }
    None
}

fn pci_config_address(bus: u8, slot: u8, function: u8, offset: u8) -> u32 {
    0x8000_0000
        | ((bus as u32) << 16)
        | ((slot as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC)
}

fn pci_read32(bus: u8, slot: u8, function: u8, offset: u8) -> u32 {
    crate::arch::port_out32(0xCF8, pci_config_address(bus, slot, function, offset));
    crate::arch::port_in32(0xCFC)
}

fn pci_write32(bus: u8, slot: u8, function: u8, offset: u8, value: u32) {
    crate::arch::port_out32(0xCF8, pci_config_address(bus, slot, function, offset));
    crate::arch::port_out32(0xCFC, value);
}

fn rtl_read8(io_base: u16, reg: u16) -> u8 {
    crate::arch::port_in8(io_base + reg)
}

fn rtl_read16(io_base: u16, reg: u16) -> u16 {
    crate::arch::port_in16(io_base + reg)
}

fn rtl_read32(io_base: u16, reg: u16) -> u32 {
    crate::arch::port_in32(io_base + reg)
}

fn rtl_write8(io_base: u16, reg: u16, value: u8) {
    crate::arch::port_out8(io_base + reg, value);
}

fn rtl_write16(io_base: u16, reg: u16, value: u16) {
    crate::arch::port_out16(io_base + reg, value);
}

fn rtl_write32(io_base: u16, reg: u16, value: u32) {
    crate::arch::port_out32(io_base + reg, value);
}

fn read_u16_be(slice: &[u8]) -> u16 {
    ((slice[0] as u16) << 8) | (slice[1] as u16)
}

fn write_u16_be(slice: &mut [u8], value: u16) {
    slice[0] = (value >> 8) as u8;
    slice[1] = value as u8;
}

fn internet_checksum(data: &[u8]) -> u16 {
    crate::c_fastpath::internet_checksum(data)
}

// ---------------------------------------------------------------------------
// TCP over IPv4 (minimal client stack: connect / send / recv / close)
// ---------------------------------------------------------------------------

const PROTO_TCP: u8 = 6;
const PROTO_ICMP: u8 = 1;
const TCP_FIN: u16 = 0x0001;
const TCP_SYN: u16 = 0x0002;
const TCP_RST: u16 = 0x0004;
const TCP_PSH: u16 = 0x0008;
const TCP_ACK: u16 = 0x0010;
const TCP_HEADER_LEN: usize = 20;
const IP_HEADER_LEN: usize = 20;
const ETH_HEADER_LEN: usize = 14;
pub const TCP_MSS: usize = 1400;
const TCP_RX_BUFFER: usize = 16384;
const TCP_MAX_CONN: usize = 16;
const EPHEMERAL_BASE: u16 = 49152;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TcpState {
    SynSent,
    Established,
    CloseWait,
    FinSent,
    Closed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NetErr {
    Again,
    Down,
    Exhausted,
    Refused,
    TimedOut,
    Reset,
}

struct TcpConn {
    active: bool,
    state: TcpState,
    rip: [u8; 4],
    rport: u16,
    lport: u16,
    snd_una: u32,
    snd_nxt: u32,
    rcv_nxt: u32,
    peer_window: u16,
    pending: [u8; TCP_MSS],
    pending_len: usize,
    pending_seq: u32,
    pending_age: usize,
    fin_requested: bool,
    fin_queued: bool,
    eof: bool,
    reset: bool,
    rx: [u8; TCP_RX_BUFFER],
    rx_len: usize,
}

impl TcpConn {
    const fn new() -> Self {
        Self {
            active: false,
            state: TcpState::Closed,
            rip: [0; 4],
            rport: 0,
            lport: 0,
            snd_una: 0,
            snd_nxt: 0,
            rcv_nxt: 0,
            peer_window: 0,
            pending: [0; TCP_MSS],
            pending_len: 0,
            pending_seq: 0,
            pending_age: 0,
            fin_requested: false,
            fin_queued: false,
            eof: false,
            reset: false,
            rx: [0; TCP_RX_BUFFER],
            rx_len: 0,
        }
    }
}

struct TcpTable {
    conns: [TcpConn; TCP_MAX_CONN],
    next_port: u16,
}

impl TcpTable {
    const fn new() -> Self {
        Self {
            conns: [const { TcpConn::new() }; TCP_MAX_CONN],
            next_port: EPHEMERAL_BASE,
        }
    }
}

static TCP: SpinMutex<TcpTable> = SpinMutex::new(TcpTable::new());

static TCP_SCRATCH: StaticCell<[u8; 12 + TCP_MSS + 64]> =
    StaticCell::new([0u8; 12 + TCP_MSS + 64]);

fn ticks_now() -> u64 {
    crate::kernel::scheduler::timer_ticks()
}

/// Drain pending RX frames, dispatching ARP/ICMP/TCP. Returns number handled.
pub fn pump() -> usize {
    let mut handled = 0usize;
    let mut frame = [0u8; MAX_FRAME_LEN];
    while handled < 64 {
        let Some(len) = receive_frame(&mut frame) else {
            break;
        };
        handled += 1;
        dispatch_frame(&frame[..len]);
    }
    handled
}

fn dispatch_frame(frame: &[u8]) {
    if frame.len() < ETH_HEADER_LEN {
        return;
    }
    match read_u16_be(&frame[12..14]) {
        ETHERTYPE_ARP => handle_arp_frame(frame),
        ETHERTYPE_IPV4 => handle_ipv4_frame(frame),
        _ => {}
    }
}

fn handle_arp_frame(frame: &[u8]) {
    if let Some((sender_ip, sender_mac)) = parse_arp_reply(frame) {
        cache_arp(sender_ip, sender_mac);
        return;
    }
    // Answer requests for our own address (cheap, keeps some stacks happy).
    if frame.len() < 42 || read_u16_be(&frame[20..22]) != ARP_OP_REQUEST {
        return;
    }
    let target_ip = [frame[38], frame[39], frame[40], frame[41]];
    let (src_mac, src_ip, io_base, ready) = {
        let state = STATE.lock();
        (
            state.mac,
            state.ip,
            state.io_base,
            state.ready && target_ip == state.ip,
        )
    };
    if !ready {
        return;
    }
    let mut reply = [0u8; 64];
    reply[..6].copy_from_slice(&frame[22..28]);
    reply[6..12].copy_from_slice(&src_mac);
    write_u16_be(&mut reply[12..14], ETHERTYPE_ARP);
    write_u16_be(&mut reply[14..16], ARP_HTYPE_ETHERNET);
    write_u16_be(&mut reply[16..18], ARP_PTYPE_IPV4);
    reply[18] = 6;
    reply[19] = 4;
    write_u16_be(&mut reply[20..22], ARP_OP_REPLY);
    reply[22..28].copy_from_slice(&src_mac);
    reply[28..32].copy_from_slice(&src_ip);
    reply[32..38].copy_from_slice(&frame[22..28]);
    reply[38..42].copy_from_slice(&target_ip);
    let _ = send_frame(&reply[..42.max(60)]);
    let _ = io_base;
}

struct IpHeader<'a> {
    src: [u8; 4],
    dst: [u8; 4],
    protocol: u8,
    payload: &'a [u8],
}

fn handle_ipv4_frame(frame: &[u8]) {
    if frame.len() < ETH_HEADER_LEN + IP_HEADER_LEN {
        return;
    }
    let ip = &frame[ETH_HEADER_LEN..];
    let ihl = ((ip[0] & 0x0F) as usize) * 4;
    if ihl < IP_HEADER_LEN || ip.len() < ihl {
        return;
    }
    let total_len = read_u16_be(&ip[2..4]) as usize;
    let end = (ihl + total_len.saturating_sub(ihl)).min(ip.len());
    if end <= ihl {
        return;
    }
    let header = IpHeader {
        src: [ip[12], ip[13], ip[14], ip[15]],
        dst: [ip[16], ip[17], ip[18], ip[19]],
        protocol: ip[9],
        payload: &ip[ihl..end],
    };
    if header.dst != local_ip() {
        return;
    }
    match header.protocol {
        PROTO_TCP => tcp_input(&header),
        _ => {}
    }
}

fn local_ip() -> [u8; 4] {
    STATE.lock().ip
}

fn route_next_hop(target: [u8; 4]) -> [u8; 4] {
    let state = STATE.lock();
    if same_subnet(state.ip, target, state.netmask) {
        target
    } else {
        state.gateway
    }
}

#[allow(clippy::too_many_arguments)]
fn transmit_tcp(
    rip: [u8; 4],
    rport: u16,
    lport: u16,
    seq: u32,
    ack: u32,
    flags: u16,
    window: u16,
    payload: &[u8],
) -> Result<(), &'static str> {
    let (src_mac, src_ip) = {
        let state = STATE.lock();
        if !state.ready || payload.len() > TCP_MSS {
            return Err("network is down");
        }
        (state.mac, state.ip)
    };
    let dst_mac = arp_resolve(route_next_hop(rip))?;

    let mut frame = [0u8; ETH_HEADER_LEN + IP_HEADER_LEN + TCP_HEADER_LEN + TCP_MSS];
    let total_ip = (IP_HEADER_LEN + TCP_HEADER_LEN + payload.len()) as u16;

    frame[..6].copy_from_slice(&dst_mac);
    frame[6..12].copy_from_slice(&src_mac);
    write_u16_be(&mut frame[12..14], ETHERTYPE_IPV4);

    let ip = &mut frame[ETH_HEADER_LEN..ETH_HEADER_LEN + IP_HEADER_LEN];
    ip[0] = 0x45;
    write_u16_be(&mut ip[2..4], total_ip);
    ip[4] = (lport as u8) ^ (seq as u8);
    ip[6] = 0x40; // don't fragment
    ip[8] = DEFAULT_TTL;
    ip[9] = PROTO_TCP;
    ip[12..16].copy_from_slice(&src_ip);
    ip[16..20].copy_from_slice(&rip);
    let ip_cksum = internet_checksum(ip);
    write_u16_be(&mut ip[10..12], ip_cksum);

    let tcp_start = ETH_HEADER_LEN + IP_HEADER_LEN;
    let tcp_end = tcp_start + TCP_HEADER_LEN + payload.len();
    {
        let tcp = &mut frame[tcp_start..tcp_end];
        write_u16_be(&mut tcp[0..2], lport);
        write_u16_be(&mut tcp[2..4], rport);
        tcp[4..8].copy_from_slice(&seq.to_be_bytes());
        tcp[8..12].copy_from_slice(&ack.to_be_bytes());
        tcp[12] = (TCP_HEADER_LEN as u8) << 4;
        tcp[13] = (flags & 0xff) as u8;
        write_u16_be(&mut tcp[14..16], window);
        write_u16_be(&mut tcp[16..18], 0);
        tcp[TCP_HEADER_LEN..].copy_from_slice(payload);
    }

    let mut pseudo_src = [0u8; 12];
    pseudo_src[0..4].copy_from_slice(&src_ip);
    pseudo_src[4..8].copy_from_slice(&rip);
    pseudo_src[9] = PROTO_TCP;
    let tcp_len = (TCP_HEADER_LEN + payload.len()) as u16;
    write_u16_be(&mut pseudo_src[10..12], tcp_len);

    let segment_len = TCP_HEADER_LEN + payload.len();
    let checksum = {
        let scratch = unsafe { &mut *TCP_SCRATCH.get() };
        scratch[..12].copy_from_slice(&pseudo_src);
        scratch[12..12 + segment_len]
            .copy_from_slice(&frame[tcp_start..tcp_start + segment_len]);
        internet_checksum(&scratch[..12 + segment_len])
    };
    write_u16_be(&mut frame[tcp_start + 16..tcp_start + 18], checksum);

    send_frame(&frame[..tcp_end.max(60)])
}

fn tcp_input(header: &IpHeader) {
    let data = header.payload;
    if data.len() < TCP_HEADER_LEN {
        return;
    }
    let src_port = read_u16_be(&data[0..2]);
    let dst_port = read_u16_be(&data[2..4]);
    let seq = u32::from_be_bytes(data[4..8].try_into().unwrap());
    let ack = u32::from_be_bytes(data[8..12].try_into().unwrap());
    let data_offset = ((data[12] >> 4) as usize) * 4;
    if data_offset < TCP_HEADER_LEN || data.len() < data_offset {
        return;
    }
    let flags = u16::from_be_bytes([data[13], data[13]]); // low byte holds flags here
    let flags = (flags & 0xff00) | data[13] as u16;
    let window = read_u16_be(&data[14..16]);
    let payload = &data[data_offset..];

    let mut table = TCP.lock();
    let Some(index) = table.conns.iter().position(|conn| {
        conn.active
            && conn.lport == dst_port
            && conn.rport == src_port
            && conn.rip == header.src
    }) else {
        return;
    };

    let advance_ack = |conn: &mut TcpConn| {
        if flags & TCP_ACK != 0 && (ack.wrapping_sub(conn.snd_una) as i32) > 0 {
            conn.snd_una = ack.min(conn.snd_nxt);
        }
        if conn.pending_len != 0 {
            let pending_end = conn.pending_seq.wrapping_add(conn.pending_len as u32);
            if (pending_end.wrapping_sub(conn.snd_una) as i32) <= 0 {
                conn.pending_len = 0;
                conn.pending_age = 0;
            }
        }
        if conn.fin_queued && conn.fin_requested {
            let fin_seq = conn.snd_nxt.wrapping_sub(1);
            if (fin_seq.wrapping_sub(conn.snd_una) as i32) < 0
                || fin_seq == conn.snd_una.wrapping_sub(1)
            {
                // FIN acknowledged once snd_una passes the FIN sequence number
                if ((conn.snd_nxt.wrapping_sub(1)).wrapping_sub(conn.snd_una) as i32) < 0 {
                    conn.state = TcpState::Closed;
                    conn.active = false;
                }
            }
        }
        conn.peer_window = window;
    };

    let conn = &mut table.conns[index];

    if flags & TCP_RST != 0 {
        if conn.state == TcpState::SynSent {
            conn.reset = true;
        } else if (ack.wrapping_sub(conn.snd_nxt) as i32) <= 0 {
            conn.reset = true;
            conn.eof = true;
        }
        if conn.reset {
            conn.state = TcpState::Closed;
        }
        return;
    }

    match conn.state {
        TcpState::SynSent => {
            if flags & TCP_SYN != 0 && flags & TCP_ACK != 0 {
                conn.rcv_nxt = seq.wrapping_add(1);
                conn.snd_una = ack;
                conn.snd_nxt = ack;
                conn.peer_window = window;
                conn.state = TcpState::Established;
                let (rip, rport, lport, rcv_nxt, snd_nxt) = (
                    conn.rip,
                    conn.rport,
                    conn.lport,
                    conn.rcv_nxt,
                    conn.snd_nxt,
                );
                drop(table);
                let _ = transmit_tcp(rip, rport, lport, snd_nxt, rcv_nxt, TCP_ACK, 8192, &[]);
            }
        }
        TcpState::Established | TcpState::CloseWait | TcpState::FinSent => {
            advance_ack(conn);
            let seg_end = seq.wrapping_add(payload.len() as u32);
            let new_data = (seg_end.wrapping_sub(conn.rcv_nxt) as i32) > 0
                && (seq.wrapping_sub(conn.rcv_nxt) as i32) <= 0;

            if payload.len() != 0 {
                if new_data && !conn.eof {
                    let offset = (conn.rcv_nxt.wrapping_sub(seq) as i32).unsigned_abs() as usize;
                    let usable = payload.len().saturating_sub(offset);
                    let space = conn.rx.len() - conn.rx_len;
                    let take = usable.min(space);
                    if take != 0 {
                        let slice = &payload[offset..offset + take];
                        conn.rx[conn.rx_len..conn.rx_len + take].copy_from_slice(slice);
                        conn.rx_len += take;
                    }
                    conn.rcv_nxt = conn.rcv_nxt.wrapping_add(usable as u32);
                }
                let (rip, rport, lport, rcv_nxt, snd_nxt) =
                    (conn.rip, conn.rport, conn.lport, conn.rcv_nxt, conn.snd_nxt);
                drop(table);
                let _ = transmit_tcp(rip, rport, lport, snd_nxt, rcv_nxt, TCP_ACK, 8192, &[]);
                return;
            }

            if flags & TCP_FIN != 0 && !conn.eof {
                conn.rcv_nxt = conn.rcv_nxt.wrapping_add(1);
                conn.eof = true;
                if conn.state == TcpState::Established {
                    conn.state = TcpState::CloseWait;
                }
                let (rip, rport, lport, rcv_nxt, snd_nxt) =
                    (conn.rip, conn.rport, conn.lport, conn.rcv_nxt, conn.snd_nxt);
                drop(table);
                let _ = transmit_tcp(rip, rport, lport, snd_nxt, rcv_nxt, TCP_ACK, 8192, &[]);
                return;
            }

            if flags & TCP_ACK != 0 {
                let (rip, rport, lport) = (conn.rip, conn.rport, conn.lport);
                let snd_nxt = conn.snd_nxt;
                let rcv_nxt = conn.rcv_nxt;
                drop(table);
                let _ = transmit_tcp(rip, rport, lport, snd_nxt, rcv_nxt, TCP_ACK, 8192, &[]);
            }
        }
        TcpState::Closed => {}
    }
}

fn retransmit_if_due(conn: &mut TcpConn) {
    if conn.pending_len == 0 || conn.pending_age < 40 {
        if conn.pending_len != 0 {
            conn.pending_age += 1;
        }
        return;
    }
    conn.pending_age += 1;
    if conn.pending_age > 400 {
        conn.reset = true;
        conn.state = TcpState::Closed;
        conn.pending_len = 0;
        return;
    }
    let seq = conn.pending_seq;
    let (rip, rport, lport, rcv_nxt) = (conn.rip, conn.rport, conn.lport, conn.rcv_nxt);
    let payload_len = conn.pending_len;
    let result = transmit_tcp(
        rip,
        rport,
        lport,
        seq,
        rcv_nxt,
        TCP_ACK | TCP_PSH,
        8192,
        &conn.pending[..payload_len],
    );
    if result.is_err() {
        conn.pending_age = 0;
    }
}

pub fn tcp_connect(dst: [u8; 4], dst_port: u16, timeout_ms: u64) -> Result<usize, NetErr> {
    let mut table = TCP.lock();
    let Some(slot) = table.conns.iter().position(|conn| !conn.active) else {
        return Err(NetErr::Exhausted);
    };
    let lport = loop {
        let candidate = table.next_port;
        table.next_port = table.next_port.wrapping_add(1).max(EPHEMERAL_BASE);
        if !table
            .conns
            .iter()
            .any(|conn| conn.active && conn.lport == candidate)
        {
            break candidate;
        }
    };
    let isn = 0x1F4_C0DE_u32.wrapping_add(ticks_now() as u32).wrapping_mul(2654435761);
    let conn = &mut table.conns[slot];
    *conn = TcpConn::new();
    conn.active = true;
    conn.state = TcpState::SynSent;
    conn.rip = dst;
    conn.rport = dst_port;
    conn.lport = lport;
    conn.snd_una = isn;
    conn.snd_nxt = isn.wrapping_add(1);
    conn.rcv_nxt = 0;
    drop(table);

    let syn_result = transmit_tcp(
        dst,
        dst_port,
        lport,
        isn,
        0,
        TCP_SYN,
        8192,
        &[],
    );

    let deadline = ticks_now() + timeout_ms;
    loop {
        if syn_result.is_err() {
            let mut table = TCP.lock();
            table.conns[slot].active = false;
            return Err(NetErr::Down);
        }
        pump();
        {
            let table = TCP.lock();
            let conn = &table.conns[slot];
            match conn.state {
                TcpState::Established => return Ok(slot),
                TcpState::Closed if conn.reset => return Err(NetErr::Refused),
                _ => {}
            }
        }
        if ticks_now() >= deadline {
            let mut table = TCP.lock();
            table.conns[slot].active = false;
            return Err(NetErr::TimedOut);
        }
        crate::arch::delay(30_000);
    }
}

pub fn tcp_send(conn_index: usize, data: &[u8]) -> Result<usize, NetErr> {
    let deadline = ticks_now() + 5000;
    let mut sent_total = 0usize;
    while sent_total < data.len() {
        let chunk_limit = {
            let mut table = TCP.lock();
            let conn = &mut table.conns[conn_index];
            if !conn.active || conn.reset || conn.state == TcpState::Closed {
                return if sent_total != 0 {
                    Ok(sent_total)
                } else {
                    Err(NetErr::Reset)
                };
            }
            if conn.fin_requested {
                return Err(NetErr::Reset);
            }
            retransmit_if_due(conn);
            let outstanding = conn.snd_nxt.wrapping_sub(conn.snd_una);
            let window_credit = (conn.peer_window as u32).saturating_sub(outstanding);
            if conn.pending_len != 0 || window_credit == 0 {
                None
            } else {
                Some(window_credit as usize)
            }
        };
        let Some(limit) = chunk_limit else {
            if ticks_now() >= deadline {
                return if sent_total != 0 {
                    Ok(sent_total)
                } else {
                    Err(NetErr::TimedOut)
                };
            }
            pump();
            crate::arch::delay(30_000);
            continue;
        };
        let start = sent_total;
        let chunk_len = (data.len() - start).min(TCP_MSS).min(limit);
        let chunk = &data[start..start + chunk_len];
        let (seq, rcv_nxt, rip, rport, lport) = {
            let mut table = TCP.lock();
            let conn = &mut table.conns[conn_index];
            let seq = conn.snd_nxt;
            conn.pending[..chunk_len].copy_from_slice(chunk);
            conn.pending_len = chunk_len;
            conn.pending_seq = seq;
            conn.pending_age = 0;
            conn.snd_nxt = seq.wrapping_add(chunk_len as u32);
            (
                seq,
                conn.rcv_nxt,
                conn.rip,
                conn.rport,
                conn.lport,
            )
        };
        transmit_tcp(rip, rport, lport, seq, rcv_nxt, TCP_ACK | TCP_PSH, 8192, chunk)
            .map_err(|_| NetErr::Down)?;
        sent_total += chunk_len;
    }
    Ok(sent_total)
}

/// Non-blocking receive. Ok(0) means EOF, Err(Again) nothing yet.
pub fn tcp_recv(conn_index: usize, out: &mut [u8]) -> Result<usize, NetErr> {
    let mut table = TCP.lock();
    let conn = &mut table.conns[conn_index];
    if !conn.active {
        return Err(NetErr::Reset);
    }
    if conn.rx_len != 0 {
        let take = conn.rx_len.min(out.len());
        out[..take].copy_from_slice(&conn.rx[..take]);
        conn.rx.copy_within(take..conn.rx_len, 0);
        conn.rx_len -= take;
        return Ok(take);
    }
    if conn.eof || conn.reset || conn.state == TcpState::Closed {
        return Ok(0);
    }
    Err(NetErr::Again)
}

pub fn tcp_connected(conn_index: usize) -> bool {
    let table = TCP.lock();
    matches!(
        table.conns[conn_index].state,
        TcpState::Established | TcpState::CloseWait | TcpState::FinSent
    ) && !table.conns[conn_index].reset
}

pub fn tcp_readable(conn_index: usize) -> bool {
    let table = TCP.lock();
    let conn = &table.conns[conn_index];
    conn.rx_len != 0 || conn.eof || conn.reset || !conn.active
}

pub fn tcp_close(conn_index: usize) -> Result<(), NetErr> {
    let (do_fin, rip, rport, lport, seq, rcv_nxt) = {
        let mut table = TCP.lock();
        let conn = &mut table.conns[conn_index];
        if !conn.active {
            return Ok(());
        }
        conn.fin_requested = true;
        if conn.state == TcpState::Established {
            let seq = conn.snd_nxt;
            conn.snd_nxt = conn.snd_nxt.wrapping_add(1);
            conn.fin_queued = true;
            conn.state = TcpState::FinSent;
            (
                true,
                conn.rip,
                conn.rport,
                conn.lport,
                seq,
                conn.rcv_nxt,
            )
        } else if conn.state == TcpState::CloseWait {
            let seq = conn.snd_nxt;
            conn.snd_nxt = conn.snd_nxt.wrapping_add(1);
            conn.fin_queued = true;
            conn.state = TcpState::FinSent;
            (
                true,
                conn.rip,
                conn.rport,
                conn.lport,
                seq,
                conn.rcv_nxt,
            )
        } else {
            (false, conn.rip, conn.rport, conn.lport, 0, conn.rcv_nxt)
        }
    };
    if do_fin {
        let _ = transmit_tcp(rip, rport, lport, seq, rcv_nxt, TCP_ACK | TCP_FIN, 8192, &[]);
        let deadline = ticks_now() + 2000;
        while ticks_now() < deadline {
            pump();
            {
                let table = TCP.lock();
                if !table.conns[conn_index].active || !table.conns[conn_index].fin_queued {
                    return Ok(());
                }
            }
            crate::arch::delay(30_000);
        }
    }
    let mut table = TCP.lock();
    table.conns[conn_index] = TcpConn::new();
    Ok(())
}
