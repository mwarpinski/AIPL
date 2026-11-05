//! The source line under an error: `FILE: LINE:COL: message` followed by the line and a caret under the column.
//!
//!   Error: prog.aipl: 1:30: Undefined variable 'y'
//!      1 | (module m (fn main [] -> i32 (return y)))
//!        |                               ^
//!
//! aipl_src/excerpt.aipl does the same in AIPL, byte for byte (tests/test_bootstrap.rs holds them equal), so the
//! rules here are the ones to keep in both: a line of the message that begins `FILE: LINE:COL: ` (the first place
//! `: DIGITS:DIGITS: ` occurs in it) gets the excerpt when FILE can be read, LINE is in it, COL is on the line or just
//! past its end, and the line is at most 200 bytes. Tabs show as spaces so that the caret stays under its column.

use std::fmt::Write;

const LONGEST: usize = 200;

/// `msg` with an excerpt after each of its lines that names a place in a readable file.
pub fn add(msg: &str) -> String {
    let mut out = String::with_capacity(msg.len());
    for (i, line) in msg.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line);
        if let Some(x) = excerpt(line) {
            out.push('\n');
            out.push_str(&x);
        }
    }
    out
}

/// (file, line, column) from the start of `l`.
fn locate(l: &str) -> Option<(&str, usize, usize)> {
    let b = l.as_bytes();
    let digits = |from: usize| {
        let mut k = from;
        while k < b.len() && b[k].is_ascii_digit() {
            k += 1;
        }
        k
    };
    for i in 1..b.len() {
        if b[i] != b':' || b.get(i + 1) != Some(&b' ') {
            continue;
        }
        let a = digits(i + 2);
        if a == i + 2 || a - i - 2 > 9 || b.get(a) != Some(&b':') {
            continue;
        }
        let c = digits(a + 1);
        if c == a + 1 || c - a - 1 > 9 || b.get(c) != Some(&b':') || b.get(c + 1) != Some(&b' ') {
            continue;
        }
        let line = l[i + 2..a].parse().ok()?;
        let col = l[a + 1..c].parse().ok()?;
        return Some((&l[..i], line, col));
    }
    None
}

fn excerpt(l: &str) -> Option<String> {
    let (file, line, col) = locate(l)?;
    let text = std::fs::read(file).ok()?;
    let src = text.split(|&c| c == b'\n').nth(line.checked_sub(1)?)?;
    let src = src.strip_suffix(b"\r").unwrap_or(src);
    if src.len() > LONGEST || col < 1 || col > src.len() + 1 {
        return None;
    }
    let src: String = std::str::from_utf8(src).ok()?.chars().map(|c| if c == '\t' { ' ' } else { c }).collect();
    let n = line.to_string();
    let mut out = String::new();
    let _ = write!(out, "  {n} | {src}\n  {} | {}^", " ".repeat(n.len()), " ".repeat(col - 1));
    Some(out)
}
