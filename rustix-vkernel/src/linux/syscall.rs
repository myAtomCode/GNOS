use super::{Errno, Result, MAX_ERRNO};

const MIN_ERROR_RETURN: u64 = (-(MAX_ERRNO as i64)) as u64;

pub const fn encode(result: Result<u64>) -> u64 {
    match result {
        Ok(value) => value,
        Err(error) => error.return_value(),
    }
}

pub const fn is_error(value: u64) -> bool {
    value >= MIN_ERROR_RETURN
}

pub const fn decode(value: u64) -> Result<u64> {
    if is_error(value) {
        let code = -(value as i64) as i32;
        match Errno::from_code(code) {
            Some(error) => Err(error),
            None => Err(Errno::EINVAL),
        }
    } else {
        Ok(value)
    }
}

pub fn dispatch(_number: u64, _arguments: [u64; 6]) -> u64 {
    encode(Err(Errno::ENOSYS))
}
