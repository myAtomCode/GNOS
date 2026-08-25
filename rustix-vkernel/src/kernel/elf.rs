use crate::linux::elf::{
    ElfError, ElfFile, ProgramHeader, EM_X86_64, ET_DYN, PF_W, PF_X, PROGRAM_HEADER_SIZE, PT_LOAD,
    PT_PHDR,
};
use crate::mm::address::{PhysicalAddress, VirtualAddress, PAGE_SIZE, USER_ADDRESS_LIMIT};
use crate::arch::console_write;
use crate::mm::frame;
use crate::mm::paging::{AddressSpace, MapFlags};

const MAX_IMAGE_PAGES: usize = 1024;

static IMAGE_PAGE_PLANS: crate::sync::IrqSpinMutex<[PagePlan; MAX_IMAGE_PAGES], 3> =
    crate::sync::IrqSpinMutex::new([PagePlan::EMPTY; MAX_IMAGE_PAGES]);
const MAX_ARGUMENTS: usize = 8;
const MAX_ENVIRONMENT: usize = 8;
const MAX_STRING_BYTES: usize = 256;
const USER_STACK_TOP: u64 = 0x0000_7fff_ffff_f000;
const USER_STACK_PAGES: usize = 64;
const MAIN_ASLR_START: u64 = 0x0000_0000_1000_0000;
const MAIN_ASLR_END: u64 = 0x0000_0000_4000_0000;
const INTERP_ASLR_START: u64 = 0x0000_0000_5000_0000;
const INTERP_ASLR_END: u64 = 0x0000_0000_7000_0000;
const VDSO_ASLR_START: u64 = 0x0000_0000_7200_0000;
const VDSO_ASLR_END: u64 = 0x0000_0000_7800_0000;
const ASLR_GRANULARITY: u64 = 2 * 1024 * 1024;

const AT_NULL: u64 = 0;
const AT_PHDR: u64 = 3;
const AT_PHENT: u64 = 4;
const AT_PHNUM: u64 = 5;
const AT_PAGESZ: u64 = 6;
const AT_BASE: u64 = 7;
const AT_ENTRY: u64 = 9;
const AT_UID: u64 = 11;
const AT_EUID: u64 = 12;
const AT_GID: u64 = 13;
const AT_EGID: u64 = 14;
const AT_SECURE: u64 = 23;
const AT_RANDOM: u64 = 25;
const AT_EXECFN: u64 = 31;
const AT_SYSINFO_EHDR: u64 = 33;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoadError {
    Elf(ElfError),
    InterpreterMissing,
    InterpreterMismatch,
    InterpreterNested,
    InvalidVdso,
    InvalidSegment,
    TooManyPages,
    AddressConflict,
    WriteExecute,
    OutOfMemory,
    InvalidArguments,
    StackOverflow,
}

impl From<ElfError> for LoadError {
    fn from(error: ElfError) -> Self {
        Self::Elf(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LoadedProcess {
    pub cr3: u64,
    pub entry: u64,
    pub stack_pointer: u64,
    pub load_bias: u64,
    pub interpreter_bias: u64,
    pub vdso_base: u64,
}

#[derive(Clone, Copy)]
struct LoadedElf {
    entry: u64,
    load_bias: u64,
    program_headers: u64,
    program_header_count: usize,
}

#[derive(Clone, Copy)]
struct PagePlan {
    used: bool,
    address: u64,
    writable: bool,
    executable: bool,
    physical: Option<PhysicalAddress>,
}

impl PagePlan {
    const EMPTY: Self = Self {
        used: false,
        address: 0,
        writable: false,
        executable: false,
        physical: None,
    };
}

pub fn load_process(
    main_image: &[u8],
    interpreter: Option<(&str, &[u8])>,
    vdso_image: &[u8],
    arguments: &[&str],
    environment: &[&str],
    random_seed: u64,
) -> Result<LoadedProcess, LoadError> {
    if arguments.is_empty()
        || arguments.len() > MAX_ARGUMENTS
        || environment.len() > MAX_ENVIRONMENT
        || arguments.iter().chain(environment).any(|value| {
            value.is_empty() || value.len() >= MAX_STRING_BYTES || value.as_bytes().contains(&0)
        })
    {
        return Err(LoadError::InvalidArguments);
    }

    let main = ElfFile::parse(main_image, EM_X86_64)?;
    let vdso = ElfFile::parse(vdso_image, EM_X86_64)?;
    if vdso.elf_type() != ET_DYN || vdso.interpreter().is_some() {
        return Err(LoadError::InvalidVdso);
    }
    let mut address_space = AddressSpace::new_user().map_err(|_| LoadError::OutOfMemory)?;
    let result = load_process_inner(
        &mut address_space,
        &main,
        interpreter,
        &vdso,
        arguments,
        environment,
        random_seed,
    );
    match result {
        Ok((entry, stack_pointer, main_elf, interpreter_bias, vdso_base)) => Ok(LoadedProcess {
            cr3: address_space.root().as_u64(),
            entry,
            stack_pointer,
            load_bias: main_elf.load_bias,
            interpreter_bias,
            vdso_base,
        }),
        Err(error) => {
            let _ = address_space.destroy();
            Err(error)
        }
    }
}

fn load_process_inner(
    address_space: &mut AddressSpace,
    main: &ElfFile<'_>,
    interpreter_source: Option<(&str, &[u8])>,
    vdso: &ElfFile<'_>,
    arguments: &[&str],
    environment: &[&str],
    random_seed: u64,
) -> Result<(u64, u64, LoadedElf, u64, u64), LoadError> {
    let main_elf = map_elf(
        address_space,
        main,
        MAIN_ASLR_START,
        MAIN_ASLR_END,
        random_seed,
    )?;

    let (entry, interpreter_bias) = if let Some(path) = main.interpreter() {
        let (registered_path, image) = interpreter_source.ok_or(LoadError::InterpreterMissing)?;
        if registered_path != path {
            return Err(LoadError::InterpreterMismatch);
        }
        let interpreter = ElfFile::parse(image, EM_X86_64)?;
        if interpreter.interpreter().is_some() {
            return Err(LoadError::InterpreterNested);
        }
        let loaded = map_elf(
            address_space,
            &interpreter,
            INTERP_ASLR_START,
            INTERP_ASLR_END,
            random_seed.rotate_left(29),
        )?;
        (loaded.entry, loaded.load_bias)
    } else {
        (main_elf.entry, 0)
    };

    let vdso_elf = map_elf(
        address_space,
        vdso,
        VDSO_ASLR_START,
        VDSO_ASLR_END,
        random_seed.rotate_left(47),
    )?;

    let stack = address_space
        .map_guarded_stack(USER_STACK_TOP, USER_STACK_PAGES)
        .map_err(|_| LoadError::OutOfMemory)?;
    let stack_jitter = (mix(random_seed) & 0x7f) * 16;
    let stack_top = stack
        .top
        .checked_sub(stack_jitter)
        .ok_or(LoadError::StackOverflow)?;
    let stack_pointer = build_initial_stack(
        address_space,
        stack.bottom,
        stack_top,
        arguments,
        environment,
        main_elf,
        interpreter_bias,
        vdso_elf.load_bias,
        random_seed,
    )?;
    Ok((
        entry,
        stack_pointer,
        main_elf,
        interpreter_bias,
        vdso_elf.load_bias,
    ))
}

fn map_elf(
    address_space: &mut AddressSpace,
    elf: &ElfFile<'_>,
    aslr_start: u64,
    aslr_end: u64,
    random_seed: u64,
) -> Result<LoadedElf, LoadError> {
    let load_bias = choose_load_bias(elf, aslr_start, aslr_end, random_seed)?;
    let mut plans = IMAGE_PAGE_PLANS.lock();
    let pages: &mut [PagePlan; MAX_IMAGE_PAGES] = &mut *plans;
    let mut page_count = 0usize;
    for header in elf.program_headers() {
        if header.kind == PT_LOAD && header.memory_size != 0 {
            collect_pages(*header, load_bias, pages, &mut page_count)?;
        }
    }
    for page in &mut pages[..page_count] {
        if page.writable && page.executable {
            console_write("[dbg] wx-page va=");
            let mut hex = [0u8; 16];
            let mut value = page.address;
            let mut index = 16;
            while index > 0 {
                index -= 1;
                let digit = (value & 0xf) as u8;
                hex[index] = if digit < 10 { b'0' + digit } else { b'a' + digit - 10 };
                value >>= 4;
            }
            console_write(core::str::from_utf8(&hex).unwrap_or("?"));
            console_write("\n");
            return Err(LoadError::WriteExecute);
        }
        let physical = frame::allocate().map_err(|_| LoadError::OutOfMemory)?;
        unsafe {
            core::ptr::write_bytes(physical.direct_mapped() as *mut u8, 0, PAGE_SIZE as usize);
        }
        let mut flags = MapFlags::PRESENT | MapFlags::USER;
        if page.writable {
            flags |= MapFlags::WRITABLE | MapFlags::NO_EXECUTE;
        } else if !page.executable {
            flags |= MapFlags::NO_EXECUTE;
        }
        let virtual_address = VirtualAddress::new(page.address).ok_or(LoadError::InvalidSegment)?;
        if address_space
            .map_user(virtual_address, physical, flags)
            .is_err()
        {
            let _ = frame::deallocate(physical);
            return Err(LoadError::AddressConflict);
        }
        page.physical = Some(physical);
    }

    for header in elf.program_headers() {
        if header.kind != PT_LOAD || header.file_size == 0 {
            continue;
        }
        let data = elf.segment_data(*header)?;
        copy_segment(&pages[..page_count], load_bias, *header, data)?;
    }

    let entry = load_bias
        .checked_add(elf.entry())
        .ok_or(LoadError::InvalidSegment)?;
    let table_bytes = elf
        .program_header_count()
        .checked_mul(PROGRAM_HEADER_SIZE)
        .ok_or(LoadError::InvalidSegment)?;
    let program_headers = if let Some(header) = elf
        .program_headers()
        .iter()
        .find(|header| header.kind == PT_PHDR)
    {
        load_bias
            .checked_add(header.virtual_address)
            .ok_or(LoadError::InvalidSegment)?
    } else {
        let table_end = elf
            .program_offset()
            .checked_add(table_bytes as u64)
            .ok_or(LoadError::InvalidSegment)?;
        let header = elf
            .program_headers()
            .iter()
            .find(|header| {
                header.kind == PT_LOAD
                    && elf.program_offset() >= header.offset
                    && table_end <= header.offset.saturating_add(header.file_size)
            })
            .ok_or(LoadError::InvalidSegment)?;
        load_bias
            .checked_add(header.virtual_address)
            .and_then(|address| address.checked_add(elf.program_offset() - header.offset))
            .ok_or(LoadError::InvalidSegment)?
    };
    if !mapped_range(&pages[..page_count], program_headers, table_bytes) {
        return Err(LoadError::InvalidSegment);
    }
    Ok(LoadedElf {
        entry,
        load_bias,
        program_headers,
        program_header_count: elf.program_header_count(),
    })
}

fn choose_load_bias(
    elf: &ElfFile<'_>,
    aslr_start: u64,
    aslr_end: u64,
    random_seed: u64,
) -> Result<u64, LoadError> {
    if elf.elf_type() != ET_DYN {
        return Ok(0);
    }
    let mut minimum = u64::MAX;
    let mut maximum = 0u64;
    for header in elf.program_headers() {
        if header.kind != PT_LOAD || header.memory_size == 0 {
            continue;
        }
        minimum = minimum.min(align_down(header.virtual_address));
        maximum = maximum.max(align_up(
            header
                .virtual_address
                .checked_add(header.memory_size)
                .ok_or(LoadError::InvalidSegment)?,
        )?);
    }
    let span = maximum
        .checked_sub(minimum)
        .ok_or(LoadError::InvalidSegment)?;
    let available = aslr_end
        .checked_sub(aslr_start)
        .and_then(|size| size.checked_sub(span))
        .ok_or(LoadError::InvalidSegment)?;
    let slots = available / ASLR_GRANULARITY + 1;
    let runtime_start = aslr_start + mix(random_seed) % slots * ASLR_GRANULARITY;
    runtime_start
        .checked_sub(minimum)
        .ok_or(LoadError::InvalidSegment)
}

fn collect_pages(
    header: ProgramHeader,
    load_bias: u64,
    pages: &mut [PagePlan; MAX_IMAGE_PAGES],
    page_count: &mut usize,
) -> Result<(), LoadError> {
    if header.offset & (PAGE_SIZE - 1) != header.virtual_address & (PAGE_SIZE - 1) {
        return Err(LoadError::InvalidSegment);
    }
    let start = load_bias
        .checked_add(header.virtual_address)
        .ok_or(LoadError::InvalidSegment)?;
    let end = start
        .checked_add(header.memory_size)
        .ok_or(LoadError::InvalidSegment)?;
    if start < PAGE_SIZE || end > USER_ADDRESS_LIMIT || end <= start {
        return Err(LoadError::InvalidSegment);
    }
    let mut address = align_down(start);
    let page_end = align_up(end)?;
    while address < page_end {
        let index = if let Some(index) = pages[..*page_count]
            .iter()
            .position(|page| page.address == address)
        {
            index
        } else {
            if *page_count == pages.len() {
                return Err(LoadError::TooManyPages);
            }
            let index = *page_count;
            pages[index] = PagePlan {
                used: true,
                address,
                writable: false,
                executable: false,
                physical: None,
            };
            *page_count += 1;
            index
        };
        pages[index].writable |= header.flags & PF_W != 0;
        pages[index].executable |= header.flags & PF_X != 0;
        address = address
            .checked_add(PAGE_SIZE)
            .ok_or(LoadError::InvalidSegment)?;
    }
    Ok(())
}

fn copy_segment(
    pages: &[PagePlan],
    load_bias: u64,
    header: ProgramHeader,
    data: &[u8],
) -> Result<(), LoadError> {
    let mut copied = 0usize;
    let start = load_bias
        .checked_add(header.virtual_address)
        .ok_or(LoadError::InvalidSegment)?;
    while copied < data.len() {
        let address = start
            .checked_add(copied as u64)
            .ok_or(LoadError::InvalidSegment)?;
        let page_address = align_down(address);
        let page = pages
            .iter()
            .find(|page| page.used && page.address == page_address)
            .ok_or(LoadError::InvalidSegment)?;
        let physical = page.physical.ok_or(LoadError::InvalidSegment)?;
        let offset = (address - page_address) as usize;
        let chunk = (PAGE_SIZE as usize - offset).min(data.len() - copied);
        unsafe {
            core::ptr::copy_nonoverlapping(
                data[copied..].as_ptr(),
                (physical.direct_mapped() + offset as u64) as *mut u8,
                chunk,
            );
        }
        copied += chunk;
    }
    Ok(())
}

fn mapped_range(pages: &[PagePlan], address: u64, length: usize) -> bool {
    let Some(end) = address.checked_add(length as u64) else {
        return false;
    };
    let mut current = address;
    while current < end {
        if !pages
            .iter()
            .any(|page| page.used && page.address == align_down(current))
        {
            return false;
        }
        current = align_down(current).saturating_add(PAGE_SIZE);
    }
    true
}

fn build_initial_stack(
    address_space: &AddressSpace,
    stack_bottom: u64,
    stack_top: u64,
    arguments: &[&str],
    environment: &[&str],
    main: LoadedElf,
    interpreter_bias: u64,
    vdso_base: u64,
    random_seed: u64,
) -> Result<u64, LoadError> {
    let mut stack_pointer = stack_top;
    let mut argument_pointers = [0u64; MAX_ARGUMENTS];
    let mut environment_pointers = [0u64; MAX_ENVIRONMENT];
    for index in (0..environment.len()).rev() {
        environment_pointers[index] = push_string(
            address_space,
            stack_bottom,
            &mut stack_pointer,
            environment[index],
        )?;
    }
    for index in (0..arguments.len()).rev() {
        argument_pointers[index] = push_string(
            address_space,
            stack_bottom,
            &mut stack_pointer,
            arguments[index],
        )?;
    }
    let execfn = argument_pointers[0];
    let mut random = [0u8; 16];
    random[..8].copy_from_slice(&mix(random_seed).to_ne_bytes());
    random[8..].copy_from_slice(&mix(random_seed.rotate_left(31)).to_ne_bytes());
    stack_pointer = stack_pointer
        .checked_sub(random.len() as u64)
        .ok_or(LoadError::StackOverflow)?;
    if stack_pointer < stack_bottom {
        return Err(LoadError::StackOverflow);
    }
    address_space
        .copy_to_user(stack_pointer, &random)
        .map_err(|_| LoadError::StackOverflow)?;
    let random_pointer = stack_pointer;

    let auxv = [
        (AT_PHDR, main.program_headers),
        (AT_PHENT, PROGRAM_HEADER_SIZE as u64),
        (AT_PHNUM, main.program_header_count as u64),
        (AT_PAGESZ, PAGE_SIZE),
        (AT_BASE, interpreter_bias),
        (AT_ENTRY, main.entry),
        (AT_UID, 0),
        (AT_EUID, 0),
        (AT_GID, 0),
        (AT_EGID, 0),
        (AT_SECURE, 0),
        (AT_RANDOM, random_pointer),
        (AT_EXECFN, execfn),
        (AT_SYSINFO_EHDR, vdso_base),
        (AT_NULL, 0),
    ];
    let word_count = 1 + arguments.len() + 1 + environment.len() + 1 + auxv.len() * 2;
    let table_size = word_count
        .checked_mul(core::mem::size_of::<u64>())
        .ok_or(LoadError::StackOverflow)?;
    stack_pointer = stack_pointer
        .checked_sub(table_size as u64)
        .ok_or(LoadError::StackOverflow)?
        & !15;
    if stack_pointer < stack_bottom {
        return Err(LoadError::StackOverflow);
    }
    let mut words = [0u64; 64];
    if word_count > words.len() {
        return Err(LoadError::InvalidArguments);
    }
    let mut word = 0usize;
    words[word] = arguments.len() as u64;
    word += 1;
    for pointer in &argument_pointers[..arguments.len()] {
        words[word] = *pointer;
        word += 1;
    }
    word += 1;
    for pointer in &environment_pointers[..environment.len()] {
        words[word] = *pointer;
        word += 1;
    }
    word += 1;
    for (kind, value) in auxv {
        words[word] = kind;
        words[word + 1] = value;
        word += 2;
    }
    let bytes = unsafe { core::slice::from_raw_parts(words.as_ptr() as *const u8, table_size) };
    address_space
        .copy_to_user(stack_pointer, bytes)
        .map_err(|_| LoadError::StackOverflow)?;
    Ok(stack_pointer)
}

fn push_string(
    address_space: &AddressSpace,
    stack_bottom: u64,
    stack_pointer: &mut u64,
    value: &str,
) -> Result<u64, LoadError> {
    *stack_pointer = stack_pointer
        .checked_sub(value.len() as u64 + 1)
        .ok_or(LoadError::StackOverflow)?;
    if *stack_pointer < stack_bottom {
        return Err(LoadError::StackOverflow);
    }
    address_space
        .copy_to_user(*stack_pointer, value.as_bytes())
        .map_err(|_| LoadError::StackOverflow)?;
    address_space
        .copy_to_user(*stack_pointer + value.len() as u64, &[0])
        .map_err(|_| LoadError::StackOverflow)?;
    Ok(*stack_pointer)
}

const fn align_down(value: u64) -> u64 {
    value & !(PAGE_SIZE - 1)
}

fn align_up(value: u64) -> Result<u64, LoadError> {
    value
        .checked_add(PAGE_SIZE - 1)
        .map(align_down)
        .ok_or(LoadError::InvalidSegment)
}

fn mix(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}
