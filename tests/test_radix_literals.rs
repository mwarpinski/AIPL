//! Hexadecimal (`0xEDB88320`) and binary (`0b1010`) integer literals: an optional sign, the prefix, and digits
//! with single `_` between them. The digits are the number's bit pattern: an i32 literal may use 32 bits (it
//! reads as signed or unsigned, like a decimal one), an `i64` literal 64. Generated literals in every spelling
//! print the values Rust computes for them in the VM, under aipl-run, and natively, in constants, constant
//! expressions, and enum values too, and the self-hosted toolchain compiles them to the Rust toolchain's
//! bytes; the text that is nearly a literal is refused in both with the same words.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_radix_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A small generator (xorshift64*), so that the cases are the same on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545F4914F6CDD1D)
    }
}

/// `digits` with a `_` after every group of `n` from the right, or none.
fn grouped(digits: &str, n: usize, underscores: bool) -> String {
    if !underscores {
        return digits.to_string();
    }
    let mut out = String::new();
    for (k, c) in digits.chars().enumerate() {
        if k > 0 && (digits.len() - k).is_multiple_of(n) {
            out.push('_');
        }
        out.push(c);
    }
    out
}

/// Some spelling of the 64-bit pattern `v` (of its low 32 bits unless `wide`), and the value it must have
/// (an i32 as signed, or the i64).
fn literal(rng: &mut Rng, v: u64, wide: bool) -> (String, i64) {
    let v = if wide { v } else { v & 0xFFFF_FFFF };
    // a plain literal reads as -2^31 to 2^32 - 1, so only a small one can be negative
    let negative = rng.next().is_multiple_of(4) && (wide || v <= 0x8000_0000);
    let underscores = rng.next().is_multiple_of(3);
    let sign = if negative { "-" } else if rng.next().is_multiple_of(11) { "+" } else { "" };
    let text = match rng.next() % 3 {
        0 => format!("0x{}", grouped(&format!("{v:X}"), 4, underscores)),
        1 => format!("0x{}", grouped(&format!("{v:x}"), 2, underscores)),
        _ => format!("0b{}", grouped(&format!("{v:b}"), 8, underscores)),
    };
    let signed = v as i64;
    let value = if negative { signed.wrapping_neg() } else { signed };
    let value = if wide { value } else { value as i32 as i64 };
    (format!("{sign}{text}{}", if wide { "i64" } else { "" }), value)
}

fn run_all(src: &Path) -> [Output; 3] {
    let dir = scratch("run");
    let (wasm, exe) = (dir.join("p.wasm"), dir.join("p"));
    let o = Command::new(AIPL).arg("compile").arg(src).arg("-o").arg(&wasm).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let o = Command::new(AIPL).args(["compile", "--exe"]).arg(src).arg("-o").arg(&exe).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let out = [
        Command::new(AIPL).arg("eval").arg(src).output().unwrap(),
        Command::new(RUNNER).arg(&wasm).output().unwrap(),
        Command::new(&exe).output().unwrap(),
    ];
    let _ = std::fs::remove_dir_all(&dir);
    out
}

fn printed(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.starts_with("[AIPL")).map(|l| format!("{l}\n")).collect()
}

#[test]
fn radix_literals_have_the_values_rust_gives_them_in_every_backend_and_toolchain() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    // edges, then random patterns of every width
    let mut patterns: Vec<u64> = vec![0, 1, 2, 7, 8, 0xF, 0x10, 0xFF, 0x100, 0x7FFF_FFFF, 0x8000_0000, 0xFFFF_FFFF, 0xEDB8_8320, 0x7FFF_FFFF_FFFF_FFFF, 0x8000_0000_0000_0000, u64::MAX];
    for _ in 0..150 {
        let r = rng.next();
        patterns.push(r >> (rng.next() % 64));
    }
    let mut body = String::new();
    let mut want = String::new();
    let mut consts = String::new();
    for (k, &p) in patterns.iter().enumerate() {
        let (lit, v) = literal(&mut rng, p, false);
        body.push_str(&format!("    (println \"{{{lit}}}\")\n"));
        want.push_str(&format!("{v}\n"));
        let (lit, v) = literal(&mut rng, p, true);
        body.push_str(&format!("    (println \"{{{lit}}}\")\n"));
        want.push_str(&format!("{v}\n"));
        // a constant, and a constant expression, of the same literal (a name starts with a capital)
        let (lit, v) = literal(&mut rng, p, false);
        consts.push_str(&format!("  (const C{k:03}:i32 {lit})\n  (const D{k:03}:i32 (bitor C{k:03} 0))\n"));
        body.push_str(&format!("    (println \"{{D{k:03}}}\")\n"));
        want.push_str(&format!("{v}\n"));
        let (lit, v) = literal(&mut rng, p, true);
        consts.push_str(&format!("  (const W{k:03}:i64 {lit})\n"));
        body.push_str(&format!("    (println \"{{W{k:03}}}\")\n"));
        want.push_str(&format!("{v}\n"));
    }
    // arithmetic on them, and enum values
    body.push_str("    (println \"{(+ 0x10 0b10)} {(bitand 0xF0F0F0F0 0xFF)} {(shru 0x80000000 31)} {(enum.ord Code.b)} {(enum.ord Code.c)}\")\n");
    want.push_str("18 240 1 17 5\n");
    let dir = scratch("values");
    let file = dir.join("lits.aipl");
    std::fs::write(&file, format!("(module lits\n  (enum Code [(a 0x10) b (c 0b101)])\n{consts}  (fn main [] -> i32\n{body}    0))\n")).unwrap();
    for (o, backend) in run_all(&file).iter().zip(["VM", "aipl-run", "native"]) {
        assert!(o.status.success(), "{backend}: {}", String::from_utf8_lossy(&o.stderr));
        let got = printed(o);
        for (k, (g, w)) in got.lines().zip(want.lines()).enumerate() {
            assert_eq!(g, w, "{backend}, line {k}");
        }
        assert_eq!(got.lines().count(), want.lines().count(), "{backend}");
    }
    // the self-hosted toolchain reads them as the Rust one does
    let module = aipl_core::resolver::Resolver::resolve(&file).unwrap();
    aipl_core::checker::TypeChecker::new().check_module(&module).unwrap();
    let rust = aipl_core::compiler::wasm::WasmCompiler::compile(&module).unwrap();
    assert!(aipl_core::selfhost::compile_with_aipl(&file).unwrap() == rust, "the self-hosted bytes differ");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn text_that_is_nearly_a_radix_literal_is_refused_alike_by_both_toolchains() {
    let dir = scratch("errors");
    let me = dir.join("aipl");
    assert!(Command::new(AIPL).args(["compile", "--exe", "aipl_src/aipl.aipl", "-o"]).arg(&me).current_dir(root()).status().unwrap().success());
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&me, std::fs::Permissions::from_mode(0o755)).unwrap();
    // (the body of a function returning i32, a part of the message)
    let cases: &[(&str, &str)] = &[
        ("0x100000000", "integer literal 4294967296 does not fit in 32 bits; write 4294967296i64 for an i64"),
        ("-0x100000000", "integer literal -4294967296 does not fit in 32 bits; write -4294967296i64 for an i64"),
        ("0b1_0000_0000_0000_0000_0000_0000_0000_0000", "does not fit in 32 bits"),
        ("0x", "0x"),
        ("0b", "0b"),
        ("0xG1", "0xG1"),
        ("0b102", "0b102"),
        ("0x_1", "0x_1"),
        ("0x1_", "0x1_"),
        ("0x1__2", "0x1__2"),
        ("0X1F", "0X1F"),
        ("0x1.5", "0x1.5"),
        ("0x10000000000000000i64", "0x10000000000000000i64"),
        ("0b11111111111111111111111111111111111111111111111111111111111111111i64", "0b1111"),
        ("0x8000000000000000", "0x8000000000000000"),
    ];
    for (k, (body, msg)) in cases.iter().enumerate() {
        let file = dir.join(format!("e{k}.aipl"));
        std::fs::write(&file, format!("(module e\n  (fn main [] -> i32 {body}))\n")).unwrap();
        let (f, out) = (file.to_str().unwrap(), dir.join("x.wasm"));
        let r = Command::new(AIPL).args(["compile", f, "-o"]).arg(&out).output().unwrap();
        let a = Command::new(&me).args(["compile", f, "-o"]).arg(&out).output().unwrap();
        let (rerr, aerr) = (String::from_utf8_lossy(&r.stderr), String::from_utf8_lossy(&a.stderr));
        assert!(!r.status.success() && !a.status.success(), "{body} compiled");
        assert!(rerr.contains(msg), "{body}: {rerr}");
        assert_eq!(rerr.trim().trim_start_matches("Error: "), aerr.trim().trim_start_matches("Error: "), "{body}: the toolchains differ");
    }
    // a constant too wide for an i32 is refused, in both
    let file = dir.join("c.aipl");
    std::fs::write(&file, "(module c\n  (const BAD:i32 0x1_0000_0000)\n  (fn main [] -> i32 BAD))\n").unwrap();
    let (f, out) = (file.to_str().unwrap(), dir.join("x.wasm"));
    let r = Command::new(AIPL).args(["compile", f, "-o"]).arg(&out).output().unwrap();
    let a = Command::new(&me).args(["compile", f, "-o"]).arg(&out).output().unwrap();
    let (rerr, aerr) = (String::from_utf8_lossy(&r.stderr), String::from_utf8_lossy(&a.stderr));
    assert!(rerr.contains("constant 'BAD' is declared i32 but its value is not an i32 literal"), "{rerr}");
    assert_eq!(rerr.trim().trim_start_matches("Error: "), aerr.trim().trim_start_matches("Error: "));
    let _ = std::fs::remove_dir_all(&dir);
}
