//! Just enough of Thrift's compact protocol to check a Parquet file before it is decoded: the
//! footer's nesting depth, and each page header's sizes (so a part that would decompress to
//! more than its limit is refused before anything is decompressed or allocated).

/// Deepest nesting of Thrift structs and containers accepted in a footer or page header.
pub const MAX_DEPTH: usize = 32;

pub struct Reader<'a> {
    b: &'a [u8],
    pub pos: usize,
}

const STOP: u8 = 0;
const TRUE: u8 = 1;
const FALSE: u8 = 2;
const BYTE: u8 = 3;
const I16: u8 = 4;
const I32: u8 = 5;
const I64: u8 = 6;
const DOUBLE: u8 = 7;
const BINARY: u8 = 8;
const LIST: u8 = 9;
const SET: u8 = 10;
const MAP: u8 = 11;
const STRUCT: u8 = 12;

impl<'a> Reader<'a> {
    pub fn new(b: &'a [u8]) -> Reader<'a> {
        Reader { b, pos: 0 }
    }

    fn byte(&mut self) -> Result<u8, String> {
        let x = *self.b.get(self.pos).ok_or("thrift: unexpected end")?;
        self.pos += 1;
        Ok(x)
    }

    fn varint(&mut self) -> Result<u64, String> {
        let mut out = 0u64;
        for shift in (0..70).step_by(7) {
            let x = self.byte()?;
            if shift == 63 && x > 1 {
                return Err("thrift: varint too long".into());
            }
            out |= ((x & 0x7f) as u64) << shift;
            if x & 0x80 == 0 {
                return Ok(out);
            }
        }
        Err("thrift: varint too long".into())
    }

    fn zigzag(&mut self) -> Result<i64, String> {
        let u = self.varint()?;
        Ok(((u >> 1) as i64) ^ -((u & 1) as i64))
    }

    fn skip_bytes(&mut self, n: u64) -> Result<(), String> {
        let n = usize::try_from(n).map_err(|_| "thrift: length too large")?;
        if self.b.len() - self.pos < n {
            return Err("thrift: length beyond the data".into());
        }
        self.pos += n;
        Ok(())
    }

    /// Skip a value of a type, at most `MAX_DEPTH` deep.
    pub fn skip(&mut self, ty: u8, depth: usize) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err("thrift: nested too deeply".into());
        }
        match ty {
            TRUE | FALSE => Ok(()),
            BYTE => self.byte().map(|_| ()),
            I16 | I32 | I64 => self.varint().map(|_| ()),
            DOUBLE => self.skip_bytes(8),
            BINARY => {
                let n = self.varint()?;
                self.skip_bytes(n)
            }
            LIST | SET => {
                let h = self.byte()?;
                let et = h & 0x0f;
                let size = match h >> 4 {
                    15 => self.varint()?,
                    s => s as u64,
                };
                if size > (self.b.len() - self.pos) as u64 && !matches!(et, TRUE | FALSE) {
                    return Err("thrift: list longer than the data".into());
                }
                for _ in 0..size {
                    if matches!(et, TRUE | FALSE) {
                        self.byte()?;
                    } else {
                        self.skip(et, depth + 1)?;
                    }
                }
                Ok(())
            }
            MAP => {
                let size = self.varint()?;
                if size == 0 {
                    return Ok(());
                }
                if size > (self.b.len() - self.pos) as u64 {
                    return Err("thrift: map longer than the data".into());
                }
                let kv = self.byte()?;
                for _ in 0..size {
                    self.skip(kv >> 4, depth + 1)?;
                    self.skip(kv & 0x0f, depth + 1)?;
                }
                Ok(())
            }
            STRUCT => self.fields(depth + 1, |_, _, _, _| Ok(false)),
            t => Err(format!("thrift: unknown type {t}")),
        }
    }

    /// Walk a struct's fields: `f(id, type, reader, depth)` returns Ok(true) when it read the
    /// value itself, else the value is skipped.
    pub fn fields(
        &mut self,
        depth: usize,
        mut f: impl FnMut(i16, u8, &mut Reader<'a>, usize) -> Result<bool, String>,
    ) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err("thrift: nested too deeply".into());
        }
        let mut last: i16 = 0;
        loop {
            let h = self.byte()?;
            let ty = h & 0x0f;
            if ty == STOP {
                return Ok(());
            }
            let id = match h >> 4 {
                0 => i16::try_from(self.zigzag()?).map_err(|_| "thrift: bad field id")?,
                d => last.checked_add(d as i16).ok_or("thrift: bad field id")?,
            };
            last = id;
            if !f(id, ty, self, depth)? {
                self.skip(ty, depth)?;
            }
        }
    }

    /// An i32 field's value (zigzag varint).
    pub fn i32(&mut self) -> Result<i32, String> {
        i32::try_from(self.zigzag()?).map_err(|_| "thrift: i32 out of range".into())
    }
}

/// A page header's uncompressed and compressed sizes, its value count (data and dictionary
/// pages; 0 for others), and the header's own length.
pub fn page_header(b: &[u8]) -> Result<(i64, i64, i64, usize), String> {
    let mut r = Reader::new(b);
    let (mut un, mut co, mut nv) = (-1i64, -1i64, 0i64);
    r.fields(0, |id, ty, r, depth| {
        Ok(match (id, ty) {
            (2, I32) => {
                un = r.i32()? as i64;
                true
            }
            (3, I32) => {
                co = r.i32()? as i64;
                true
            }
            // DataPageHeader, DictionaryPageHeader, DataPageHeaderV2: field 1 is num_values.
            (5, STRUCT) | (7, STRUCT) | (8, STRUCT) => {
                r.fields(depth + 1, |id, ty, r, _| {
                    Ok(if (id, ty) == (1, I32) {
                        nv = r.i32()? as i64;
                        true
                    } else {
                        false
                    })
                })?;
                true
            }
            _ => false,
        })
    })?;
    if un < 0 || co < 0 || nv < 0 {
        return Err("parquet page header without its sizes".into());
    }
    Ok((un, co, nv, r.pos))
}

/// Check that a Thrift struct (a Parquet footer) is well formed and nested at most
/// [`MAX_DEPTH`] deep, without building it.
pub fn check_struct(b: &[u8]) -> Result<(), String> {
    let mut r = Reader::new(b);
    r.skip(STRUCT, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_headers_and_depth() {
        // A page header: type 0, uncompressed 100, compressed 40, then a nested empty struct.
        // Field headers: delta 1 type i32 (0x15), zigzag 0; delta 1 i32 (0x15), zigzag(100)=200;
        // delta 1 i32, zigzag(40)=80; delta 2 struct (0x2c), stop; stop.
        let b = [0x15, 0x00, 0x15, 0xc8, 0x01, 0x15, 0x50, 0x2c, 0x00, 0x00];
        assert_eq!(page_header(&b).unwrap(), (100, 40, 0, 10));
        assert!(page_header(&b[..5]).is_err());
        // Structs nested 40 deep are refused.
        let mut deep = vec![0x1c; 40];
        deep.extend(vec![0x00; 41]);
        assert!(check_struct(&deep).unwrap_err().contains("deeply"));
        let mut ok = vec![0x1c; 10];
        ok.extend(vec![0x00; 11]);
        assert!(check_struct(&ok).is_ok());
        // A list claiming more elements than there are bytes.
        let b = [0x19, 0xf5, 0xff, 0xff, 0xff, 0x0f, 0x00];
        assert!(check_struct(&b).is_err());
    }
}
