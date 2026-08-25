use super::address::{PAGE_SIZE, USER_ADDRESS_LIMIT};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UserAccess {
    Read,
    Write,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UserRangeError {
    LengthTooLarge,
    AddressOverflow,
    OutsideUserSpace,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UserRange {
    start: u64,
    end: u64,
    length: usize,
    access: UserAccess,
}

impl UserRange {
    pub const fn start(self) -> u64 {
        self.start
    }

    pub const fn end(self) -> u64 {
        self.end
    }

    pub const fn length(self) -> usize {
        self.length
    }

    pub const fn access(self) -> UserAccess {
        self.access
    }
}

pub fn checked_user_length(raw: u64, maximum: usize) -> Result<usize, UserRangeError> {
    usize::try_from(raw)
        .ok()
        .filter(|length| *length <= maximum && *length <= isize::MAX as usize)
        .ok_or(UserRangeError::LengthTooLarge)
}

pub fn checked_user_offset(address: u64, offset: usize) -> Result<u64, UserRangeError> {
    let offset = u64::try_from(offset).map_err(|_| UserRangeError::AddressOverflow)?;
    address
        .checked_add(offset)
        .ok_or(UserRangeError::AddressOverflow)
}

pub fn checked_user_range(
    address: u64,
    length: usize,
    access: UserAccess,
) -> Result<UserRange, UserRangeError> {
    if length > isize::MAX as usize {
        return Err(UserRangeError::LengthTooLarge);
    }
    if length == 0 {
        return Ok(UserRange {
            start: address,
            end: address,
            length,
            access,
        });
    }
    let length = u64::try_from(length).map_err(|_| UserRangeError::LengthTooLarge)?;
    let end = address
        .checked_add(length)
        .ok_or(UserRangeError::AddressOverflow)?;
    if address < PAGE_SIZE || end > USER_ADDRESS_LIMIT || end <= address {
        return Err(UserRangeError::OutsideUserSpace);
    }
    Ok(UserRange {
        start: address,
        end,
        length: length as usize,
        access,
    })
}
