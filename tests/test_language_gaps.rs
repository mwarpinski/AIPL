//! The gaps closed on 2026-10-16 (AIPL_SPEC.md 4.A, 4.I, section 3): f32
//! literals and conversions, the 8- and 16-bit storage types, `inv`
//! contracts checked at entry and exit, and constant expressions. Each
//! program in tests/aipl/gaps prints the same in the VM, under aipl-run, and
//! as a native executable, and the self-hosted toolchain compiles it to the
//! bytes the Rust one does. (str.concat, std/date, and par.reduce test
//! themselves in aipl_src/test_suite.aipl; generic plain types are in
//! tests/test_interfaces.rs.)

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn program(name: &str) -> PathBuf {
    root().join("tests/aipl/gaps").join(format!("{name}.aipl"))
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_gaps_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.starts_with("[AIPL")).map(|l| format!("{l}\n")).collect()
}

/// The three runs of program `name` with `args`: VM, aipl-run, native.
fn run_all(name: &str, args: &[&str]) -> [Output; 3] {
    let src = program(name);
    let dir = scratch(&format!("{name}_{}", args.join("_")));
    let wasm = dir.join("p.wasm");
    let exe = dir.join("p");
    assert!(Command::new(AIPL).arg("compile").arg(&src).arg("-o").arg(&wasm).status().unwrap().success());
    assert!(Command::new(AIPL).args(["compile", "--exe"]).arg(&src).arg("-o").arg(&exe).status().unwrap().success());
    let out = [
        Command::new(AIPL).arg("eval").arg(&src).arg("--").args(args).output().unwrap(),
        Command::new(RUNNER).arg(&wasm).args(args).output().unwrap(),
        Command::new(&exe).args(args).output().unwrap(),
    ];
    let _ = std::fs::remove_dir_all(&dir);
    out
}

/// Program `name` prints `want` in every backend.
fn prints(name: &str, want: &str) {
    for (o, backend) in run_all(name, &[]).iter().zip(["VM", "aipl-run", "native"]) {
        assert!(o.status.success(), "{name} in {backend}: {}", String::from_utf8_lossy(&o.stderr));
        assert_eq!(stdout(o), want, "{name} in {backend}");
    }
}

/// Program `name` with `arg` fails in every backend: the VM's message
/// contains `vm`, the compiled ones' stderr contains `compiled`.
fn fails(name: &str, arg: &str, vm: &str, compiled: &str) {
    let [v, w, n] = run_all(name, &[arg]);
    let text = |o: &Output| format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert!(text(&v).contains(vm), "{name} {arg} in the VM: {}", text(&v));
    for (o, backend) in [(&w, "aipl-run"), (&n, "native")] {
        assert!(!o.status.success(), "{name} {arg} in {backend} succeeded");
        assert!(text(o).contains(compiled), "{name} {arg} in {backend}: {}", text(o));
    }
}

/// The self-hosted toolchain's bytes for `path` are the Rust toolchain's.
fn same_bytes(path: &Path) {
    let module = aipl_core::resolver::Resolver::resolve(path).unwrap();
    aipl_core::checker::TypeChecker::new().check_module(&module).unwrap();
    let rust = aipl_core::compiler::wasm::WasmCompiler::compile(&module).unwrap();
    assert_eq!(aipl_core::selfhost::compile_with_aipl(path).unwrap(), rust, "{}", path.display());
}

#[test]
fn f32_values_round_and_convert_alike_everywhere() {
    prints(
        "f32_values",
        "0.1+0.2 0.300000012\nbits 1050253722\nbits 1.0 1065353216\nthird 0.333333343\ndemote 3.141592741\n\
         from i32 16777216.000000000\nfrom i64 9007199254740992.000000000\nf64 from i32 -7.0\ntrunc f32 -2\n\
         trunc f64 2147483647\ntrunc f32 i64 10000000000\nreinterpret 3.141592741\nfield 3.000000000\nlt ok\neq ok\n",
    );
    same_bytes(&program("f32_values"));
}

/// Truncation to an integer traps on NaN and outside the integer's range,
/// at exactly wasm's bounds, natively too.
#[test]
fn f32_truncation_traps_at_the_wasm_bounds() {
    let nan = ("invalid conversion to integer (NaN)", "wasm trap: invalid conversion to integer");
    let big = ("integer overflow converting", "wasm trap: integer overflow");
    for (mode, (vm, compiled)) in [("1", nan), ("2", big), ("4", big), ("6", big), ("8", big), ("9", big), ("10", nan), ("11", big)] {
        fails("f32_traps", mode, vm, compiled);
    }
    // just inside: -2^31 - 0.9 as a double, -2^31 and -2^63 as floats, the largest float below 2^31
    for (mode, want) in [("3", "-2147483648\n"), ("5", "-2147483648\n"), ("7", "-9223372036854775808\n"), ("12", "2147483520\n")] {
        for (o, backend) in run_all("f32_traps", &[mode]).iter().zip(["VM", "aipl-run", "native"]) {
            assert_eq!(stdout(o), want, "mode {mode} in {backend}: {}", String::from_utf8_lossy(&o.stderr));
        }
    }
    same_bytes(&program("f32_traps"));
}

/// The self-hosted compiler rounds an f32 literal (nearest f64, then
/// nearest f32, ties to even) in integer arithmetic; Rust uses `as f32`.
/// Midpoints between floats, subnormals, and overflow to infinity agree.
#[test]
fn f32_literals_round_alike_in_both_toolchains() {
    let mut lits: Vec<String> = [
        "0.0", "-0.0", "16777217.0", "16777219.0", "3.4028235e38", "3.4028236e38", "3.4028235677973366e38", "1.0e39", "-1.0e39",
        "1.0e400", "1.17549435e-38", "1.4e-45", "7.0e-46", "7.006492321624085e-46", "7.006492321624086e-46", "1.0e-46", "1.0e-300",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    // midpoints between neighboring floats (exact as doubles), across the range
    let mut x: u32 = 1;
    while x < 0x7f7f_ffff {
        let mid = (f32::from_bits(x) as f64 + f32::from_bits(x + 1) as f64) / 2.0;
        lits.push(decimal(mid));
        x = x.wrapping_mul(3).wrapping_add(7919) % 0x7f7f_ffff;
        if lits.len() > 400 {
            break;
        }
    }
    let body: String = lits.iter().map(|l| format!("    (block (let _shown:i32 (i32.reinterpret_f32 {l}f32)) (println \"{{_shown}}\"))\n")).collect();
    let dir = scratch("literals");
    let file = dir.join("lits.aipl");
    std::fs::write(&file, format!("(module lits\n  (import io)\n  (fn main [] -> i32\n{body}    0))\n")).unwrap();
    same_bytes(&file);
    let _ = std::fs::remove_dir_all(&dir);
}

/// x as a float literal: its shortest decimal, with a '.' (`1e30` -> `1.0e30`).
fn decimal(x: f64) -> String {
    let s = format!("{x:?}");
    if s.contains('.') {
        s
    } else if let Some((m, e)) = s.split_once('e') {
        format!("{m}.0e{e}")
    } else {
        format!("{s}.0")
    }
}

#[test]
fn storage_types_read_as_i32_everywhere() {
    prints(
        "storage",
        "tag 44\ndelta -56\nlen 4464\noff -2\nsizeof Rec 12\nsizeof u16 2\nsum of bytes 488\ni16 -300\ngray 60\n\
         load16_u 65534\nload16_s -2\nload8_s -2\nload8 254\n",
    );
    same_bytes(&program("storage"));
}

/// (inv c) is checked when the function starts and again when it returns.
#[test]
fn invariants_are_checked_at_entry_and_exit() {
    fails("invariants", "-1", "Entry invariant failed in 'withdraw'", "Entry invariant failed in 'withdraw': (inv (gte (get a Acct.bal) 0))");
    fails("invariants", "10", "Exit invariant failed in 'withdraw'", "Exit invariant failed in 'withdraw': (inv (gte (get a Acct.bal) 0))");
    for (o, backend) in run_all("invariants", &["20"]).iter().zip(["VM", "aipl-run", "native"]) {
        assert!(o.status.success(), "{backend}: {}", String::from_utf8_lossy(&o.stderr));
    }
    same_bytes(&program("invariants"));
}

/// A contract's block may bind: its lets are locals in both code generators
/// (it verified and ran in the VM, but no compiled build had them).
#[test]
fn a_contract_may_bind_a_let() {
    prints("contract_lets", "half 5\n");
    fails("contract_lets", "7", "Pre-condition failed in 'halve'", "Pre-condition failed in 'halve': (req (block (let m:i32 (% n 2)) (eq m 0))) with n = 7");
    same_bytes(&program("contract_lets"));
}

/// Constants folded from other constants, in any order, with i32 wrapping
/// and f64 arithmetic.
#[test]
fn constant_expressions_fold_alike_everywhere() {
    prints("const_exprs", "MB 1048576\nMASK 4095\nBIG 12000000000\nWRAP -2147483648\nTWO_PI 6.283185307179586\n");
    same_bytes(&program("const_exprs"));
}
