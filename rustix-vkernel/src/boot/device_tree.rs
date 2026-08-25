use core::ptr;

const FDT_MAGIC: u32 = 0xd00d_feed;
const FDT_BEGIN_NODE: u32 = 1;
const FDT_END_NODE: u32 = 2;
const FDT_PROP: u32 = 3;
const FDT_NOP: u32 = 4;
const FDT_END: u32 = 9;
const MAX_DTB_SIZE: u64 = 1024 * 1024;
const MAX_DEPTH: usize = 16;
const MAX_CPUS: usize = 8;

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum PsciConduit {
    Unknown,
    Hvc,
    Smc,
}

#[derive(Clone, Copy)]
pub struct CpuBootInfo {
    pub mpidr: u64,
    pub release_address: u64,
    pub spin_table: bool,
    pub enabled: bool,
}

impl CpuBootInfo {
    const EMPTY: Self = Self {
        mpidr: 0,
        release_address: 0,
        spin_table: false,
        enabled: false,
    };
}

#[derive(Clone, Copy)]
pub struct HardwareDiscovery {
    pub valid: bool,
    pub gic_distributor_base: u64,
    pub gic_cpu_interface_base: u64,
    pub system_timer_base: u64,
    pub pcie_controller_base: u64,
    pub physical_timer_interrupt: u32,
    pub psci_conduit: PsciConduit,
    pub cpus: [CpuBootInfo; MAX_CPUS],
    pub cpu_count: usize,
}

impl HardwareDiscovery {
    pub const fn empty() -> Self {
        Self {
            valid: false,
            gic_distributor_base: 0,
            gic_cpu_interface_base: 0,
            system_timer_base: 0,
            pcie_controller_base: 0,
            physical_timer_interrupt: 30,
            psci_conduit: PsciConduit::Unknown,
            cpus: [CpuBootInfo::EMPTY; MAX_CPUS],
            cpu_count: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct NodeState {
    parent_address_cells: usize,
    parent_size_cells: usize,
    child_address_cells: usize,
    child_size_cells: usize,
    cpus_container: bool,
    cpu: bool,
    psci: bool,
    arm_timer: bool,
    gic: bool,
    system_timer: bool,
    pcie: bool,
    enabled: bool,
    spin_table: bool,
    psci_conduit: PsciConduit,
    registers: [u64; 4],
    register_count: usize,
    cpu_mpidr: u64,
    release_address: u64,
    physical_timer_interrupt: u32,
}

impl NodeState {
    const EMPTY: Self = Self {
        parent_address_cells: 2,
        parent_size_cells: 1,
        child_address_cells: 2,
        child_size_cells: 1,
        cpus_container: false,
        cpu: false,
        psci: false,
        arm_timer: false,
        gic: false,
        system_timer: false,
        pcie: false,
        enabled: true,
        spin_table: false,
        psci_conduit: PsciConduit::Unknown,
        registers: [0; 4],
        register_count: 0,
        cpu_mpidr: 0,
        release_address: 0,
        physical_timer_interrupt: 30,
    };
}

pub fn discover_hardware(address: u64) -> HardwareDiscovery {
    let mut discovery = HardwareDiscovery::empty();
    if address == 0 || address & 7 != 0 || read_be_u32(address) != FDT_MAGIC {
        return discovery;
    }
    let total_size = read_be_u32(address + 4) as u64;
    let structure_offset = read_be_u32(address + 8) as u64;
    let strings_offset = read_be_u32(address + 12) as u64;
    let strings_size = read_be_u32(address + 32) as u64;
    let structure_size = read_be_u32(address + 36) as u64;
    if !(40..=MAX_DTB_SIZE).contains(&total_size)
        || !range_valid(structure_offset, structure_size, total_size)
        || !range_valid(strings_offset, strings_size, total_size)
    {
        return discovery;
    }

    let mut nodes = [NodeState::EMPTY; MAX_DEPTH];
    let mut depth = 0usize;
    let mut cursor = structure_offset;
    let structure_end = structure_offset + structure_size;
    discovery.valid = true;

    while cursor + 4 <= structure_end {
        let token = read_be_u32(address + cursor);
        cursor += 4;
        match token {
            FDT_BEGIN_NODE => {
                if depth == MAX_DEPTH {
                    return HardwareDiscovery::empty();
                }
                let Some(name_length) = string_length(address + cursor, structure_end - cursor)
                else {
                    return HardwareDiscovery::empty();
                };
                let parent = if depth == 0 {
                    NodeState::EMPTY
                } else {
                    nodes[depth - 1]
                };
                let mut node = NodeState::EMPTY;
                node.parent_address_cells = parent.child_address_cells;
                node.parent_size_cells = parent.child_size_cells;
                node.child_address_cells = parent.child_address_cells;
                node.child_size_cells = parent.child_size_cells;
                node.cpus_container = name_equals(address + cursor, name_length, b"cpus");
                node.cpu = depth > 0
                    && parent.cpus_container
                    && name_starts_with(address + cursor, name_length, b"cpu@");
                node.psci = name_equals(address + cursor, name_length, b"psci");
                node.arm_timer = name_equals(address + cursor, name_length, b"timer");
                nodes[depth] = node;
                depth += 1;
                cursor = align4(cursor + name_length + 1);
            }
            FDT_END_NODE => {
                if depth == 0 {
                    return HardwareDiscovery::empty();
                }
                depth -= 1;
                commit_node(nodes[depth], &mut discovery);
            }
            FDT_PROP => {
                if depth == 0 || cursor + 8 > structure_end {
                    return HardwareDiscovery::empty();
                }
                let value_size = read_be_u32(address + cursor) as u64;
                let name_offset = read_be_u32(address + cursor + 4) as u64;
                cursor += 8;
                if cursor + value_size > structure_end || name_offset >= strings_size {
                    return HardwareDiscovery::empty();
                }
                parse_property(
                    address,
                    strings_offset,
                    strings_size,
                    name_offset,
                    cursor,
                    value_size,
                    &mut nodes[depth - 1],
                );
                cursor = align4(cursor + value_size);
            }
            FDT_NOP => {}
            FDT_END => return discovery,
            _ => return HardwareDiscovery::empty(),
        }
    }
    HardwareDiscovery::empty()
}

fn parse_property(
    base: u64,
    strings_offset: u64,
    strings_size: u64,
    name_offset: u64,
    value: u64,
    value_size: u64,
    node: &mut NodeState,
) {
    if property_equals(
        base,
        strings_offset,
        strings_size,
        name_offset,
        b"#address-cells",
    ) && value_size == 4
    {
        node.child_address_cells = read_be_u32(base + value) as usize;
    } else if property_equals(
        base,
        strings_offset,
        strings_size,
        name_offset,
        b"#size-cells",
    ) && value_size == 4
    {
        node.child_size_cells = read_be_u32(base + value) as usize;
    } else if property_equals(
        base,
        strings_offset,
        strings_size,
        name_offset,
        b"compatible",
    ) {
        node.gic |= string_list_contains(base + value, value_size, b"arm,gic-400")
            || string_list_contains(base + value, value_size, b"arm,cortex-a15-gic");
        node.system_timer |=
            string_list_contains(base + value, value_size, b"brcm,bcm2835-system-timer");
        node.pcie |= string_list_contains(base + value, value_size, b"brcm,bcm2712-pcie");
        node.arm_timer |= string_list_contains(base + value, value_size, b"arm,armv8-timer")
            || string_list_contains(base + value, value_size, b"arm,armv7-timer");
        node.psci |= string_list_contains(base + value, value_size, b"arm,psci-0.2")
            || string_list_contains(base + value, value_size, b"arm,psci-1.0");
    } else if property_equals(base, strings_offset, strings_size, name_offset, b"reg") {
        parse_registers(base + value, value_size, node);
    } else if property_equals(
        base,
        strings_offset,
        strings_size,
        name_offset,
        b"interrupts",
    ) && node.arm_timer
    {
        parse_timer_interrupts(base + value, value_size, node);
    } else if property_equals(base, strings_offset, strings_size, name_offset, b"method") {
        node.psci_conduit = if value_equals(base + value, value_size, b"hvc") {
            PsciConduit::Hvc
        } else if value_equals(base + value, value_size, b"smc") {
            PsciConduit::Smc
        } else {
            PsciConduit::Unknown
        };
    } else if property_equals(
        base,
        strings_offset,
        strings_size,
        name_offset,
        b"enable-method",
    ) {
        node.spin_table = value_equals(base + value, value_size, b"spin-table");
    } else if property_equals(
        base,
        strings_offset,
        strings_size,
        name_offset,
        b"cpu-release-addr",
    ) {
        node.release_address = read_cells(base + value, (value_size / 4).min(2) as usize);
    } else if property_equals(base, strings_offset, strings_size, name_offset, b"status") {
        node.enabled = !value_equals(base + value, value_size, b"disabled");
    }
}

fn parse_registers(address: u64, size: u64, node: &mut NodeState) {
    if node.cpu {
        if (1..=2).contains(&node.parent_address_cells)
            && size >= (node.parent_address_cells * 4) as u64
        {
            node.cpu_mpidr = read_cells(address, node.parent_address_cells);
        }
        return;
    }
    if !(1..=2).contains(&node.parent_address_cells) || !(1..=2).contains(&node.parent_size_cells) {
        return;
    }
    let tuple_cells = node.parent_address_cells + node.parent_size_cells;
    let tuple_size = (tuple_cells * 4) as u64;
    let mut offset = 0u64;
    while offset + tuple_size <= size && node.register_count < node.registers.len() {
        node.registers[node.register_count] =
            read_cells(address + offset, node.parent_address_cells);
        node.register_count += 1;
        offset += tuple_size;
    }
}

fn parse_timer_interrupts(address: u64, size: u64, node: &mut NodeState) {
    let mut offset = 0u64;
    while offset + 12 <= size {
        let kind = read_be_u32(address + offset);
        let number = read_be_u32(address + offset + 4);
        if kind == 1 && number == 14 {
            node.physical_timer_interrupt = 16 + number;
            return;
        }
        offset += 12;
    }
}

fn commit_node(node: NodeState, discovery: &mut HardwareDiscovery) {
    if node.gic && node.register_count >= 2 {
        discovery.gic_distributor_base = node.registers[0];
        discovery.gic_cpu_interface_base = node.registers[1];
    }
    if node.system_timer && node.register_count != 0 {
        discovery.system_timer_base = node.registers[0];
    }
    if node.pcie && node.register_count != 0 {
        discovery.pcie_controller_base = node.registers[0];
    }
    if node.arm_timer {
        discovery.physical_timer_interrupt = node.physical_timer_interrupt;
    }
    if node.psci && node.psci_conduit != PsciConduit::Unknown {
        discovery.psci_conduit = node.psci_conduit;
    }
    if node.cpu && node.enabled && discovery.cpu_count < discovery.cpus.len() {
        discovery.cpus[discovery.cpu_count] = CpuBootInfo {
            mpidr: node.cpu_mpidr,
            release_address: node.release_address,
            spin_table: node.spin_table,
            enabled: true,
        };
        discovery.cpu_count += 1;
    }
}

fn property_equals(
    base: u64,
    strings_offset: u64,
    strings_size: u64,
    offset: u64,
    expected: &[u8],
) -> bool {
    if offset + expected.len() as u64 >= strings_size {
        return false;
    }
    value_equals(
        base + strings_offset + offset,
        expected.len() as u64 + 1,
        expected,
    )
}

fn string_list_contains(address: u64, size: u64, expected: &[u8]) -> bool {
    let mut offset = 0u64;
    while offset < size {
        let Some(length) = string_length(address + offset, size - offset) else {
            return false;
        };
        if name_equals(address + offset, length, expected) {
            return true;
        }
        offset += length + 1;
    }
    false
}

fn value_equals(address: u64, size: u64, expected: &[u8]) -> bool {
    size > expected.len() as u64
        && expected
            .iter()
            .enumerate()
            .all(|(index, byte)| read_u8(address + index as u64) == *byte)
        && read_u8(address + expected.len() as u64) == 0
}

fn name_equals(address: u64, length: u64, expected: &[u8]) -> bool {
    length == expected.len() as u64
        && expected
            .iter()
            .enumerate()
            .all(|(index, byte)| read_u8(address + index as u64) == *byte)
}

fn name_starts_with(address: u64, length: u64, expected: &[u8]) -> bool {
    length >= expected.len() as u64
        && expected
            .iter()
            .enumerate()
            .all(|(index, byte)| read_u8(address + index as u64) == *byte)
}

fn string_length(address: u64, maximum: u64) -> Option<u64> {
    (0..maximum).find(|offset| read_u8(address + *offset) == 0)
}

fn read_cells(address: u64, cells: usize) -> u64 {
    let mut value = 0u64;
    for cell in 0..cells {
        value = (value << 32) | read_be_u32(address + (cell * 4) as u64) as u64;
    }
    value
}

fn read_u8(address: u64) -> u8 {
    unsafe { ptr::read_volatile(address as *const u8) }
}

fn read_be_u32(address: u64) -> u32 {
    u32::from_be(unsafe { ptr::read_unaligned(address as *const u32) })
}

fn range_valid(offset: u64, size: u64, total: u64) -> bool {
    offset
        .checked_add(size)
        .is_some_and(|end| offset < total && end <= total)
}

const fn align4(value: u64) -> u64 {
    value.saturating_add(3) & !3
}
