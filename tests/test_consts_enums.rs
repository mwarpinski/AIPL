//! Named constants and enums (AIPL_SPEC.md 4.I): the same results in the VM,
//! under wasmtime, and as a native executable; every checker rule with its
//! message; the printer round trip. Self-hosted byte parity is in
//! tests/test_selfhost.rs and tests/test_resolver_aipl.rs.

use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::parser::Parser;
use aipl_core::printer::print_module;
use aipl_core::resolver::Resolver;
use aipl_core::vm::{Value, VM};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_consts_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Resolves and checks `src` as a one-file program; the error if either fails.
fn check(name: &str, src: &str) -> Result<aipl_core::ast::Module, String> {
    let dir = scratch(name);
    let file = dir.join("m.aipl");
    std::fs::write(&file, src).unwrap();
    let m = Resolver::resolve(&file);
    let _ = std::fs::remove_dir_all(&dir);
    let m = m?;
    TypeChecker::new().check_module(&m)?;
    Ok(m)
}

fn wasm_main(wasm: &[u8]) -> i32 {
    use wasmtime::{Engine, Linker, Module, Store};
    use wasmtime_wasi::p1::WasiP1Ctx;
    let engine = Engine::default();
    let module = Module::new(&engine, wasm).unwrap();
    let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).unwrap();
    let mut store = Store::new(&engine, wasmtime_wasi::WasiCtxBuilder::new().build_p1());
    let inst = linker.instantiate(&mut store, &module).unwrap();
    inst.get_typed_func::<(), i32>(&mut store, "main").unwrap().call(&mut store, ()).unwrap()
}

/// tests/aipl/consts_enums.aipl built as a native executable, through a
/// wrapper whose main prints its result; the printed number.
fn native_main() -> Option<i32> {
    use std::os::unix::fs::PermissionsExt;
    if !aipl_core::native::supported() {
        return None;
    }
    let dir = scratch("native");
    for f in ["consts_enums.aipl", "palette.aipl"] {
        std::fs::copy(root().join("tests/aipl").join(f), dir.join(f)).unwrap();
    }
    let wrap = dir.join("wrap.aipl");
    std::fs::write(&wrap, "(module wrap (import consts_enums) (import io)\n  (fn main [] -> i32 (call io.println_int \"\" (call consts_enums.main)) 0))").unwrap();
    let m = Resolver::resolve(&wrap).unwrap();
    TypeChecker::new().check_module(&m).unwrap();
    let exe = dir.join("prog");
    std::fs::write(&exe, aipl_core::native::executable(&WasmCompiler::compile(&m).unwrap(), false).unwrap()).unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = loop {
        match std::process::Command::new(&exe).output() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => std::thread::sleep(std::time::Duration::from_millis(10)),
            r => break r.unwrap(),
        }
    };
    let _ = std::fs::remove_dir_all(&dir);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    Some(String::from_utf8(out.stdout).unwrap().trim().parse().unwrap())
}

#[test]
fn constants_and_enums_agree_in_every_backend() {
    let path = root().join("tests/aipl/consts_enums.aipl");
    let module = Resolver::resolve(&path).unwrap();
    TypeChecker::new().check_module(&module).unwrap();
    let mut vm = VM::new();
    vm.load_module(module.clone());
    assert_eq!(vm.invoke("main", vec![]).unwrap(), Value::Int(2294));
    let wasm = WasmCompiler::compile(&module).unwrap();
    assert_eq!(wasm_main(&wasm), 2294);
    if let Some(n) = native_main() {
        assert_eq!(n, 2294);
    }
    // the printer round trip: enums, enum types, enum.cast and enum.ord
    let printed = print_module(&module);
    assert!(printed.contains("(enum Op [(add 0) (sub 1) (mul 7)])"), "{printed}");
    assert!(printed.contains("(enum.cast palette.Color 10)"), "{printed}");
    let reparsed = Parser::parse(&printed).unwrap();
    assert_eq!(print_module(&reparsed), printed);
    TypeChecker::new().check_module(&reparsed).unwrap();
    assert_eq!(WasmCompiler::compile(&reparsed).unwrap(), wasm);
}

#[test]
fn enum_values_count_up_from_the_last_explicit_one() {
    let m = check(
        "values",
        "(module m (enum E [a b (c 10) d (e -3) f])
           (fn main [] -> i32 (+ (enum.ord E.a) (+ (* 10 (enum.ord E.b)) (+ (* 100 (enum.ord E.d)) (enum.ord E.f))))))",
    )
    .unwrap();
    let mut vm = VM::new();
    vm.load_module(m.clone());
    // a 0, b 1, d 11, f -2
    assert_eq!(vm.invoke("main", vec![]).unwrap(), Value::Int(10 + 1100 - 2));
    assert_eq!(wasm_main(&WasmCompiler::compile(&m).unwrap()), 1108);
}

#[test]
fn every_rule_has_its_message() {
    let cases: &[(&str, &str)] = &[
        // enums are their own types
        ("(enum C [a b]) (fn f [] -> i32 (let x:C C.a) (+ x 1))", "Add on enum 'C': enums have no arithmetic; compare them with eq/neq, or convert with (enum.ord x) and (enum.cast C n)"),
        ("(enum C [a b]) (fn f [] -> i32 (+ 1 C.b))", "Add on enum 'C': enums have no arithmetic"),
        ("(enum C [a b]) (enum D [a b]) (fn f [] -> bool (eq C.a D.a))", "Type mismatch in comparison: Enum(\"C\") vs Enum(\"D\")"),
        ("(enum C [a b]) (fn f [] -> bool (lt C.a C.b))", "pointers, arrays, function refs, and enums compare only with eq/neq"),
        ("(enum C [a b]) (fn f [] -> i32 C.a)", "Function 'f' expects return type I32, but body returned Enum(\"C\")"),
        ("(enum C [a]) (fn f [] -> C (enum.cast C 1i64))", "enum.cast C needs an i32 value, got I64"),
        ("(fn f [] -> i32 (enum.ord 3))", "enum.ord needs an enum value, got I32"),
        // definitions
        ("(enum C [a b]) (fn f [] -> C C.z)", "enum 'C' has no member 'z'"),
        ("(enum C [a (b 0)]) (fn f [] -> i32 0)", "enum 'C': members 'a' and 'b' both have the value 0"),
        ("(enum C [a a]) (fn f [] -> i32 0)", "enum 'C' has two members named 'a'"),
        ("(enum C [Red]) (fn f [] -> i32 0)", "enum member 'C.Red' must start with a lowercase letter or '_'"),
        ("(enum C []) (fn f [] -> i32 0)", "enum 'C' has no members"),
        ("(enum C [a]) (enum C [b]) (fn f [] -> i32 0)", "Duplicate enum definition 'C'"),
        ("(struct C [x:i32]) (enum C [a]) (fn f [] -> i32 0)", "'C' is defined as both a struct and an enum"),
        ("(enum C [(a 3000000000)]) (fn f [] -> i32 0)", "enum member value 3000000000 is not an i32"),
        // types
        ("(fn f [x:Colr] -> i32 0)", "Unknown type 'Colr' (not a scalar type or an enum)"),
        ("(struct P [x:i32]) (fn f [p:P] -> i32 0)", "'P' is a struct, which is only used through a pointer: write (ptr P)"),
        // constants
        ("(const max:i32 5) (fn f [] -> i32 max)", "constant 'max' must be named in capitals, at least two characters"),
        ("(const N:i32 5) (fn f [] -> i32 N)", "constant 'N' must be named in capitals"),
        ("(const NN:i32 true) (fn f [] -> i32 NN)", "constant 'NN' is declared i32 but its value is not an i32 literal"),
        ("(const NN:i32 3000000000) (fn f [] -> i32 NN)", "constant 'NN' is declared i32 but its value is not an i32 literal"),
        ("(const NN:f32 1.0) (fn f [] -> i32 0)", "constant 'NN' has type 'f32'; a constant is i32, i64, f64, bool, or str"),
        ("(const NN:i32 (+ 1 2)) (fn f [] -> i32 NN)", "constant 'NN' is declared i32 but its value is not an i32 literal"),
        ("(const MAX:i32 5) (fn f [] -> i32 (let MAX:i32 1) MAX)", "'MAX' is a constant; it cannot also name a variable, parameter, or field"),
        ("(const MAX:i32 5) (fn f [MAX:i32] -> i32 0)", "'MAX' is a constant; it cannot also name a variable, parameter, or field"),
        ("(const MAX:i32 5) (const MAX:i32 6) (fn f [] -> i32 MAX)", "constant 'MAX' is defined twice"),
        ("(const MAX:i32) (fn f [] -> i32 0)", "a constant is (const NAME:TYPE literal)"),
    ];
    for (i, (body, want)) in cases.iter().enumerate() {
        let src = format!("(module m {body})");
        match check(&format!("rule{i}"), &src) {
            Ok(_) => panic!("accepted: {src}"),
            Err(e) => assert!(e.contains(want), "{src}\n  expected: {want}\n  got: {e}"),
        }
    }
}

#[test]
fn constants_keep_their_literal_types() {
    let m = check(
        "kinds",
        "(module m (const AA:i32 -7) (const BB:i64 -9000000000i64) (const CC:f64 2.5) (const DD:bool false) (const EE:str \"a\\\"b\")
           (fn main [] -> i32
             (+ AA (+ (i32.wrap (/ BB 1000000000i64)) (+ (i32.wrap (i64.trunc_f64_s (* CC 2.0))) (+ (if DD 100 0) (str.len EE)))))))",
    )
    .unwrap();
    let mut vm = VM::new();
    vm.load_module(m.clone());
    // -7 - 9 + 5 + 0 + 3
    assert_eq!(vm.invoke("main", vec![]).unwrap(), Value::Int(-8));
    assert_eq!(wasm_main(&WasmCompiler::compile(&m).unwrap()), -8);
}

/// Parser::parse (no resolver) has no constant expansion, like generics.
#[test]
fn constants_need_the_resolver() {
    let e = Parser::parse("(module m (const NN:i32 1) (fn f [] -> i32 NN))").unwrap_err();
    assert!(e.contains("(const ...) is expanded during import resolution"), "{e}");
}
