//! The formatting forms (AIPL_SPEC.md 4.K) and std/show: tests/aipl/formats
//! prints the same in the VM, under aipl-run, and natively, and the
//! self-hosted toolchain compiles it to the Rust toolchain's bytes; every
//! error has the same message in both toolchains; and std/show writes what
//! Python writes for 1,455 generated values and formats
//! (tests/aipl/show, from tools/show_vectors.py); and every other value but
//! void and a function reference prints through a printer generated for its
//! type (tests/aipl/formats/values.aipl).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_formats_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.starts_with("[AIPL")).map(|l| format!("{l}\n")).collect()
}

/// The program's standard output in the VM, under aipl-run, and natively.
fn run_all(src: &Path, name: &str) -> [Output; 3] {
    let dir = scratch(name);
    let wasm = dir.join("p.wasm");
    let exe = dir.join("p");
    assert!(Command::new(AIPL).arg("compile").arg(src).arg("-o").arg(&wasm).status().unwrap().success());
    assert!(Command::new(AIPL).args(["compile", "--exe"]).arg(src).arg("-o").arg(&exe).status().unwrap().success());
    let out = [
        Command::new(AIPL).arg("eval").arg(src).output().unwrap(),
        Command::new(RUNNER).arg(&wasm).output().unwrap(),
        Command::new(&exe).output().unwrap(),
    ];
    let _ = std::fs::remove_dir_all(&dir);
    out
}

fn same_bytes(path: &Path) {
    let module = aipl_core::resolver::Resolver::resolve(path).unwrap();
    aipl_core::checker::TypeChecker::new().check_module(&module).unwrap();
    let rust = aipl_core::compiler::wasm::WasmCompiler::compile(&module).unwrap();
    assert_eq!(aipl_core::selfhost::compile_with_aipl(path).unwrap(), rust, "{}", path.display());
}

#[test]
fn the_forms_print_alike_in_every_backend_and_toolchain() {
    let src = root().join("tests/aipl/formats/main.aipl");
    let want = "Ada ran 3 tests, mean 2.72\n\
                s = [    3|1.5|{}|63|blue] len 20\n\
                row 1: 001;row 2: 004;row 3: 009;\n\
                no newline true 0.1 1e+16 100.0 0.3333333333333333 -0000007 -2.5 ####FF 101\n\
                lib: 42 7 5\n\
                x: boxed\n\
                item 1 of 2\n\
                item 2 of 2\n\
                color green      red| 3\n";
    for (o, backend) in run_all(&src, "main").iter().zip(["VM", "aipl-run", "native"]) {
        assert!(o.status.success(), "{backend}: {}", String::from_utf8_lossy(&o.stderr));
        assert_eq!(stdout(o), want, "{backend}");
        assert!(String::from_utf8_lossy(&o.stderr).contains("to stderr"), "{backend}");
    }
    same_bytes(&src);
}

#[test]
fn std_show_writes_what_python_writes() {
    let src = root().join("tests/aipl/show/vectors.aipl");
    let want = std::fs::read_to_string(root().join("tests/aipl/show/vectors.expected")).unwrap();
    for (o, backend) in run_all(&src, "vectors").iter().zip(["VM", "aipl-run", "native"]) {
        assert!(o.status.success(), "{backend}: {}", String::from_utf8_lossy(&o.stderr));
        let got = stdout(o);
        for (k, (g, w)) in got.lines().zip(want.lines()).enumerate() {
            assert_eq!(g, w, "{backend}, case {} (tests/aipl/show/vectors.aipl line {})", k + 1, k + 4);
        }
        assert_eq!(got.lines().count(), want.lines().count(), "{backend}");
    }
}

/// Structs behind pointers, value structs, arrays, unions, results, and
/// generic instances print through the printers formatting generates
/// (tests/aipl/formats/values.aipl): alike in the VM, under aipl-run, and
/// natively, and compiled to the same bytes by both toolchains.
#[test]
fn every_value_prints_through_a_generated_printer() {
    let src = root().join("tests/aipl/formats/values.aipl");
    let want = std::fs::read_to_string(root().join("tests/aipl/formats/values.expected")).unwrap();
    let runs = run_all(&src, "values");
    for (o, backend) in runs.iter().zip(["VM", "aipl-run", "native"]) {
        assert!(o.status.success(), "{backend}: {}", String::from_utf8_lossy(&o.stderr));
        // an address is the same in every backend but changes with the standard library
        let got = stdout(o);
        let masked: String = got
            .split("@ ")
            .enumerate()
            .map(|(k, part)| if k == 0 { part.to_string() } else { format!("@ ADDR{}", part.trim_start_matches(|c: char| c.is_ascii_digit())) })
            .collect();
        assert_eq!(masked, want, "{backend}");
        assert_eq!(got, stdout(&runs[0]), "{backend}: the addresses differ from the VM's");
    }
    same_bytes(&src);
}

/// Every mistake in a formatting form, with its message, the same from the
/// Rust and the self-hosted toolchain.
#[test]
fn every_formatting_error_has_one_message_in_both_toolchains() {
    let cases: &[(&str, &str)] = &[
        ("(println \"{(ref main)}\")", "format cannot print (fn [] -> i32): it prints every value but void and function references"),
        ("(println \"{p:>5}\")", "formats are for numbers, bool, str, str.Bytes, and enums, not (ptr P)"),
        ("(println \"{n:q}\")", "bad format 'q': a format is [[fill]align][+][0][width][.decimals][x|X|b], e.g. {n:>8} or {x:.2}"),
        ("(println \"{name:.2}\")", "decimals (.N) are for f32 and f64, not str"),
        ("(println \"{x:x}\")", "x, X, and b are for i32 and i64, not f64"),
        ("(println \"{name:+}\")", "+ and 0 are for numbers, not str"),
        ("(println \"oops {n\")", "unclosed { in a format string (write {{ for a brace)"),
        ("(println \"a } b\")", "a } in a format string closes nothing (write }} for a brace)"),
        ("(println \"{}\")", "{} needs a value: {name} or {(expression)}"),
        ("(println \"{nme}\")", "Undefined variable 'nme'"),
        ("(println \"{(call f \\\"a\\\")}\")", "a placeholder cannot hold a string or an escape; bind it with let first"),
        ("(eprintln \"\\t{n:\\t>3}\")", "a placeholder cannot hold a string or an escape; bind it with let first"),
        ("(print \"{n x}\")", "a placeholder is {name} or {(expression)}, then optionally :format, e.g. {n:>5} or {x:.2}"),
        ("(format_to n \"x\")", "format_to writes to a (ptr buf.Buf) or a (ptr stream.Writer), got i32"),
        ("(format_to)", "format_to takes a buffer and a string literal, e.g. (format_to b \"n = {n}\")"),
        ("(println name)", "println takes one string literal, e.g. (println \"n = {n}\")"),
        ("(println \"a\" \"b\")", "println takes one string literal, e.g. (println \"n = {n}\")"),
        ("(println \"{n:5000}\")", "bad format '5000': a width is at most 4095"),
        ("(println \"{x:.41}\")", "bad format '.41': at most 40 decimals"),
        ("(println \"{(+ n 1}\")", "unclosed { in a format string (write {{ for a brace)"),
        ("(let q:i32 (format \"x\")) 0", "Type mismatch in 'let': expected i32, got str"),
    ];
    let dir = scratch("errors");
    for (k, (body, want)) in cases.iter().enumerate() {
        let file = dir.join(format!("e{k}.aipl"));
        let src = format!(
            "(module e (import alloc)\n  (struct P [a:i32])\n  (fn main [] -> i32\n    (let p:(ptr P) (call (alloc.create P) (call alloc.forever))) (let n:i32 1) (let x:f64 1.5) (let name:str \"a\")\n    {body}\n    0))\n"
        );
        std::fs::write(&file, &src).unwrap();
        let rust = aipl_core::resolver::Resolver::resolve(&file)
            .and_then(|m| aipl_core::checker::TypeChecker::new().check_module(&m))
            .expect_err(body);
        assert!(rust.contains(want), "{body}\n  expected: {want}\n  rust: {rust}");
        let aipl = aipl_core::selfhost::compile_with_aipl(&file).map(|_| ()).expect_err(body);
        assert_eq!(aipl, rust, "{body}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Without the resolver there is no formatting form: Parser::parse names it.
#[test]
fn the_parser_alone_rejects_the_forms() {
    let err = aipl_core::parser::Parser::parse("(module m (fn main [] -> i32 (println \"x\") 0))").unwrap_err();
    assert!(err.contains("(println ...) is expanded during import resolution"), "{err}");
}
