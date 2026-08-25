use core::fmt;

#[derive(Clone, Copy)]
pub struct InlineString<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> InlineString<N> {
    pub const fn new() -> Self {
        Self {
            buf: [0; N],
            len: 0,
        }
    }

    pub const fn from_str(text: &str) -> Self {
        let bytes = text.as_bytes();
        let mut value = Self::new();
        let mut index = 0;
        while index < bytes.len() && index < N {
            value.buf[index] = bytes[index];
            index += 1;
        }
        value.len = index;
        value
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn set(&mut self, text: &str) {
        self.clear();
        let _ = self.push_str(text);
    }

    pub fn push_byte(&mut self, byte: u8) -> Result<(), ()> {
        // A single non-ASCII byte cannot form valid UTF-8 by itself. Multi-byte
        // text must enter through push_str, which preserves this type's invariant.
        if !byte.is_ascii() || self.len >= N {
            return Err(());
        }
        self.buf[self.len] = byte;
        self.len += 1;
        Ok(())
    }

    pub fn push_str(&mut self, text: &str) -> Result<(), ()> {
        let bytes = text.as_bytes();
        if self.len + bytes.len() > N {
            return Err(());
        }
        self.buf[self.len..self.len + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len();
        Ok(())
    }

    pub fn pop(&mut self) -> Option<u8> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        Some(self.buf[self.len])
    }

    pub fn as_str(&self) -> &str {
        // push_str accepts UTF-8 and push_byte only accepts ASCII.
        unsafe { core::str::from_utf8_unchecked(&self.buf[..self.len]) }
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn truncate(&mut self, new_len: usize) {
        assert!(new_len <= self.len);
        assert!(self.as_str().is_char_boundary(new_len));
        self.len = new_len;
    }
}

impl<const N: usize> Default for InlineString<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> fmt::Write for InlineString<N> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.push_str(s).map_err(|_| fmt::Error)
    }
}
