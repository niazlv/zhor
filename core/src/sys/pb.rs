//! Минимальный читатель protobuf (wire format) без кодогенерации: varint, fixed32/64,
//! length-delimited. Для `dumpsys batterystats --proto` — сводки без истории (~270 КБ вместо
//! 13 МБ текста `-c`, в 4–5 раз дешевле для system_server).

#[derive(Debug, Clone, Copy)]
pub enum Val<'a> {
    Int(u64),
    Bytes(&'a [u8]),
    F64(f64),
    F32(f32),
}

impl<'a> Val<'a> {
    pub fn int(&self) -> u64 {
        match *self {
            Val::Int(v) => v,
            Val::F64(v) => v as u64,
            Val::F32(v) => v as u64,
            Val::Bytes(_) => 0,
        }
    }
    /// int32/int64 со знаком (proto2 кодирует отрицательные как 10-байтный varint)
    pub fn i64(&self) -> i64 {
        self.int() as i64
    }
    pub fn f64(&self) -> f64 {
        match *self {
            Val::F64(v) => v,
            Val::F32(v) => v as f64,
            Val::Int(v) => v as f64,
            Val::Bytes(_) => 0.0,
        }
    }
    pub fn bytes(&self) -> &'a [u8] {
        match *self {
            Val::Bytes(b) => b,
            _ => &[],
        }
    }
    pub fn str(&self) -> String {
        String::from_utf8_lossy(self.bytes()).into_owned()
    }
}

/// Итератор полей сообщения: (номер поля, значение). Битые данные — конец итерации.
pub struct Fields<'a> {
    b: &'a [u8],
    i: usize,
}

pub fn fields(b: &[u8]) -> Fields<'_> {
    Fields { b, i: 0 }
}

fn varint(b: &[u8], i: &mut usize) -> Option<u64> {
    let mut v: u64 = 0;
    let mut shift = 0;
    loop {
        let c = *b.get(*i)?;
        *i += 1;
        v |= ((c & 0x7f) as u64) << shift;
        if c & 0x80 == 0 {
            return Some(v);
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
}

impl<'a> Iterator for Fields<'a> {
    type Item = (u32, Val<'a>);
    fn next(&mut self) -> Option<Self::Item> {
        if self.i >= self.b.len() {
            return None;
        }
        let key = varint(self.b, &mut self.i)?;
        let (num, wt) = ((key >> 3) as u32, key & 7);
        let v = match wt {
            0 => Val::Int(varint(self.b, &mut self.i)?),
            1 => {
                let s = self.b.get(self.i..self.i + 8)?;
                self.i += 8;
                Val::F64(f64::from_le_bytes(s.try_into().ok()?))
            }
            2 => {
                let n = varint(self.b, &mut self.i)? as usize;
                let s = self.b.get(self.i..self.i.checked_add(n)?)?;
                self.i += n;
                Val::Bytes(s)
            }
            5 => {
                let s = self.b.get(self.i..self.i + 4)?;
                self.i += 4;
                Val::F32(f32::from_le_bytes(s.try_into().ok()?))
            }
            _ => return None,
        };
        Some((num, v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wire() {
        // {1: 150, 2: "hi", 3: {1: 7}, 4: -1 (int64)}
        let mut b = vec![0x08, 0x96, 0x01, 0x12, 0x02, b'h', b'i', 0x1a, 0x02, 0x08, 0x07, 0x20];
        b.extend_from_slice(&[0xff; 9]);
        b.push(0x01);
        let f: Vec<(u32, Val)> = fields(&b).collect();
        assert_eq!(f[0].0, 1);
        assert_eq!(f[0].1.int(), 150);
        assert_eq!(f[1].1.str(), "hi");
        let inner: Vec<(u32, Val)> = fields(f[2].1.bytes()).collect();
        assert_eq!(inner[0].1.int(), 7);
        assert_eq!(f[3].1.i64(), -1);
    }
}
