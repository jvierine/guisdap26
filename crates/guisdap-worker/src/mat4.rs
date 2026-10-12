//! Strict MATLAB level-4 reader for EISCAT recordings. Column-major, split complex.
use anyhow::{bail, ensure, Context, Result};
use byteorder::{LittleEndian as LE, ReadBytesExt};
use guisdap_core::C;
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
};
#[derive(Debug)]
pub struct Array {
    pub rows: usize,
    pub cols: usize,
    pub real: Vec<f64>,
    pub imag: Vec<f64>,
}
pub fn parse(bytes: &[u8]) -> Result<BTreeMap<String, Array>> {
    let mut r = Cursor::new(bytes);
    let mut out = BTreeMap::new();
    while (r.position() as usize) < bytes.len() {
        let kind = r.read_i32::<LE>().context("MAT v4 header")?;
        ensure!(
            (0..=51).contains(&kind) && kind % 10 <= 1,
            "unsupported MAT v4 type/endian {kind}"
        );
        let rows = r.read_i32::<LE>()?;
        let cols = r.read_i32::<LE>()?;
        let complex = r.read_i32::<LE>()?;
        let name_len = r.read_i32::<LE>()?;
        ensure!(
            rows > 0 && cols > 0 && (1..=256).contains(&name_len) && (0..=1).contains(&complex),
            "invalid MAT dimensions/header"
        );
        let n = (rows as usize)
            .checked_mul(cols as usize)
            .context("MAT dimensions overflow")?;
        let width = match kind / 10 {
            0 => 8,
            1 | 2 => 4,
            3 | 4 => 2,
            5 => 1,
            _ => bail!("unsupported numeric type"),
        };
        let needed = n
            .checked_mul(width)
            .and_then(|v| v.checked_mul(1 + complex as usize))
            .and_then(|v| v.checked_add(name_len as usize))
            .context("MAT size overflow")?;
        ensure!(
            needed <= bytes.len() - r.position() as usize,
            "truncated MAT variable"
        );
        let mut name = vec![0; name_len as usize];
        r.read_exact(&mut name)?;
        ensure!(name.last() == Some(&0), "unterminated MAT variable name");
        name.pop();
        let name = String::from_utf8(name)?;
        let mut read_values = || -> Result<Vec<f64>> {
            (0..n)
                .map(|_| {
                    Ok(match kind / 10 {
                        0 => r.read_f64::<LE>()?,
                        1 => r.read_f32::<LE>()? as f64,
                        2 => r.read_i32::<LE>()? as f64,
                        3 => r.read_i16::<LE>()? as f64,
                        4 => r.read_u16::<LE>()? as f64,
                        5 => r.read_u8()? as f64,
                        _ => unreachable!(),
                    })
                })
                .collect()
        };
        let real = read_values()?;
        let imag = if complex == 1 { read_values()? } else { vec![] };
        ensure!(!out.contains_key(&name), "duplicate MAT variable {name}");
        out.insert(
            name,
            Array {
                rows: rows as usize,
                cols: cols as usize,
                real,
                imag,
            },
        );
    }
    Ok(out)
}
impl Array {
    pub fn complex(&self) -> Vec<C> {
        self.real
            .iter()
            .enumerate()
            .map(|(i, &re)| C::new(re, self.imag.get(i).copied().unwrap_or(0.0)))
            .collect()
    }
    pub fn text(&self) -> String {
        self.real
            .iter()
            .filter_map(|x| char::from_u32(*x as u32))
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_complex_and_rejects_truncation() {
        let mut b = vec![];
        for i in [10i32, 2, 1, 1, 2] {
            b.extend(i.to_le_bytes());
        }
        b.extend(b"z\0");
        for f in [1f32, 2., 3., 4.] {
            b.extend(f.to_le_bytes());
        }
        let a = parse(&b).unwrap();
        assert_eq!(a["z"].complex(), vec![C::new(1., 3.), C::new(2., 4.)]);
        b.pop();
        assert!(parse(&b).is_err());
    }
}
