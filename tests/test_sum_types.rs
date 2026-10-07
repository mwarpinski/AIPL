//! Unions, make, and match (AIPL_SPEC.md 4.J): the same results in the VM,
//! under wasmtime, and as a native executable; the evaluation order of make;
//! the trap for a match on a value that is no member; every checker rule
//! with its message; the printer round trip. Self-hosted byte parity is in
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
    let d = std::env::temp_dir().join(format!("aipl_sum_{}_{}", std::process::id(), name));
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

/// main's result under wasmtime, or the trap's message.
fn wasm_main(wasm: &[u8]) -> Result<i32, String> {
    use wasmtime::{Engine, Linker, Module, Store};
    use wasmtime_wasi::p1::WasiP1Ctx;
    let engine = Engine::default();
    let module = Module::new(&engine, wasm).unwrap();
    let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).unwrap();
    let mut store = Store::new(&engine, wasmtime_wasi::WasiCtxBuilder::new().build_p1());
    let inst = linker.instantiate(&mut store, &module).unwrap();
    let main = inst.get_typed_func::<(), i32>(&mut store, "main").unwrap();
    main.call(&mut store, ()).map_err(|e| format!("{e:?}"))
}

/// tests/aipl/sum_types.aipl built as a native executable, through a
/// wrapper whose main prints its result; the printed number.
fn native_main() -> Option<i32> {
    use std::os::unix::fs::PermissionsExt;
    if !aipl_core::native::supported() {
        return None;
    }
    let dir = scratch("native");
    for f in ["sum_types.aipl", "geometry.aipl"] {
        std::fs::copy(root().join("tests/aipl").join(f), dir.join(f)).unwrap();
    }
    let wrap = dir.join("wrap.aipl");
    std::fs::write(&wrap, "(module wrap (import sum_types) (import io)\n  (fn main [] -> i32 (call io.println_int \"\" (call sum_types.main)) 0))").unwrap();
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
fn sum_types_agree_in_every_backend() {
    let path = root().join("tests/aipl/sum_types.aipl");
    let module = Resolver::resolve(&path).unwrap();
    TypeChecker::new().check_module(&module).unwrap();
    let mut vm = VM::new();
    vm.load_module(module.clone());
    assert_eq!(vm.invoke("main", vec![]).unwrap(), Value::Int(1103));
    let wasm = WasmCompiler::compile(&module).unwrap();
    assert_eq!(wasm_main(&wasm), Ok(1103));
    if let Some(n) = native_main() {
        assert_eq!(n, 1103);
    }
    // the printer round trip: unions, union types, make, and match arms
    let printed = print_module(&module);
    assert!(printed.contains("(union geometry.Path [(stop) (step dx:i32 dy:i32 rest:geometry.Path)])"), "{printed}");
    assert!(printed.contains("(match s (geometry.Shape.rect [w h] 4) (geometry.Shape.circle [r] 0) (else 1))"), "{printed}");
    let reparsed = Parser::parse(&printed).unwrap();
    assert_eq!(print_module(&reparsed), printed);
    TypeChecker::new().check_module(&reparsed).unwrap();
    // positions are in the printed text there: compare without them
    let same = |w: &[u8]| aipl_core::compiler::wasm::without_lines(w);
    assert_eq!(same(&WasmCompiler::compile(&reparsed).unwrap()), same(&wasm));
}

/// Fields of every width keep their values; bool fields read back as 0/1.
#[test]
fn every_field_type_round_trips() {
    let m = check(
        "fields",
        "(module m
           (union V [(wide a:i64 b:f64 c:bool d:str) (narrow x:i32)])
           (fn main [] -> i32
             (match (make V.wide -5000000000i64 2.5 true \"hey\")
               (V.wide [a b c d] (+ (i32.wrap (/ a 1000000000i64)) (+ (i32.wrap (i64.trunc_f64_s (* b 2.0))) (+ (if c 100 0) (str.len d)))))
               (V.narrow [x] x))))",
    )
    .unwrap();
    let mut vm = VM::new();
    vm.load_module(m.clone());
    // -5 + 5 + 100 + 3
    assert_eq!(vm.invoke("main", vec![]).unwrap(), Value::Int(103));
    assert_eq!(wasm_main(&WasmCompiler::compile(&m).unwrap()), Ok(103));
}

/// make claims its cell before evaluating the fields: a field that
/// allocates gets memory after the cell (8 bytes here), in both backends.
#[test]
fn make_allocates_before_its_fields() {
    let m = check(
        "order",
        "(module m (union U [(v p:i32)])
           (fn main [] -> i32
             (let p0:i32 (mem.alloc 8))
             (match (make U.v (mem.alloc 8)) (U.v [p1] (- p1 p0)))))",
    )
    .unwrap();
    let mut vm = VM::new();
    vm.load_module(m.clone());
    assert_eq!(vm.invoke("main", vec![]).unwrap(), Value::Int(16));
    assert_eq!(wasm_main(&WasmCompiler::compile(&m).unwrap()), Ok(16));
}

/// Only an enum.cast value that is no member reaches the end of a match
/// without an else arm: both backends stop there.
#[test]
fn a_value_that_is_no_member_traps() {
    let m = check(
        "trap",
        "(module m (enum C [a b])
           (fn main [] -> i32 (match (enum.cast C 7) (C.a 1) (C.b 2))))",
    )
    .unwrap();
    let mut vm = VM::new();
    vm.load_module(m.clone());
    let e = vm.invoke("main", vec![]).unwrap_err();
    assert!(e.contains("unreachable"), "{e}");
    let e = wasm_main(&WasmCompiler::compile(&m).unwrap()).unwrap_err();
    assert!(e.contains("unreachable"), "{e}");
}

#[test]
fn every_rule_has_its_message() {
    let u = "(union S [(circle r:f64) (sq w:i32) (none)])";
    let cases: Vec<(String, &str)> = vec![
        // definitions
        ("(union S [(a) (a)]) (fn f [] -> i32 0)".into(), "union 'S' has two variants named 'a'"),
        ("(union S [(A)]) (fn f [] -> i32 0)".into(), "union variant 'S.A' must start with a lowercase letter or '_'"),
        ("(union S []) (fn f [] -> i32 0)".into(), "union 'S' has no variants"),
        ("(union S [(a x:i32 x:i32)]) (fn f [] -> i32 0)".into(), "variant 'S.a' has two fields named 'x'"),
        ("(union S [(a)]) (union S [(b)]) (fn f [] -> i32 0)".into(), "Duplicate union definition 'S'"),
        ("(struct S [x:i32]) (union S [(a)]) (fn f [] -> i32 0)".into(), "'S' is defined as a union and as a struct or enum"),
        ("(union S [(a x:void)]) (fn f [] -> i32 0)".into(), "Field 'x' of variant 'S.a'"),
        ("(union S [(a p:(ptr Q))]) (fn f [] -> i32 0)".into(), "Unknown struct 'Q' in (ptr Q) (field 'p' of variant 'S.a')"),
        ("(union S [a]) (fn f [] -> i32 0)".into(), "a union variant is (name field:type ...)"),
        // make
        (format!("{u} (fn f [] -> S (make S.oval 1.0))"), "union 'S' has no variant 'oval'"),
        (format!("{u} (fn f [] -> S (make S.circle))"), "make S.circle takes 1 values (its fields), got 0"),
        (format!("{u} (fn f [] -> S (make S.sq 1.0))"), "make S.sq: field 'w' is i32, got f64"),
        ("(fn f [] -> i32 (make T.a))".into(), "Unknown union 'T' in make"),
        ("(fn f [] -> i32 (make nodot))".into(), "make names a variant as Union.variant, got nodot"),
        // match
        (format!("{u} (fn f [s:S] -> i32 (match s (S.circle [r] 1) (S.sq [w] 2)))"), "match is missing S.none (add those arms or an else arm)"),
        (format!("{u} (fn f [s:S] -> i32 (match s (S.circle [r] 1) (S.sq [w] 2) (S.none 3) (else 4)))"), "the else arm never runs: every member of S is matched"),
        (format!("{u} (fn f [s:S] -> i32 (match s (S.circle [r] 1) (S.circle [q] 2) (else 0)))"), "S.circle is matched twice"),
        (format!("{u} (fn f [s:S] -> i32 (match s (S.circle 1) (else 0)))"), "the S.circle arm binds its 1 fields in order: [r], got 0 names"),
        (format!("{u} (fn f [s:S] -> i32 (match s (S.oval 1) (else 0)))"), "S has no member 'oval'"),
        (format!("{u} (union T [(a)]) (fn f [s:S] -> i32 (match s (T.a 1) (else 0)))"), "a match arm on S is (S.member ...), got T.a"),
        (format!("{u} (fn f [s:S] -> i32 (match s (S.sq [w] w) (else true)))"), "match arm type mismatch: S.sq yields i32, else yields bool"),
        (format!("{u} (fn f [s:S r:f64] -> i32 (match s (S.circle [r] 1) (else 0)))"), "Cannot shadow existing variable 'r' in the S.circle arm"),
        ("(enum C [a b]) (fn f [c:C] -> i32 (match c (C.a [x] 1) (else 0)))".into(), "C.a is an enum member; its arm binds nothing"),
        ("(fn f [x:i32] -> i32 (match x (else 0)))".into(), "match needs a union or enum value, got i32"),
        (format!("{u} (fn f [s:S] -> i32 (match s (else 0) (S.none 1)))"), "the (else ...) arm of a match comes last"),
        (format!("{u} (fn f [s:S] -> i32 (match s (else [x] 0)))"), "the else arm of a match binds nothing"),
        // union values
        (format!("{u} (fn f [a:S b:S] -> bool (eq a b))"), "eq on union 'S': union values do not compare; take them apart with match"),
        (format!("{u} (fn f [a:S] -> i32 (+ a 1))"), "+ on union 'S': unions have no arithmetic; take them apart with match"),
        (format!("{u} (fn f [] -> S (ptr.null S))"), "Unknown struct 'S' in (ptr S)"),
        (format!("{u} (fn f [] -> S (enum.cast S 1))"), "'S' is a union, not an enum: its values are made with (make S.variant ...)"),
        // a name keeps one type per function
        (format!("{u} (fn f [s:S] -> i32 (match s (S.circle [x] 1) (S.sq [x] x) (else 0)))"), "'x' is i32 here but f64 elsewhere in this function; a name keeps one type per function (rename one)"),
        ("(fn f [] -> i32 (block (let y:f64 1.5) 0) (block (let y:i32 3) y))".into(), "'y' is i32 here but f64 elsewhere in this function"),
    ];
    for (i, (body, want)) in cases.iter().enumerate() {
        let src = format!("(module m {body})");
        match check(&format!("rule{i}"), &src) {
            Ok(_) => panic!("accepted: {src}"),
            Err(e) => assert!(e.contains(want), "{src}\n  expected: {want}\n  got: {e}"),
        }
    }
}

/// `_` in an arm's binders reads nothing: it may repeat, need not keep a
/// type, and gets no local, in every backend and the self-hosted compiler.
#[test]
fn wildcard_binders_bind_nothing() {
    let m = check(
        "wild",
        "(module m (union U [(a x:i64 y:bool) (b x:f64 y:i32)])
           (fn f [u:U] -> i32 (match u (U.a [_ _] 1) (U.b [_ y] y)))
           (fn main [] -> i32 (+ (call f (make U.a 5i64 true)) (call f (make U.b 1.5 41)))))",
    )
    .unwrap();
    let mut vm = VM::new();
    vm.load_module(m.clone());
    assert_eq!(vm.invoke("main", vec![]).unwrap(), Value::Int(42));
    assert_eq!(wasm_main(&WasmCompiler::compile(&m).unwrap()), Ok(42));
    // `_` is not a variable inside the arm
    let e = check("wild2", "(module m (union U [(a x:i32)]) (fn f [u:U] -> i32 (match u (U.a [_] _))))").unwrap_err();
    assert!(e.contains("Undefined variable '_'"), "{e}");
}
