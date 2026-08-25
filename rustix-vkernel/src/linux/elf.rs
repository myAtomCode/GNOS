use core::str;

pub const ELF_HEADER_SIZE: usize = 64;
pub const PROGRAM_HEADER_SIZE: usize = 56;
pub const MAX_PROGRAM_HEADERS: usize = 16;

pub const ET_EXEC: u16 = 2;
pub const ET_DYN: u16 = 3;
pub const EM_X86_64: u16 = 62;

pub const PT_LOAD: u32 = 1;
pub const PT_INTERP: u32 = 3;
pub const PT_PHDR: u32 = 6;

pub const PF_X: u32 = 1;
pub const PF_W: u32 = 2;
pub const PF_R: u32 = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ElfError {
    Truncated,
    BadMagic,
    UnsupportedClass,
    UnsupportedEndian,
    UnsupportedVersion,
    UnsupportedType,
    UnsupportedMachine,
    InvalidHeader,
    TooManyProgramHeaders,
    InvalidProgramHeader,
    InvalidAlignment,
    InvalidFileRange,
    InvalidMemoryRange,
    MissingLoadSegment,
    InvalidEntry,
    InvalidInterpreter,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramHeader {
    pub kind: u32,
    pub flags: u32,
    pub offset: u64,
    pub virtual_address: u64,
    pub file_size: u64,
    pub memory_size: u64,
    pub alignment: u64,
}

impl ProgramHeader {
    const EMPTY: Self = Self {
        kind: 0,
        flags: 0,
        offset: 0,
        virtual_address: 0,
        file_size: 0,
        memory_size: 0,
        alignment: 0,
    };
}

pub struct ElfFile<'a> {
    image: &'a [u8],
    elf_type: u16,
    entry: u64,
    program_offset: u64,
    headers: [ProgramHeader; MAX_PROGRAM_HEADERS],
    header_count: usize,
    interpreter: Option<&'a str>,
}

impl<'a> ElfFile<'a> {
    pub fn parse(image: &'a [u8], machine: u16) -> Result<Self, ElfError> {
        if image.len() < ELF_HEADER_SIZE {
            return Err(ElfError::Truncated);
        }
        if image[..4] != [0x7f, b'E', b'L', b'F'] {
            return Err(ElfError::BadMagic);
        }
        if image[4] != 2 {
            return Err(ElfError::UnsupportedClass);
        }
        if image[5] != 1 {
            return Err(ElfError::UnsupportedEndian);
        }
        if image[6] != 1 || read_u32(image, 20)? != 1 {
            return Err(ElfError::UnsupportedVersion);
        }
        let elf_type = read_u16(image, 16)?;
        if !matches!(elf_type, ET_EXEC | ET_DYN) {
            return Err(ElfError::UnsupportedType);
        }
        if read_u16(image, 18)? != machine {
            return Err(ElfError::UnsupportedMachine);
        }
        if read_u16(image, 52)? as usize != ELF_HEADER_SIZE
            || read_u16(image, 54)? as usize != PROGRAM_HEADER_SIZE
        {
            return Err(ElfError::InvalidHeader);
        }
        let header_count = read_u16(image, 56)? as usize;
        if header_count == 0 || header_count > MAX_PROGRAM_HEADERS {
            return Err(ElfError::TooManyProgramHeaders);
        }
        let program_offset = read_u64(image, 32)?;
        let table_size = header_count
            .checked_mul(PROGRAM_HEADER_SIZE)
            .ok_or(ElfError::InvalidHeader)?;
        checked_file_range(image, program_offset, table_size as u64)?;

        let mut headers = [ProgramHeader::EMPTY; MAX_PROGRAM_HEADERS];
        let mut load_segments = 0usize;
        let mut interpreter = None;
        for (index, header) in headers[..header_count].iter_mut().enumerate() {
            let offset = usize::try_from(program_offset)
                .ok()
                .and_then(|base| {
                    index
                        .checked_mul(PROGRAM_HEADER_SIZE)
                        .and_then(|i| base.checked_add(i))
                })
                .ok_or(ElfError::InvalidHeader)?;
            *header = ProgramHeader {
                kind: read_u32(image, offset)?,
                flags: read_u32(image, offset + 4)?,
                offset: read_u64(image, offset + 8)?,
                virtual_address: read_u64(image, offset + 16)?,
                file_size: read_u64(image, offset + 32)?,
                memory_size: read_u64(image, offset + 40)?,
                alignment: read_u64(image, offset + 48)?,
            };
            if header.alignment != 0 && !header.alignment.is_power_of_two() {
                return Err(ElfError::InvalidAlignment);
            }
            if header.kind == PT_LOAD
                && (header.flags & !(PF_R | PF_W | PF_X) != 0 || header.flags & PF_R == 0)
            {
                return Err(ElfError::InvalidProgramHeader);
            }
            if header.file_size > header.memory_size && header.kind == PT_LOAD {
                return Err(ElfError::InvalidMemoryRange);
            }
            if header.file_size != 0 {
                checked_file_range(image, header.offset, header.file_size)?;
            }
            header
                .virtual_address
                .checked_add(header.memory_size)
                .ok_or(ElfError::InvalidMemoryRange)?;

            match header.kind {
                PT_LOAD => load_segments += 1,
                PT_INTERP => {
                    if interpreter.is_some() || !(2..=128).contains(&header.file_size) {
                        return Err(ElfError::InvalidInterpreter);
                    }
                    let bytes = file_range(image, header.offset, header.file_size)?;
                    if bytes.last() != Some(&0) || bytes[..bytes.len() - 1].contains(&0) {
                        return Err(ElfError::InvalidInterpreter);
                    }
                    let path = str::from_utf8(&bytes[..bytes.len() - 1])
                        .map_err(|_| ElfError::InvalidInterpreter)?;
                    if !path.starts_with('/') {
                        return Err(ElfError::InvalidInterpreter);
                    }
                    interpreter = Some(path);
                }
                _ => {}
            }
        }
        if load_segments == 0 {
            return Err(ElfError::MissingLoadSegment);
        }

        let entry = read_u64(image, 24)?;
        let executable_entry = headers[..header_count].iter().any(|header| {
            header.kind == PT_LOAD
                && header.flags & PF_X != 0
                && entry >= header.virtual_address
                && entry < header.virtual_address.saturating_add(header.memory_size)
        });
        if !executable_entry {
            return Err(ElfError::InvalidEntry);
        }

        Ok(Self {
            image,
            elf_type,
            entry,
            program_offset,
            headers,
            header_count,
            interpreter,
        })
    }

    pub const fn elf_type(&self) -> u16 {
        self.elf_type
    }

    pub const fn entry(&self) -> u64 {
        self.entry
    }

    pub const fn program_offset(&self) -> u64 {
        self.program_offset
    }

    pub const fn program_header_count(&self) -> usize {
        self.header_count
    }

    pub fn program_headers(&self) -> &[ProgramHeader] {
        &self.headers[..self.header_count]
    }

    pub const fn interpreter(&self) -> Option<&'a str> {
        self.interpreter
    }

    pub fn segment_data(&self, header: ProgramHeader) -> Result<&'a [u8], ElfError> {
        file_range(self.image, header.offset, header.file_size)
    }
}

fn checked_file_range(image: &[u8], offset: u64, length: u64) -> Result<(), ElfError> {
    let end = offset
        .checked_add(length)
        .ok_or(ElfError::InvalidFileRange)?;
    let end = usize::try_from(end).map_err(|_| ElfError::InvalidFileRange)?;
    let offset = usize::try_from(offset).map_err(|_| ElfError::InvalidFileRange)?;
    if offset > end || end > image.len() {
        return Err(ElfError::InvalidFileRange);
    }
    Ok(())
}

fn file_range(image: &[u8], offset: u64, length: u64) -> Result<&[u8], ElfError> {
    checked_file_range(image, offset, length)?;
    let start = offset as usize;
    Ok(&image[start..start + length as usize])
}

fn read_u16(image: &[u8], offset: usize) -> Result<u16, ElfError> {
    let bytes = image.get(offset..offset + 2).ok_or(ElfError::Truncated)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(image: &[u8], offset: usize) -> Result<u32, ElfError> {
    let bytes = image.get(offset..offset + 4).ok_or(ElfError::Truncated)?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

fn read_u64(image: &[u8], offset: usize) -> Result<u64, ElfError> {
    let bytes = image.get(offset..offset + 8).ok_or(ElfError::Truncated)?;
    Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
}
