use core::cmp::min;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::sync::SpinMutex;

pub const SECTOR_SIZE: usize = 512;
pub const MAX_SECTORS: usize = 256;
pub const MAX_IO_BYTES: usize = SECTOR_SIZE * MAX_SECTORS;
pub const ASYNC_QUEUE_DEPTH: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockError {
    Unaligned,
    OutOfRange,
    ReadOnly,
    QueueFull,
    InvalidToken,
    Busy,
    Unsupported,
    HardwareUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockOperation {
    Read,
    Write,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockRequest {
    pub operation: BlockOperation,
    pub lba: u64,
    pub sectors: u16,
    pub buffer: *mut u8,
}

impl BlockRequest {
    pub fn read(lba: u64, output: &mut [u8]) -> Result<Self, BlockError> {
        Self::new(BlockOperation::Read, lba, output)
    }

    pub fn write(lba: u64, input: &[u8]) -> Result<Self, BlockError> {
        Self::new(BlockOperation::Write, lba, input)
    }

    fn new(operation: BlockOperation, lba: u64, buffer: &[u8]) -> Result<Self, BlockError> {
        if buffer.is_empty() || buffer.len() % SECTOR_SIZE != 0 {
            return Err(BlockError::Unaligned);
        }
        let sectors = buffer.len() / SECTOR_SIZE;
        if sectors > u16::MAX as usize {
            return Err(BlockError::OutOfRange);
        }
        Ok(Self {
            operation,
            lba,
            sectors: sectors as u16,
            buffer: buffer.as_ptr() as *mut u8,
        })
    }

    pub const fn bytes(self) -> usize {
        self.sectors as usize * SECTOR_SIZE
    }
}

pub trait BlockDevice {
    fn sector_count(&self) -> u64;
    fn read_sectors(&mut self, lba: u64, output: &mut [u8]) -> Result<(), BlockError>;
    fn write_sectors(&mut self, lba: u64, input: &[u8]) -> Result<(), BlockError>;
    fn flush(&mut self) -> Result<(), BlockError> {
        Ok(())
    }

    fn sector_size(&self) -> usize {
        SECTOR_SIZE
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IoToken(u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IoState {
    Pending,
    Complete,
    Failed(BlockError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IoCompletion {
    pub token: IoToken,
    pub state: IoState,
    pub sectors: u16,
}

#[derive(Clone, Copy)]
struct QueuedRequest {
    used: bool,
    token: IoToken,
    request: BlockRequest,
    state: IoState,
}

impl QueuedRequest {
    const fn empty() -> Self {
        Self {
            used: false,
            token: IoToken(0),
            request: BlockRequest {
                operation: BlockOperation::Read,
                lba: 0,
                sectors: 0,
                buffer: core::ptr::null_mut(),
            },
            state: IoState::Pending,
        }
    }
}

pub struct AsyncBlockQueue {
    next_token: u16,
    requests: [QueuedRequest; ASYNC_QUEUE_DEPTH],
}

impl AsyncBlockQueue {
    pub const fn new() -> Self {
        Self {
            next_token: 1,
            requests: [QueuedRequest::empty(); ASYNC_QUEUE_DEPTH],
        }
    }

    pub fn submit(&mut self, request: BlockRequest) -> Result<IoToken, BlockError> {
        let slot = self
            .requests
            .iter_mut()
            .find(|request| !request.used)
            .ok_or(BlockError::QueueFull)?;
        let token = IoToken(self.next_token.max(1));
        self.next_token = self.next_token.wrapping_add(1).max(1);
        *slot = QueuedRequest {
            used: true,
            token,
            request,
            state: IoState::Pending,
        };
        Ok(token)
    }

    pub fn poll<D: BlockDevice>(&mut self, device: &mut D) -> Option<IoCompletion> {
        let request = self
            .requests
            .iter_mut()
            .find(|request| request.used && request.state == IoState::Pending)?;
        let result = unsafe {
            let buffer =
                core::slice::from_raw_parts_mut(request.request.buffer, request.request.bytes());
            match request.request.operation {
                BlockOperation::Read => device.read_sectors(request.request.lba, buffer),
                BlockOperation::Write => device.write_sectors(request.request.lba, buffer),
            }
        };
        request.state = match result {
            Ok(()) => IoState::Complete,
            Err(error) => IoState::Failed(error),
        };
        Some(IoCompletion {
            token: request.token,
            state: request.state,
            sectors: request.request.sectors,
        })
    }

    pub fn reap(&mut self, token: IoToken) -> Result<IoState, BlockError> {
        let request = self
            .requests
            .iter_mut()
            .find(|request| request.used && request.token == token)
            .ok_or(BlockError::InvalidToken)?;
        if request.state == IoState::Pending {
            return Ok(IoState::Pending);
        }
        let state = request.state;
        request.used = false;
        Ok(state)
    }
}

pub struct MemoryBlockDevice<const N: usize> {
    data: [u8; N],
    read_only: bool,
}

impl<const N: usize> MemoryBlockDevice<N> {
    pub const fn new() -> Self {
        Self {
            data: [0; N],
            read_only: false,
        }
    }

    pub const fn from_bytes(data: [u8; N]) -> Self {
        Self {
            data,
            read_only: false,
        }
    }

    pub fn set_read_only(&mut self, read_only: bool) {
        self.read_only = read_only;
    }

    pub fn bytes(&self) -> &[u8; N] {
        &self.data
    }

    pub fn bytes_mut(&mut self) -> &mut [u8; N] {
        &mut self.data
    }
}

impl<const N: usize> BlockDevice for MemoryBlockDevice<N> {
    fn sector_count(&self) -> u64 {
        (N / SECTOR_SIZE) as u64
    }

    fn read_sectors(&mut self, lba: u64, output: &mut [u8]) -> Result<(), BlockError> {
        validate_request(self.sector_count(), lba, output.len())?;
        let offset = lba as usize * SECTOR_SIZE;
        output.copy_from_slice(&self.data[offset..offset + output.len()]);
        Ok(())
    }

    fn write_sectors(&mut self, lba: u64, input: &[u8]) -> Result<(), BlockError> {
        if self.read_only {
            return Err(BlockError::ReadOnly);
        }
        validate_request(self.sector_count(), lba, input.len())?;
        let offset = lba as usize * SECTOR_SIZE;
        self.data[offset..offset + input.len()].copy_from_slice(input);
        Ok(())
    }
}

fn validate_request(sector_count: u64, lba: u64, bytes: usize) -> Result<(), BlockError> {
    if bytes == 0 || bytes % SECTOR_SIZE != 0 {
        return Err(BlockError::Unaligned);
    }
    let sectors = (bytes / SECTOR_SIZE) as u64;
    if lba.checked_add(sectors).is_none() || lba + sectors > sector_count {
        return Err(BlockError::OutOfRange);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AtaPio {
    pub io_base: u16,
    pub control_base: u16,
    pub drive: u8,
    pub present: bool,
    pub sectors: u64,
}

impl AtaPio {
    pub const fn primary_master() -> Self {
        Self {
            io_base: 0x1f0,
            control_base: 0x3f6,
            drive: 0,
            present: false,
            sectors: 0,
        }
    }

    pub fn probe(&mut self) -> bool {
        #[cfg(target_arch = "x86_64")]
        {
            let status = crate::arch::port_in8(self.io_base + 7);
            if status != 0xff && status != 0 {
                let mut identify = [0u16; 256];
                self.present = self.identify(&mut identify).is_ok();
                if self.present {
                    self.sectors = u64::from(identify[60]) | (u64::from(identify[61]) << 16);
                    if identify[83] & (1 << 10) != 0 {
                        self.sectors = u64::from(identify[100])
                            | (u64::from(identify[101]) << 16)
                            | (u64::from(identify[102]) << 32)
                            | (u64::from(identify[103]) << 48);
                    }
                }
            }
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            self.present = false;
        }
        self.present
    }

    pub fn identify(&self, output: &mut [u16; 256]) -> Result<(), BlockError> {
        #[cfg(target_arch = "x86_64")]
        {
            crate::arch::port_out8(self.io_base + 6, 0xe0 | (self.drive << 4));
            crate::arch::port_out8(self.io_base + 2, 0);
            crate::arch::port_out8(self.io_base + 3, 0);
            crate::arch::port_out8(self.io_base + 4, 0);
            crate::arch::port_out8(self.io_base + 5, 0);
            crate::arch::port_out8(self.io_base + 7, 0xec);
            if !ata_wait(self.io_base, true) {
                return Err(BlockError::HardwareUnavailable);
            }
            for word in output.iter_mut() {
                *word = crate::arch::port_in16(self.io_base);
            }
            Ok(())
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = output;
            Err(BlockError::Unsupported)
        }
    }
}

impl BlockDevice for AtaPio {
    fn sector_count(&self) -> u64 {
        self.sectors
    }

    fn read_sectors(&mut self, lba: u64, output: &mut [u8]) -> Result<(), BlockError> {
        validate_request(self.sector_count(), lba, output.len())?;
        #[cfg(target_arch = "x86_64")]
        {
            for sector in 0..output.len() / SECTOR_SIZE {
                let address = lba + sector as u64;
                ata_select(self, address, 0x20)?;
                let destination = &mut output[sector * SECTOR_SIZE..(sector + 1) * SECTOR_SIZE];
                for word in 0..256 {
                    let value = crate::arch::port_in16(self.io_base);
                    destination[word * 2..word * 2 + 2].copy_from_slice(&value.to_le_bytes());
                }
            }
            Ok(())
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = (lba, output);
            Err(BlockError::Unsupported)
        }
    }

    fn write_sectors(&mut self, lba: u64, input: &[u8]) -> Result<(), BlockError> {
        validate_request(self.sector_count(), lba, input.len())?;
        #[cfg(target_arch = "x86_64")]
        {
            for sector in 0..input.len() / SECTOR_SIZE {
                let address = lba + sector as u64;
                ata_select(self, address, 0x30)?;
                let source = &input[sector * SECTOR_SIZE..(sector + 1) * SECTOR_SIZE];
                for word in 0..256 {
                    crate::arch::port_out16(
                        self.io_base,
                        u16::from_le_bytes([source[word * 2], source[word * 2 + 1]]),
                    );
                }
                crate::arch::port_out8(self.io_base + 7, 0xe7);
                if !ata_wait(self.io_base, false) {
                    return Err(BlockError::HardwareUnavailable);
                }
            }
            Ok(())
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = (lba, input);
            Err(BlockError::Unsupported)
        }
    }

    fn flush(&mut self) -> Result<(), BlockError> {
        #[cfg(target_arch = "x86_64")]
        {
            if !self.present {
                return Err(BlockError::HardwareUnavailable);
            }
            crate::arch::port_out8(self.io_base + 7, 0xe7);
            if ata_wait(self.io_base, false) {
                Ok(())
            } else {
                Err(BlockError::HardwareUnavailable)
            }
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            Err(BlockError::Unsupported)
        }
    }
}

#[cfg(target_arch = "x86_64")]
fn ata_wait(io_base: u16, require_data: bool) -> bool {
    for _ in 0..100_000 {
        let status = crate::arch::port_in8(io_base + 7);
        if status & 0x01 != 0 {
            return false;
        }
        if status & 0x80 == 0 && (!require_data || status & 0x08 != 0) {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

#[cfg(target_arch = "x86_64")]
fn ata_select(device: &AtaPio, lba: u64, command: u8) -> Result<(), BlockError> {
    if lba >= 1 << 28 || !device.present {
        return Err(BlockError::OutOfRange);
    }
    crate::arch::port_out8(
        device.io_base + 6,
        0xe0 | (device.drive << 4) | ((lba >> 24) as u8 & 0x0f),
    );
    crate::arch::port_out8(device.io_base + 2, 1);
    crate::arch::port_out8(device.io_base + 3, lba as u8);
    crate::arch::port_out8(device.io_base + 4, (lba >> 8) as u8);
    crate::arch::port_out8(device.io_base + 5, (lba >> 16) as u8);
    crate::arch::port_out8(device.io_base + 7, command);
    if ata_wait(device.io_base, true) {
        Ok(())
    } else {
        Err(BlockError::HardwareUnavailable)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AhciController {
    pub abar: u64,
    pub ports: u32,
    pub present: bool,
}

impl AhciController {
    pub const fn new(abar: u64) -> Self {
        Self {
            abar,
            ports: 0,
            present: false,
        }
    }

    pub fn probe(&mut self) -> bool {
        if self.abar == 0 {
            self.present = false;
            return false;
        }
        #[cfg(target_arch = "x86_64")]
        {
            let capabilities = unsafe { core::ptr::read_volatile(self.abar as *const u32) };
            self.ports = unsafe { core::ptr::read_volatile((self.abar + 0x0c) as *const u32) };
            self.present = capabilities != u32::MAX;
        }
        #[cfg(target_arch = "aarch64")]
        {
            self.present = false;
        }
        self.present
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NvmeController {
    pub bar0: u64,
    pub version: u32,
    pub present: bool,
    pub admin_queue_depth: u16,
    pub io_queue_depth: u16,
}

impl NvmeController {
    pub const fn new(bar0: u64) -> Self {
        Self {
            bar0,
            version: 0,
            present: false,
            admin_queue_depth: 0,
            io_queue_depth: 0,
        }
    }

    pub fn probe(&mut self) -> bool {
        if self.bar0 == 0 {
            self.present = false;
            return false;
        }
        #[cfg(target_arch = "x86_64")]
        {
            self.version = unsafe { core::ptr::read_volatile((self.bar0 + 8) as *const u32) };
            self.present = self.version != u32::MAX;
            self.admin_queue_depth = 16;
            self.io_queue_depth = 64;
        }
        #[cfg(target_arch = "aarch64")]
        {
            self.present = false;
        }
        self.present
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StorageSummary {
    pub ata_present: bool,
    pub ahci_present: bool,
    pub nvme_present: bool,
    pub async_queue_depth: usize,
    pub ext2_supported: bool,
    pub ext4_supported: bool,
    pub block_self_test: bool,
    pub async_self_test: bool,
    pub ext_self_test: bool,
}

static STORAGE_READS: AtomicU64 = AtomicU64::new(0);
static STORAGE_WRITES: AtomicU64 = AtomicU64::new(0);
static STORAGE_ERRORS: AtomicU64 = AtomicU64::new(0);
static STORAGE_STATUS: AtomicU64 = AtomicU64::new(0);

pub fn initialize() -> StorageSummary {
    let mut ata = AtaPio::primary_master();
    let ata_present = ata.probe();
    let (ahci_bar, nvme_bar) = discover_pci_storage();
    let mut ahci = AhciController::new(ahci_bar);
    let ahci_present = ahci.probe();
    let mut nvme = NvmeController::new(nvme_bar);
    let nvme_present = nvme.probe();
    let (block_self_test, async_self_test) = block_layer_self_test();
    let ext_self_test = crate::extfs::self_test();
    let summary = StorageSummary {
        ata_present,
        ahci_present,
        nvme_present,
        async_queue_depth: ASYNC_QUEUE_DEPTH,
        ext2_supported: true,
        ext4_supported: true,
        block_self_test,
        async_self_test,
        ext_self_test,
    };
    let mut flags = 0u64;
    flags |= u64::from(summary.ata_present);
    flags |= u64::from(summary.ahci_present) << 1;
    flags |= u64::from(summary.nvme_present) << 2;
    flags |= u64::from(summary.block_self_test) << 3;
    flags |= u64::from(summary.async_self_test) << 4;
    flags |= u64::from(summary.ext_self_test) << 5;
    STORAGE_STATUS.store(flags, Ordering::Release);
    summary
}

pub fn summary() -> StorageSummary {
    let flags = STORAGE_STATUS.load(Ordering::Acquire);
    StorageSummary {
        ata_present: flags & 1 != 0,
        ahci_present: flags & 2 != 0,
        nvme_present: flags & 4 != 0,
        async_queue_depth: ASYNC_QUEUE_DEPTH,
        ext2_supported: true,
        ext4_supported: true,
        block_self_test: flags & 8 != 0,
        async_self_test: flags & 16 != 0,
        ext_self_test: flags & 32 != 0,
    }
}

fn block_layer_self_test() -> (bool, bool) {
    let mut device = MemoryBlockDevice::<2048>::new();
    let input = [0x5au8; SECTOR_SIZE];
    let mut output = [0u8; SECTOR_SIZE];
    let block = device.write_sectors(1, &input).is_ok()
        && device.read_sectors(1, &mut output).is_ok()
        && output == input;
    output.fill(0);
    let request = match BlockRequest::read(1, &mut output) {
        Ok(request) => request,
        Err(_) => return (block, false),
    };
    let mut queue = AsyncBlockQueue::new();
    let token = match queue.submit(request) {
        Ok(token) => token,
        Err(_) => return (block, false),
    };
    let completion = queue.poll(&mut device);
    let asynchronous = completion.is_some_and(|completion| {
        completion.token == token && completion.state == IoState::Complete
    }) && queue.reap(token) == Ok(IoState::Complete)
        && output == input;
    (block, asynchronous)
}

#[cfg(target_arch = "x86_64")]
fn discover_pci_storage() -> (u64, u64) {
    let mut ahci = 0u64;
    let mut nvme = 0u64;
    for bus in 0u16..=255 {
        for slot in 0u8..32 {
            for function in 0u8..8 {
                let id = pci_read32(bus as u8, slot, function, 0);
                if id == u32::MAX {
                    continue;
                }
                let class = pci_read32(bus as u8, slot, function, 8);
                let base_class = (class >> 24) as u8;
                let subclass = (class >> 16) as u8;
                let programming = (class >> 8) as u8;
                if base_class != 0x01 {
                    continue;
                }
                let mut command = pci_read32(bus as u8, slot, function, 4);
                command |= 0x0006;
                pci_write32(bus as u8, slot, function, 4, command);
                if subclass == 0x06 && programming == 0x01 && ahci == 0 {
                    ahci = pci_memory_bar(bus as u8, slot, function, 0x24);
                }
                if subclass == 0x08 && programming == 0x02 && nvme == 0 {
                    nvme = pci_memory_bar(bus as u8, slot, function, 0x10);
                }
            }
        }
    }
    (map_controller_bar(ahci), map_controller_bar(nvme))
}

#[cfg(not(target_arch = "x86_64"))]
fn discover_pci_storage() -> (u64, u64) {
    (0, 0)
}

#[cfg(target_arch = "x86_64")]
fn pci_config_address(bus: u8, slot: u8, function: u8, offset: u8) -> u32 {
    0x8000_0000
        | (u32::from(bus) << 16)
        | (u32::from(slot) << 11)
        | (u32::from(function) << 8)
        | (u32::from(offset) & 0xfc)
}

#[cfg(target_arch = "x86_64")]
fn pci_read32(bus: u8, slot: u8, function: u8, offset: u8) -> u32 {
    crate::arch::port_out32(0xcf8, pci_config_address(bus, slot, function, offset));
    crate::arch::port_in32(0xcfc)
}

#[cfg(target_arch = "x86_64")]
fn pci_write32(bus: u8, slot: u8, function: u8, offset: u8, value: u32) {
    crate::arch::port_out32(0xcf8, pci_config_address(bus, slot, function, offset));
    crate::arch::port_out32(0xcfc, value);
}

#[cfg(target_arch = "x86_64")]
fn pci_memory_bar(bus: u8, slot: u8, function: u8, offset: u8) -> u64 {
    let low = pci_read32(bus, slot, function, offset);
    if low & 1 != 0 {
        return 0;
    }
    let mut address = u64::from(low & !0x0f);
    if low & 0x06 == 0x04 {
        address |= u64::from(pci_read32(bus, slot, function, offset + 4)) << 32;
    }
    address
}

#[cfg(target_arch = "x86_64")]
fn map_controller_bar(physical: u64) -> u64 {
    if physical == 0
        || physical >= crate::mm::address::MAX_DIRECT_MAPPED_PHYSICAL
        || !crate::mm::paging::map_mmio_uncached(physical, 8192)
    {
        return 0;
    }
    crate::mm::address::PHYSICAL_MEMORY_OFFSET + physical
}

pub fn io_counters() -> (u64, u64, u64) {
    (
        STORAGE_READS.load(Ordering::Relaxed),
        STORAGE_WRITES.load(Ordering::Relaxed),
        STORAGE_ERRORS.load(Ordering::Relaxed),
    )
}

pub fn record_read(result: Result<(), BlockError>) {
    match result {
        Ok(()) => {
            STORAGE_READS.fetch_add(1, Ordering::Relaxed);
        }
        Err(_) => {
            STORAGE_ERRORS.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub fn record_write(result: Result<(), BlockError>) {
    match result {
        Ok(()) => {
            STORAGE_WRITES.fetch_add(1, Ordering::Relaxed);
        }
        Err(_) => {
            STORAGE_ERRORS.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub fn copy_sector_range(input: &[u8], output: &mut [u8], offset: usize) -> usize {
    if offset >= input.len() {
        return 0;
    }
    let count = min(output.len(), input.len() - offset);
    output[..count].copy_from_slice(&input[offset..offset + count]);
    count
}

pub static MEMORY_DISK: SpinMutex<MemoryBlockDevice<131_072>> =
    SpinMutex::new(MemoryBlockDevice::new());
