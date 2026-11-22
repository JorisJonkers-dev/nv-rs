// SPDX-License-Identifier: GPL-3.0-only
//! The `printf` family's format engine over the guest's argument list (a `va_list` is a pointer
//! to the arguments on the stack).

use crate::x86::Stop;
use engine_core::prelude::*;

fn fault(addr: u32) -> Stop {
    Stop::Fault { addr, write: false }
}

struct Args<'a> {
    e: &'a Engine,
    at: u32,
}

impl Args<'_> {
    fn u32(&mut self) -> Result<u32, Stop> {
        let mut b = [0u8; 4];
        self.e
            .mem
            .try_read(self.at, &mut b)
            .map_err(|_| fault(self.at))?;
        self.at = self.at.wrapping_add(4);
        Ok(u32::from_le_bytes(b))
    }
    fn u64(&mut self) -> Result<u64, Stop> {
        let mut b = [0u8; 8];
        self.e
            .mem
            .try_read(self.at, &mut b)
            .map_err(|_| fault(self.at))?;
        self.at = self.at.wrapping_add(8);
        Ok(u64::from_le_bytes(b))
    }
}

fn pad(mut s: Vec<u8>, width: usize, left: bool, zero: bool) -> Vec<u8> {
    if s.len() >= width {
        return s;
    }
    let n = width - s.len();
    if left {
        s.extend(std::iter::repeat(b' ').take(n));
        s
    } else if zero {
        let sign = usize::from(matches!(s.first(), Some(b'-' | b'+' | b' ')));
        let mut o = s[..sign].to_vec();
        o.extend(std::iter::repeat(b'0').take(n));
        o.extend_from_slice(&s[sign..]);
        o
    } else {
        let mut o = vec![b' '; n];
        o.extend(s);
        o
    }
}

fn exp_form(v: f64, prec: usize, upper: bool) -> String {
    let s = format!("{v:.prec$e}");
    let (m, x) = s.split_once('e').unwrap_or((&s, "0"));
    let x: i32 = x.parse().unwrap_or(0);
    let r = format!("{m}e{}{:02}", if x < 0 { '-' } else { '+' }, x.abs());
    if upper {
        r.to_uppercase()
    } else {
        r
    }
}

fn general(v: f64, prec: usize, upper: bool) -> String {
    let p = if prec == 0 { 1 } else { prec };
    if v == 0.0 {
        return "0".into();
    }
    let x = v.abs().log10().floor() as i32;
    let s = if x < -4 || x >= p as i32 {
        let t = exp_form(v, p - 1, upper);
        let (m, e) = t
            .split_once(if upper { 'E' } else { 'e' })
            .unwrap_or((&t, ""));
        let m = if m.contains('.') {
            m.trim_end_matches('0').trim_end_matches('.')
        } else {
            m
        };
        format!("{m}{}{e}", if upper { 'E' } else { 'e' })
    } else {
        let t = format!("{v:.*}", (p as i32 - 1 - x).max(0) as usize);
        if t.contains('.') {
            t.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            t
        }
    };
    s
}

/// Formats `fmt` with the arguments at `va`.
pub fn format(e: &Engine, fmt: &[u8], va: u32) -> Result<Vec<u8>, Stop> {
    let mut a = Args { e, at: va };
    let mut out = Vec::new();
    let mut i = 0;
    while i < fmt.len() {
        let c = fmt[i];
        i += 1;
        if c != b'%' {
            out.push(c);
            continue;
        }
        let (mut left, mut plus, mut space, mut zero, mut alt) =
            (false, false, false, false, false);
        while let Some(&f) = fmt.get(i) {
            match f {
                b'-' => left = true,
                b'+' => plus = true,
                b' ' => space = true,
                b'0' => zero = true,
                b'#' => alt = true,
                _ => break,
            }
            i += 1;
        }
        let mut width = 0usize;
        if fmt.get(i) == Some(&b'*') {
            let w = a.u32()? as i32;
            if w < 0 {
                left = true;
            }
            width = w.unsigned_abs() as usize;
            i += 1;
        } else {
            while let Some(d @ b'0'..=b'9') = fmt.get(i) {
                width = width * 10 + (d - b'0') as usize;
                i += 1;
            }
        }
        let mut prec: Option<usize> = None;
        if fmt.get(i) == Some(&b'.') {
            i += 1;
            let mut p = 0usize;
            if fmt.get(i) == Some(&b'*') {
                p = a.u32()? as usize;
                i += 1;
            } else {
                while let Some(d @ b'0'..=b'9') = fmt.get(i) {
                    p = p * 10 + (d - b'0') as usize;
                    i += 1;
                }
            }
            prec = Some(p);
        }
        let mut long = 0u8; // 1: l / z / t, 2: ll / j / I64, 3: h, 4: hh
        loop {
            match fmt.get(i) {
                Some(b'l') => long = if long == 1 { 2 } else { 1 },
                Some(b'h') => long = if long == 3 { 4 } else { 3 },
                Some(b'z' | b't' | b'L' | b'w') => long = 1,
                Some(b'j') => long = 2,
                Some(b'I') => {
                    if fmt[i..].starts_with(b"I64") {
                        long = 2;
                        i += 2;
                    } else if fmt[i..].starts_with(b"I32") {
                        i += 2;
                    }
                }
                _ => break,
            }
            i += 1;
        }
        let Some(&conv) = fmt.get(i) else { break };
        i += 1;
        let body: Vec<u8> = match conv {
            b'%' => b"%".to_vec(),
            b'c' => vec![a.u32()? as u8],
            b's' => {
                let p = a.u32()?;
                let mut s = if p == 0 {
                    b"(null)".to_vec()
                } else {
                    e.mem.cstr(p)
                };
                if let Some(n) = prec {
                    s.truncate(n);
                }
                s
            }
            b'd' | b'i' | b'u' | b'x' | b'X' | b'o' | b'p' => {
                let signed = matches!(conv, b'd' | b'i');
                let v: i128 = if conv == b'p' {
                    i128::from(a.u32()?)
                } else if long == 2 {
                    let w = a.u64()?;
                    if signed {
                        i128::from(w as i64)
                    } else {
                        i128::from(w)
                    }
                } else {
                    let w = a.u32()?;
                    match (signed, long) {
                        (true, 3) => i128::from(w as i16),
                        (true, 4) => i128::from(w as i8),
                        (true, _) => i128::from(w as i32),
                        (false, 3) => i128::from(w as u16),
                        (false, 4) => i128::from(w as u8),
                        (false, _) => i128::from(w),
                    }
                };
                let mut digits = match conv {
                    b'x' | b'p' => format!("{:x}", v.unsigned_abs()),
                    b'X' => format!("{:X}", v.unsigned_abs()),
                    b'o' => format!("{:o}", v.unsigned_abs()),
                    _ => v.unsigned_abs().to_string(),
                };
                if conv == b'p' {
                    digits = format!("{digits:0>8}").to_uppercase();
                }
                if let Some(p) = prec {
                    while digits.len() < p {
                        digits.insert(0, '0');
                    }
                }
                let mut s = String::new();
                if signed && v < 0 {
                    s.push('-');
                } else if signed && plus {
                    s.push('+');
                } else if signed && space {
                    s.push(' ');
                }
                if alt && v != 0 && matches!(conv, b'x' | b'X') {
                    s.push_str(if conv == b'x' { "0x" } else { "0X" });
                }
                s.push_str(&digits);
                let z = zero && !left && prec.is_none();
                out.extend(pad(s.into_bytes(), width, left, z));
                continue;
            }
            b'f' | b'F' | b'e' | b'E' | b'g' | b'G' | b'a' | b'A' => {
                let v = f64::from_bits(a.u64()?);
                let p = prec.unwrap_or(6);
                let mut s = if v.is_nan() {
                    "nan".to_string()
                } else if v.is_infinite() {
                    if v < 0.0 { "-inf" } else { "inf" }.to_string()
                } else {
                    match conv {
                        b'f' | b'F' => format!("{v:.p$}"),
                        b'e' | b'E' => exp_form(v, p, conv == b'E'),
                        _ => general(v, p, conv == b'G'),
                    }
                };
                if v >= 0.0 && plus {
                    s.insert(0, '+');
                } else if v >= 0.0 && space {
                    s.insert(0, ' ');
                }
                let z = zero && !left && v.is_finite();
                out.extend(pad(s.into_bytes(), width, left, z));
                continue;
            }
            b'n' => {
                a.u32()?;
                Vec::new()
            }
            other => vec![b'%', other],
        };
        out.extend(pad(body, width, left, false));
    }
    Ok(out)
}
