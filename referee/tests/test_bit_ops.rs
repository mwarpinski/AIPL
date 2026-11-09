//! The bit operations `rotl`, `rotr`, `bitnot`, `clz`, `ctz`, and `popcnt` on i32 and i64 (AIPL_SPEC.md 4.A): generated operands and
//! counts (every edge, random patterns, counts of 0, the width, past it, and negative) give what Rust's
//! `rotate_left`, `rotate_right`, `!`, `leading_zeros`, `trailing_zeros`, and `count_ones` give, in the VM, under aipl-run, as a native executable, and, where
//! an AArch64 machine can run it, as an AArch64 one; the `_c` forms (a constant count, which the native
//! backends lower on its own) and constants folded from literals do too; and the self-hosted toolchain
//! compiles the program to the Rust toolchain's bytes.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

#[path = "../../tests/arm64/mod.rs"]
mod arm64;

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_bitops_{}_{}", std::process::id(), name));
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

/// What operation `op` of tests/aipl/bits/ops.aipl prints for x and n.
fn expect(op: &str, x: i64, n: i64) -> String {
    let (xi, ni) = (x as i32 as u32, n as i32 as u32);
    let (xl, nl) = (x as u64, n as u32);
    let c = |name: &str| op.strip_prefix(name).and_then(|k| k.strip_prefix("_c")).and_then(|k| k.parse::<i64>().ok());
    if let Some(k) = c("rotl32") {
        return (xi.rotate_left(k as u32 & 31) as i32).to_string();
    }
    if let Some(k) = c("rotr32") {
        return (xi.rotate_right(k as u32 & 31) as i32).to_string();
    }
    if let Some(k) = c("rotl64") {
        return (xl.rotate_left(k as u32 & 63) as i64).to_string();
    }
    if let Some(k) = c("rotr64") {
        return (xl.rotate_right(k as u32 & 63) as i64).to_string();
    }
    match op {
        "rotl32" => (xi.rotate_left(ni & 31) as i32).to_string(),
        "rotr32" => (xi.rotate_right(ni & 31) as i32).to_string(),
        "rotl64" => (xl.rotate_left(nl & 63) as i64).to_string(),
        "rotr64" => (xl.rotate_right(nl & 63) as i64).to_string(),
        "bitnot32" => (!(x as i32)).to_string(),
        "bitnot64" => (!x).to_string(),
        "clz32" => xi.leading_zeros().to_string(),
        "ctz32" => xi.trailing_zeros().to_string(),
        "popcnt32" => xi.count_ones().to_string(),
        "clz64" => xl.leading_zeros().to_string(),
        "ctz64" => xl.trailing_zeros().to_string(),
        "popcnt64" => xl.count_ones().to_string(),
        "clz32_sum" => (xi.leading_zeros() + xi.wrapping_add(ni).leading_zeros()).to_string(),
        "popcnt64_sum" => (xl.count_ones() + (x.wrapping_add(n) as u64).count_ones()).to_string(),
        _ => panic!("{op}"),
    }
}

const CONSTANT_COUNTS: [&str; 14] = [
    "rotl32_c0", "rotl32_c5", "rotl32_c31", "rotl32_c32", "rotl32_c37", "rotr32_c1", "rotr32_c13", "rotr32_c-1", "rotl64_c0", "rotl64_c7",
    "rotl64_c63", "rotl64_c64", "rotr64_c1", "rotr64_c40",
];

/// The stdin lines and the answers they must produce.
fn cases() -> (String, String) {
    let mut rng = Rng(0x1234_5678_9ABC_DEF1);
    let mut xs: Vec<i64> = vec![
        0, 1, -1, 2, 0x7FFF_FFFF, -0x8000_0000, 0x8000_0001, 0xFF, 0xFF00_0000, 0x0123_4567_89AB_CDEF, i64::MAX, i64::MIN, 0x5555_5555_5555_5555,
        0x1_0000_0000, 0xFFFF_FFFF, 0x8000, 0x1_0000, 0xFFFF_FFFF_0000_0000u64 as i64,
    ];
    for _ in 0..40 {
        let r = rng.next();
        xs.push((r >> (rng.next() % 64)) as i64);
    }
    let mut counts: Vec<i64> = vec![0, 1, 2, 5, 7, 8, 13, 31, 32, 33, 40, 63, 64, 65, 127, 128, -1, -2, -31, -32, -33, -64, -65, i32::MAX as i64, i32::MIN as i64];
    for _ in 0..10 {
        counts.push(rng.next() as i64);
    }
    let (mut input, mut want) = (String::new(), String::new());
    for &x in &xs {
        for &n in &counts {
            for op in ["rotl32", "rotr32", "rotl64", "rotr64"] {
                input.push_str(&format!("{op} {x} {n}\n"));
                want.push_str(&format!("{}\n", expect(op, x, n)));
            }
        }
        for op in ["bitnot32", "bitnot64", "clz32", "ctz32", "popcnt32", "clz64", "ctz64", "popcnt64"] {
            input.push_str(&format!("{op} {x} 0\n"));
            want.push_str(&format!("{}\n", expect(op, x, 0)));
        }
        for &n in counts.iter().take(8) {
            for op in ["clz32_sum", "popcnt64_sum"] {
                input.push_str(&format!("{op} {x} {n}\n"));
                want.push_str(&format!("{}\n", expect(op, x, n)));
            }
        }
        for op in CONSTANT_COUNTS {
            input.push_str(&format!("{op} {x} 0\n"));
            want.push_str(&format!("{}\n", expect(op, x, 0)));
        }
    }
    (input, want)
}

fn run(cmd: &mut Command, input: &[u8]) -> Output {
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let input = input.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let out = child.wait_with_output().unwrap();
    let _ = writer.join();
    out
}

fn same(backend: &str, input: &str, want: &str, o: &Output) {
    assert!(o.status.success(), "{backend}: {}", String::from_utf8_lossy(&o.stderr));
    let got: String = String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.starts_with("[AIPL")).map(|l| format!("{l}\n")).collect();
    let lines: Vec<&str> = input.lines().collect();
    for (k, (g, w)) in got.lines().zip(want.lines()).enumerate() {
        assert_eq!(g, w, "{backend}: {}", lines[k]);
    }
    assert_eq!(got.lines().count(), want.lines().count(), "{backend}");
}

#[test]
fn rotations_and_bitnot_agree_with_rust_in_every_backend() {
    let dir = scratch("ops");
    let src = root().join("tests/aipl/bits/ops.aipl");
    let (wasm, exe, arm) = (dir.join("p.wasm"), dir.join("p"), dir.join("p_arm"));
    for args in [vec!["compile"], vec!["compile", "--exe"], vec!["compile", "--exe", "--target", "arm64"]] {
        let out = if args.len() == 1 {
            &wasm
        } else if args.len() == 2 {
            &exe
        } else {
            &arm
        };
        let o = Command::new(AIPL).args(&args).arg(&src).arg("-o").arg(out).output().unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    }
    let (input, want) = cases();
    same("aipl-run", &input, &want, &run(Command::new(RUNNER).arg(&wasm), input.as_bytes()));
    same("native", &input, &want, &run(&mut Command::new(&exe), input.as_bytes()));
    // the interpreter is slower: every 25th line
    let all: Vec<&str> = input.lines().collect();
    let wants: Vec<&str> = want.lines().collect();
    let few: Vec<usize> = (0..all.len()).step_by(25).collect();
    let (head, head_want): (String, String) = (few.iter().map(|&k| format!("{}\n", all[k])).collect(), few.iter().map(|&k| format!("{}\n", wants[k])).collect());
    same("VM", &head, &head_want, &run(Command::new(AIPL).arg("eval").arg(&src), head.as_bytes()));
    // AArch64, where a machine can run it
    match arm64::run(&std::fs::read(&arm).unwrap(), &[], input.as_bytes()) {
        Some(o) => same("AArch64", &input, &want, &o),
        None => eprintln!("no AArch64 machine to run on: not run"),
    }
    let module = aipl_core::resolver::Resolver::resolve(&src).unwrap();
    aipl_core::checker::TypeChecker::new().check_module(&module).unwrap();
    let rust = aipl_core::compiler::wasm::WasmCompiler::compile(&module).unwrap();
    assert!(aipl_core::selfhost::compile_with_aipl(&src).unwrap() == rust, "the self-hosted bytes differ");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A constant folded from literals is what the program computes at run time, and what Rust computes.
#[test]
fn constants_fold_rotations_and_bitnot_as_rust_computes_them() {
    let mut rng = Rng(0x0F0F_1234_ABCD_5678);
    let (mut consts, mut body, mut want) = (String::new(), String::new(), String::new());
    for k in 0..120 {
        let x = rng.next() >> (rng.next() % 64);
        let n = rng.next() % 140;
        let (x32, x64) = (x as u32 as i32, x as i64);
        let (op, ty, lit, count, value) = match k % 12 {
            6 => ("clz", "i32", format!("{x32}"), String::new(), (x32 as u32).leading_zeros() as i64),
            7 => ("ctz", "i32", format!("{x32}"), String::new(), (x32 as u32).trailing_zeros() as i64),
            8 => ("popcnt", "i32", format!("{x32}"), String::new(), (x32 as u32).count_ones() as i64),
            9 => ("clz", "i64", format!("{x64}i64"), String::new(), (x64 as u64).leading_zeros() as i64),
            10 => ("ctz", "i64", format!("{x64}i64"), String::new(), (x64 as u64).trailing_zeros() as i64),
            11 => ("popcnt", "i64", format!("{x64}i64"), String::new(), (x64 as u64).count_ones() as i64),
            0 => ("rotl", "i32", format!("{x32}"), format!("{n}"), (x32 as u32).rotate_left(n as u32 & 31) as i32 as i64),
            1 => ("rotr", "i32", format!("{x32}"), format!("{n}"), (x32 as u32).rotate_right(n as u32 & 31) as i32 as i64),
            2 => ("rotl", "i64", format!("{x64}i64"), format!("{n}i64"), (x64 as u64).rotate_left(n as u32 & 63) as i64),
            3 => ("rotr", "i64", format!("{x64}i64"), format!("{n}i64"), (x64 as u64).rotate_right(n as u32 & 63) as i64),
            4 => ("bitnot", "i32", format!("{x32}"), String::new(), !x32 as i64),
            5 => ("bitnot", "i64", format!("{x64}i64"), String::new(), !x64),
            _ => unreachable!(),
        };
        let expr = if count.is_empty() { format!("({op} {lit})") } else { format!("({op} {lit} {count})") };
        consts.push_str(&format!("  (const K{k:02}:{ty} {expr})\n"));
        body.push_str(&format!("    (println \"{{K{k:02}}}\")\n"));
        want.push_str(&format!("{value}\n"));
    }
    let dir = scratch("consts");
    let file = dir.join("consts.aipl");
    std::fs::write(&file, format!("(module consts\n{consts}  (fn main [] -> i32\n{body}    0))\n")).unwrap();
    let (wasm, exe) = (dir.join("p.wasm"), dir.join("p"));
    for (args, out) in [(vec!["compile"], &wasm), (vec!["compile", "--exe"], &exe)] {
        let o = Command::new(AIPL).args(&args).arg(&file).arg("-o").arg(out).output().unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    }
    same("VM", "", &want, &Command::new(AIPL).arg("eval").arg(&file).output().unwrap());
    same("aipl-run", "", &want, &Command::new(RUNNER).arg(&wasm).output().unwrap());
    same("native", "", &want, &Command::new(&exe).output().unwrap());
    let module = aipl_core::resolver::Resolver::resolve(&file).unwrap();
    aipl_core::checker::TypeChecker::new().check_module(&module).unwrap();
    let rust = aipl_core::compiler::wasm::WasmCompiler::compile(&module).unwrap();
    assert!(aipl_core::selfhost::compile_with_aipl(&file).unwrap() == rust, "the self-hosted bytes differ");
    let _ = std::fs::remove_dir_all(&dir);
}

/// What the checker says of the wrong operands, the same words from both toolchains.
#[test]
fn the_new_operations_are_refused_alike_by_both_toolchains() {
    let dir = scratch("errors");
    let me = dir.join("aipl");
    assert!(Command::new(AIPL).args(["compile", "--exe", "aipl_src/aipl.aipl", "-o"]).arg(&me).current_dir(root()).status().unwrap().success());
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&me, std::fs::Permissions::from_mode(0o755)).unwrap();
    let cases: &[(&str, &str)] = &[
        ("(rotl 1 2i64)", "Type mismatch in binary op: i32 vs i64"),
        ("(rotl 1.5 2.5)", "rotl is integer arithmetic (i32 or i64), got f64"),
        ("(rotr true false)", "rotr is integer arithmetic (i32 or i64), got bool"),
        ("(rotl 1)", "Arithmetic/bitwise opcode rotl requires 2 arguments"),
        ("(bitnot 1.5)", "bitnot is integer arithmetic (i32 or i64), got f64"),
        ("(bitnot 1 2)", "bitnot requires 1 argument (x: i32 or i64)"),
        ("(bitnot)", "bitnot requires 1 argument (x: i32 or i64)"),
        ("(bitnot true)", "bitnot is integer arithmetic (i32 or i64), got bool"),
        ("(clz 1.5)", "clz is integer arithmetic (i32 or i64), got f64"),
        ("(ctz 1 2)", "ctz requires 1 argument (x: i32 or i64)"),
        ("(popcnt)", "popcnt requires 1 argument (x: i32 or i64)"),
    ];
    for (k, (expr, msg)) in cases.iter().enumerate() {
        let file = dir.join(format!("e{k}.aipl"));
        std::fs::write(&file, format!("(module e\n  (fn main [] -> i32 (let x:i32 0) {expr} 0))\n")).unwrap();
        let (f, out) = (file.to_str().unwrap(), dir.join("x.wasm"));
        let r = Command::new(AIPL).args(["compile", f, "-o"]).arg(&out).output().unwrap();
        let a = Command::new(&me).args(["compile", f, "-o"]).arg(&out).output().unwrap();
        let (rerr, aerr) = (String::from_utf8_lossy(&r.stderr), String::from_utf8_lossy(&a.stderr));
        assert!(!r.status.success() && !a.status.success(), "{expr} compiled");
        assert!(rerr.contains(msg), "{expr}: {rerr}");
        assert_eq!(rerr.trim().trim_start_matches("Error: "), aerr.trim().trim_start_matches("Error: "), "{expr}: the toolchains differ");
    }
    // a constant expression takes them, and refuses the wrong types and counts
    let bad: &[(&str, &str)] = &[
        ("(rotl 1 2i64)", "rotl needs two operands of one type, got i32 and i64"),
        ("(rotl 1)", "rotl takes 2 operands, got 1"),
        ("(bitnot true)", "bitnot does not apply to bool"),
        ("(bitnot 1 2)", "bitnot takes 1 operand, got 2"),
        ("(rotl 1.5 2.5)", "rotl does not apply to f64"),
        ("(clz true)", "clz does not apply to bool"),
        ("(popcnt 1 2)", "popcnt takes 1 operand, got 2"),
    ];
    for (k, (expr, msg)) in bad.iter().enumerate() {
        let file = dir.join(format!("c{k}.aipl"));
        let ty = if expr.contains("1.5") { "f64" } else { "i32" };
        std::fs::write(&file, format!("(module c\n  (const BAD:{ty} {expr})\n  (fn main [] -> i32 0))\n")).unwrap();
        let (f, out) = (file.to_str().unwrap(), dir.join("x.wasm"));
        let r = Command::new(AIPL).args(["compile", f, "-o"]).arg(&out).output().unwrap();
        let a = Command::new(&me).args(["compile", f, "-o"]).arg(&out).output().unwrap();
        let (rerr, aerr) = (String::from_utf8_lossy(&r.stderr), String::from_utf8_lossy(&a.stderr));
        assert!(rerr.contains(msg), "{expr}: {rerr}");
        assert_eq!(rerr.trim().trim_start_matches("Error: "), aerr.trim().trim_start_matches("Error: "), "{expr}: the toolchains differ");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
