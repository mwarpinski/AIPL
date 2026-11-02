//! Typed pointers and arrays: `(ptr S)` and `(arr T)` are checked strictly,
//! lower to i32 in both backends, and struct names are namespaced by imports.

use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::resolver::Resolver;
use aipl_core::vm::{Value, VM};
use std::fs;
use wasmtime::{Engine, Instance, Module as WasmModule, Store, TypedFunc};

/// The program as the toolchain reads it: resolved when it imports (an
/// allocation names its allocator from std/alloc), else just parsed.
fn parse_program(src: &str) -> Result<aipl_core::ast::Module, String> {
    if src.contains("(import ") {
        aipl_core::resolver::Resolver::resolve_source(src, &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/inline.aipl"))
    } else {
        aipl_core::parser::Parser::parse(src)
    }
}

fn check_err(src: &str) -> String {
    let module = parse_program(src).unwrap_or_else(|e| panic!("parse failed: {e}"));
    TypeChecker::new().check_module(&module).expect_err("expected a checker error")
}

fn parse_err(src: &str) -> String {
    parse_program(src).expect_err("expected a parse error")
}

/// Runs a zero-arg i32 function in the VM and in wasmtime; both must agree.
fn run_both(src: &str, f: &str) -> i32 {
    let module = parse_program(src).unwrap();
    TypeChecker::new().check_module(&module).unwrap_or_else(|e| panic!("check failed: {e}"));
    let mut vm = VM::new();
    vm.load_module(module.clone());
    let vm_val = match vm.invoke(f, vec![]).unwrap() {
        Value::Int(i) => i as i32,
        Value::Bool(b) => b as i32,
        other => panic!("unexpected {other:?}"),
    };
    let wasm = WasmCompiler::compile(&module).unwrap();
    let engine = Engine::default();
    let m = WasmModule::new(&engine, &wasm).unwrap();
    let mut store = Store::new(&engine, ());
    let inst = Instance::new(&mut store, &m, &[]).unwrap();
    let func: TypedFunc<(), i32> = inst.get_typed_func(&mut store, f).unwrap();
    let wasm_val = func.call(&mut store, ()).unwrap();
    assert_eq!(vm_val, wasm_val, "VM and wasm disagree on {f}");
    vm_val
}

// ---------------------------------------------------------------------------
// The checker rejects every way of mixing up pointers, arrays, and integers.
// ---------------------------------------------------------------------------

const STRUCTS: &str = "(struct Point [x:i32 y:i32]) (struct Node [val:i32 next:(ptr Node)])";

#[test]
fn get_through_the_wrong_struct_pointer_is_rejected() {
    let err = check_err(&format!("(module m {STRUCTS}\n  (fn f [] -> i32\n    (let n:(ptr Node) (ptr.null Node))\n    (get n Point.x)))"));
    assert!(err.starts_with("4:5: get Point.x needs a (ptr Point)"), "{err}");
}

#[test]
fn an_i32_is_not_a_pointer() {
    let err = check_err(&format!("(module m {STRUCTS}\n  (fn f [p:i32] -> i32\n    (get p Point.x)))"));
    assert!(err.starts_with("3:5: get Point.x needs a (ptr Point), got i32"), "{err}");
    let err = check_err(&format!("(module m {STRUCTS}\n  (fn f [] -> i32\n    (let p:(ptr Point) 1024)\n    0))"));
    assert!(err.contains("Type mismatch in 'let'"), "{err}");
}

#[test]
fn pointers_have_no_arithmetic_or_ordering() {
    let err = check_err(&format!("(module m {STRUCTS}\n  (fn f [p:(ptr Point)] -> (ptr Point)\n    (+ p p)))"));
    assert!(err.starts_with("3:5:") && err.contains("pointers, arrays, and function refs have no arithmetic"), "{err}");
    let err = check_err(&format!("(module m {STRUCTS}\n  (fn f [p:(ptr Point) q:(ptr Point)] -> bool\n    (lt p q)))"));
    assert!(err.contains("compare only with eq/neq"), "{err}");
}

#[test]
fn arrays_are_typed_by_element_and_distinct_from_pointers() {
    let err = check_err("(module m\n  (fn f [] -> i64\n    (let a:(arr i32) (arr.null i32))\n    (arr.get i64 a 0)))");
    assert!(err.starts_with("4:5: arr.get i64 needs an (arr i64)"), "{err}");
    let err = check_err(&format!("(module m {STRUCTS}\n  (fn f [p:(ptr Point)] -> i32\n    (arr.len p)))"));
    assert!(err.contains("arr.len needs an (arr T)"), "{err}");
    let err = check_err(&format!("(module m {STRUCTS}\n  (fn f [a:(arr i32)] -> i32\n    (ptr.addr a)))"));
    assert!(err.contains("ptr.addr needs a (ptr S)"), "{err}");
}

#[test]
fn malformed_pointer_and_array_types_are_rejected() {
    let err = parse_err("(module m (fn f [p:(ptr i32)] -> i32 0))");
    assert!(err.contains("ptr points to a struct; for a sequence of i32 use (arr i32)"), "{err}");
    let err = parse_err("(module m (fn f [a:(arr i32 4)] -> i32 0))");
    assert!(err.contains("(arr T) takes no length"), "{err}");
    let err = check_err("(module m (fn f [p:(ptr Missing)] -> i32 0))");
    assert!(err.contains("Unknown struct 'Missing' in (ptr Missing)"), "{err}");
    let err = check_err("(module m (struct S [r:(result i32 i32)]) (fn f [] -> i32 0))");
    assert!(err.contains("Unsupported type for memory layout") || err.contains("field 'r'"), "{err}");
}

#[test]
fn casts_must_start_from_an_i32() {
    let err = check_err(&format!("(module m {STRUCTS}\n  (fn f [p:(ptr Point)] -> (ptr Node) (unsafe)\n    (ptr.cast Node p)))"));
    assert!(err.contains("cast needs an i32 address"), "{err}");
}

// ---------------------------------------------------------------------------
// Both backends agree, and the types cost nothing at run time.
// ---------------------------------------------------------------------------

#[test]
fn typed_linked_list_runs_in_both_backends() {
    let src = r#"
(module list (import alloc)
  (struct Node [(read val:i32) next:(ptr Node)])
  (fn push [head:(ptr Node) v:i32] -> (ptr Node)
    (let n:(ptr Node) (new Node (call alloc.forever) [val:v next:head]))
    n)
  (fn sum [] -> i32
    (let h:(ptr Node) (ptr.null Node))
    (for i 1 11 (set! h (call push h i)))
    (let total:i32 0)
    (while (neq h (ptr.null Node))
      (set! total (+ total (get h Node.val)))
      (set! h (get h Node.next)))
    total))
"#;
    assert_eq!(run_both(src, "sum"), 55);
}

#[test]
fn arrays_of_pointers_lengths_and_casts_run_in_both_backends() {
    let src = r#"
(module arrs (import alloc)
  (struct P [x:i32])
  (fn f [] -> i32 (unsafe)
    (let ps:(arr (ptr P)) (arr.new (ptr P) (call alloc.forever) 4))
    (for i 0 4
      (let p:(ptr P) (new P (call alloc.forever) [x:(* i i)]))
      (arr.set (ptr P) ps i p))
    (let total:i32 0)
    (for i 0 (arr.len ps)
      (set! total (+ total (get (arr.get (ptr P) ps i) P.x))))
    ;; round trip through an address: same pointer, same field
    (let back:(ptr P) (ptr.cast P (ptr.addr (arr.get (ptr P) ps 3))))
    (let nested:(arr (arr i32)) (arr.new (arr i32) (call alloc.forever) 1))
    (arr.set (arr i32) nested 0 (arr.new i32 (call alloc.forever) 7))
    (+ (* 100 (arr.len (arr.get (arr i32) nested 0))) (+ total (get back P.x)))))
"#;
    // 0 + 1 + 4 + 9 = 14, plus P.x of element 3 (9), plus 100 * 7
    assert_eq!(run_both(src, "f"), 723);
}

#[test]
fn null_pointers_compare_equal_and_cost_nothing() {
    let src = r#"
(module nulls
  (struct S [v:i32])
  (fn f [] -> bool
    (let a:(ptr S) (ptr.null S))
    (let b:(arr i32) (arr.null i32))
    (and (eq a (ptr.null S)) (and (eq (ptr.addr a) 0) (eq (arr.addr b) 0)))))
"#;
    assert_eq!(run_both(src, "f"), 1);
}

// ---------------------------------------------------------------------------
// Struct names are namespaced by the import system, like function names.
// ---------------------------------------------------------------------------

fn resolve_files(files: &[(&str, &str)]) -> Result<aipl_core::ast::Module, String> {
    let dir = std::env::temp_dir().join(format!("aipl_ptr_resolve_{}_{}", std::process::id(), files[0].0));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    for (name, src) in files {
        fs::write(dir.join(format!("{name}.aipl")), src).unwrap();
    }
    let r = Resolver::resolve(&dir.join(format!("{}.aipl", files[0].0)));
    let _ = fs::remove_dir_all(&dir);
    r
}

#[test]
fn two_imported_modules_may_each_define_the_same_struct_name() {
    let module = resolve_files(&[
        ("app", r#"
(module app
  (import geo)
  (import list as l)
  (fn main [] -> i32
    (let p:(ptr geo.Node) (call geo.make 40))
    (let q:(ptr l.Node) (call l.make 2))
    (+ (get p geo.Node.x) (get q l.Node.val))))"#),
        ("geo", r#"
(module geo (import alloc)
  (struct Node [(read x:i32) y:i32] (pub))
  (fn make [x:i32] -> (ptr Node) (pub)
    (let n:(ptr Node) (new Node (call alloc.forever) [x:(call ident x) y:(zero i32)]))
    n)
  (fn ident [v:i32] -> i32 v))"#),
        ("list", r#"
(module list (import alloc)
  (struct Node [(read val:i32) next:(ptr Node)] (pub))
  (fn make [v:i32] -> (ptr Node) (pub)
    (let n:(ptr Node) (new Node (call alloc.forever) [val:v next:(ptr.null Node)]))
    n))"#),
    ])
    .expect("resolve");
    // the program's own structs (std/alloc brings its own)
    let names: Vec<&str> = module.structs.iter().map(|s| s.name.as_str()).filter(|n| n.starts_with("geo.") || n.starts_with("list.")).collect();
    assert_eq!(names, ["geo.Node", "list.Node"]);
    TypeChecker::new().check_module(&module).unwrap_or_else(|e| panic!("check failed: {e}"));
    let mut vm = VM::new();
    vm.load_module(module.clone());
    // (call ident x) inside (put ...) was renamed too: the old resolver skipped calls nested in struct forms
    assert_eq!(vm.invoke("main", vec![]).unwrap(), Value::Int(42));
    WasmCompiler::compile(&module).expect("compiles");
}

#[test]
fn an_importers_bare_struct_name_does_not_reach_into_an_import() {
    let err = resolve_files(&[
        ("app2", "(module app2 (import geo2) (fn main [] -> i32 (sizeof Node)))"),
        ("geo2", "(module geo2 (struct Node [x:i32]) (fn f [] -> i32 0))"),
    ])
    .and_then(|m| TypeChecker::new().check_module(&m))
    .unwrap_err();
    assert!(err.contains("Unknown struct 'Node'"), "{err}");
}
