pub mod wasm;

/// `--max-memory SIZE` as wasm pages: a number of bytes, or of K, M, or G
/// (binary: 1K is 1024 bytes), that is a whole number of 64 KiB pages from
/// 1M (the 16 pages a module starts with) to 2G (every address a
/// non-negative i32, which std and the compiler rely on: the limit may only
/// be lowered). aipl.aipl reads it the same way.
pub fn memory_pages(text: &str) -> Result<u32, String> {
    let bad = || format!("--max-memory {text}: expected a size from 1M to 2G in whole 64K pages, as 512M");
    let (digits, unit) = match text.char_indices().find(|(_, c)| !c.is_ascii_digit()) {
        Some((i, _)) => (&text[..i], &text[i..]),
        None => (text, ""),
    };
    let scale: u64 = match unit {
        "" => 1,
        "K" => 1 << 10,
        "M" => 1 << 20,
        "G" => 1 << 30,
        _ => return Err(bad()),
    };
    if digits.is_empty() || digits.len() > 10 {
        return Err(bad());
    }
    let bytes = digits.parse::<u64>().map_err(|_| bad())? * scale;
    if !bytes.is_multiple_of(65536) || !(16..=crate::vm::MAX_PAGES as u64).contains(&(bytes / 65536)) {
        return Err(bad());
    }
    Ok((bytes / 65536) as u32)
}

use crate::ast::Module;

pub trait Compiler {
    type Output;
    fn compile(&self, module: &Module) -> Result<Self::Output, String>;
}
