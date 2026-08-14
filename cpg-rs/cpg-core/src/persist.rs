//! Compact binary serialisation of the columnar store.
//!
//! Because the graph is already columns of primitives, persistence is close to
//! a memory dump: we length-prefix the string table and write each column as a
//! contiguous run. This is the on-disk corollary of flatgraph's design win —
//! the same layout that makes the in-memory graph small makes it cheap to save
//! and reload, so a tool builds the CPG once and reopens it for fast
//! incremental updates instead of reparsing from cold every run.
//!
//! The format is little-endian. New files use a versioned envelope around the
//! columnar payload so incompatible and corrupt files can be rejected before
//! graph construction.

/// Append-only little-endian writer.
#[derive(Default)]
pub struct ByteWriter {
    pub buf: Vec<u8>,
}

impl ByteWriter {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    pub fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub fn bytes(&mut self, b: &[u8]) {
        self.u32(b.len() as u32);
        self.buf.extend_from_slice(b);
    }
    /// `Option<u32>` with `u32::MAX` as the None sentinel.
    pub fn opt_u32(&mut self, v: Option<u32>) {
        self.u32(v.unwrap_or(u32::MAX));
    }
}

/// Cursor-based little-endian reader.
pub struct ByteReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

#[derive(Debug)]
pub struct DecodeError(pub String);

impl<'a> ByteReader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        ByteReader { buf, pos: 0 }
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let end = self.pos.checked_add(n).ok_or_else(|| {
            DecodeError(format!(
                "byte range overflow at {} (+{}), len {}",
                self.pos,
                n,
                self.buf.len()
            ))
        })?;
        if end > self.buf.len() {
            return Err(DecodeError(format!(
                "unexpected EOF at {} (+{}), len {}",
                self.pos,
                n,
                self.buf.len()
            )));
        }
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    pub fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16, DecodeError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    pub fn u32(&mut self) -> Result<u32, DecodeError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    pub fn i32(&mut self) -> Result<i32, DecodeError> {
        let b = self.take(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    pub fn u64(&mut self) -> Result<u64, DecodeError> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }
    pub fn bytes(&mut self) -> Result<&'a [u8], DecodeError> {
        let n = self.u32()? as usize;
        self.take(n)
    }
    pub fn opt_u32(&mut self) -> Result<Option<u32>, DecodeError> {
        let v = self.u32()?;
        Ok(if v == u32::MAX { None } else { Some(v) })
    }
    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }
    pub fn position(&self) -> usize {
        self.pos
    }
}
