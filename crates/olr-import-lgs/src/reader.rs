use crate::LgsError;

/// Little-endian cursor over a byte slice. Every read reports truncation instead
/// of returning garbage, unlike OLX's `fread`-based loader.
pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub(crate) fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    pub(crate) fn bytes(&mut self, n: usize, what: &'static str) -> Result<&'a [u8], LgsError> {
        if self.remaining() < n {
            return Err(LgsError::Truncated { offset: self.pos, what });
        }
        let out = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(out)
    }

    pub(crate) fn i32(&mut self, what: &'static str) -> Result<i32, LgsError> {
        Ok(i32::from_le_bytes(self.bytes(4, what)?.try_into().expect("4 bytes")))
    }

    pub(crate) fn u32(&mut self, what: &'static str) -> Result<u32, LgsError> {
        Ok(u32::from_le_bytes(self.bytes(4, what)?.try_into().expect("4 bytes")))
    }

    pub(crate) fn f32(&mut self, what: &'static str) -> Result<f32, LgsError> {
        Ok(f32::from_le_bytes(self.bytes(4, what)?.try_into().expect("4 bytes")))
    }

    /// A 32-bit int read as a boolean, as OLX does with `fread_endian<int>` into a `bool`.
    pub(crate) fn flag(&mut self, what: &'static str) -> Result<bool, LgsError> {
        Ok(self.i32(what)? != 0)
    }

    /// OLX `readString`: one length byte, then that many bytes. Decoded as Latin-1
    /// so every byte maps to a character.
    pub(crate) fn pstring(&mut self, what: &'static str) -> Result<String, LgsError> {
        let len = self.bytes(1, what)?[0] as usize;
        Ok(latin1(self.bytes(len, what)?))
    }

    /// A fixed-size C string field: stops at the first NUL, because OLX files
    /// contain leftover memory after it.
    pub(crate) fn fixed_cstr(&mut self, len: usize, what: &'static str) -> Result<String, LgsError> {
        let raw = self.bytes(len, what)?;
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        Ok(latin1(&raw[..end]))
    }
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_little_endian_and_strings() {
        let data = [1, 0, 0, 0, 0, 0, 128, 63, 3, b'a', b'b', 0xE9, b'x', 0, b'g'];
        let mut r = Reader::new(&data);
        assert_eq!(r.i32("i").unwrap(), 1);
        assert_eq!(r.f32("f").unwrap(), 1.0);
        assert_eq!(r.pstring("s").unwrap(), "abé");
        assert_eq!(r.fixed_cstr(3, "c").unwrap(), "x");
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn truncation_reports_offset() {
        let mut r = Reader::new(&[5, b'a']);
        match r.pstring("name") {
            Err(LgsError::Truncated { offset: 1, what: "name" }) => {}
            other => panic!("unexpected {other:?}"),
        }
    }
}
