//! First-class function references (P10): `(ref f)` has type
//! `(fn [params] -> ret)`, `call_ref` names the signature it calls through,
//! and both backends agree (a funcref table + call_indirect in wasm).

use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::parser::Parser;
use aipl_core::resolver::Resolver;
use aipl_core::vm::{Value, VM};
use wasmtime::{Engine, Instance, Module as WasmModule, Store, TypedFunc};

fn check_err(src: &str) -> String {
    let m = Parser::parse(src).unwrap_or_else(|e| panic!("parse failed: {e}"));
    TypeChecker::new().check_module(&m).expect_err("expected a checker error")
}

fn run_both(src: &str, f: &str) -> i32 {
    let module = Parser::parse(src).unwrap();
    TypeChecker::new().check_module(&module).unwrap_or_else(|e| panic!("check failed: {e}"));
    let mut vm = VM::new();
    vm.load_module(module.clone());
    let vm_val = match vm.invoke(f, vec![]).unwrap() {
        Value::Int(i) => i as i32,
        Value::Bool(b) => b as i32,
        other => panic!("unexpected {other:?}"),
    };
    let wasm = WasmCompiler::compile(&module).unwrap();
    wasmparser::Validator::new().validate_all(&wasm).expect("valid wasm");
    let engine = Engine::default();
    let m = WasmModule::new(&engine, &wasm).unwrap();
    let mut store = Store::new(&engine, ());
    let inst = Instance::new(&mut store, &m, &[]).unwrap();
    let func: TypedFunc<(), i32> = inst.get_typed_func(&mut store, f).unwrap();
    assert_eq!(vm_val, func.call(&mut store, ()).unwrap(), "VM and wasm disagree on {f}");
    vm_val
}

pub const REFS_PROGRAM: &str = r#"
(module refs
  (struct Op [apply:(fn [i32 i32] -> i32) name:str])
  (fn add [a:i32 b:i32] -> i32 (+ a b))
  (fn mul [a:i32 b:i32] -> i32 (* a b))
  (fn neg [a:i64] -> i64 (- 0i64 a))
  (fn twice [f:(fn [i32 i32] -> i32) x:i32] -> i32
    (call_ref (fn [i32 i32] -> i32) f x x))
  (fn main [] -> i32
    (let ops:(arr (fn [i32 i32] -> i32)) (arr.new (fn [i32 i32] -> i32) 2))
    (arr.set (fn [i32 i32] -> i32) ops 0 (ref add))
    (arr.set (fn [i32 i32] -> i32) ops 1 (ref mul))
    (let o:(ptr Op) (new Op))
    (put o Op.apply (ref mul))
    (let n:i64 (call_ref (fn [i64] -> i64) (ref neg) 5i64))
    (+ (call twice (arr.get (fn [i32 i32] -> i32) ops 0) 20)
       (+ (call_ref (fn [i32 i32] -> i32) (get o Op.apply) 3 4)
          (+ (i32.wrap n) (if (and (eq (ref add) (ref add)) (neq (ref add) (ref mul))) 1 0))))))
"#;

#[test]
fn refs_through_params_arrays_and_struct_fields_agree() {
    // twice(add, 20) = 40, mul 3 4 = 12, neg 5 = -5, identity checks 1
    assert_eq!(run_both(REFS_PROGRAM, "main"), 48);
}

#[test]
fn a_ref_is_the_functions_position_in_both_backends() {
    let src = "(module m (fn a [] -> i32 0) (fn b [] -> i32 1) (fn f [] -> bool (eq (ref b) (ref b))))";
    assert_eq!(run_both(src, "f"), 1);
}

#[test]
fn call_ref_signature_must_match_the_function_type() {
    let err = check_err("(module m (fn add [a:i32 b:i32] -> i32 (+ a b))\n  (fn f [] -> i64\n    (call_ref (fn [i32 i32] -> i64) (ref add) 1 2)))");
    assert!(err.starts_with("3:5: call_ref signature") && err.contains("does not match"), "{err}");
    let err = check_err("(module m (fn add [a:i32 b:i32] -> i32 (+ a b))\n  (fn f [] -> i32\n    (call_ref (fn [i32 i32] -> i32) (ref add) 1)))");
    assert!(err.contains("call_ref expects 2 arguments, got 1"), "{err}");
    let err = check_err("(module m (fn add [a:i32 b:i32] -> i32 (+ a b))\n  (fn f [] -> i32\n    (call_ref (fn [i32 i32] -> i32) (ref add) 1 true)))");
    assert!(err.contains("call_ref argument 2 expects i32, got bool"), "{err}");
}

#[test]
fn refs_are_not_integers() {
    let err = check_err("(module m (fn f [] -> i32 (call_ref (fn [] -> i32) 0)))");
    assert!(err.contains("does not match the function's type"), "{err}");
    let err = check_err("(module m (fn g [] -> i32 1) (fn f [] -> (fn [] -> i32) (+ (ref g) (ref g))))");
    assert!(err.contains("have no arithmetic"), "{err}");
    let err = check_err("(module m (fn f [] -> i32 (call_ref (fn [] -> i32) (ref missing))))");
    assert!(err.contains("Undefined function 'missing' in ref"), "{err}");
}

#[test]
fn thread_spawn_takes_a_worker_of_type_fn_i32_to_i32() {
    let err = check_err("(module m (fn w [a:i64] -> i32 (unsafe) 0) (fn f [] -> i32 (unsafe) (thread.spawn (ref w) 1)))");
    assert!(err.contains("thread.spawn needs a worker of type (fn [i32] -> i32)"), "{err}");
}

#[test]
fn refs_in_an_imported_module_point_at_its_qualified_functions() {
    let dir = std::env::temp_dir().join(format!("aipl_refs_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("app.aipl"), "(module app (import lib) (fn main [] -> i32 (call lib.run 6)))").unwrap();
    std::fs::write(dir.join("lib.aipl"), "(module lib (fn sq [x:i32] -> i32 (* x x)) (fn run [x:i32] -> i32 (call_ref (fn [i32] -> i32) (ref sq) x)))").unwrap();
    let module = Resolver::resolve(&dir.join("app.aipl")).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    TypeChecker::new().check_module(&module).unwrap();
    let mut vm = VM::new();
    vm.load_module(module.clone());
    assert_eq!(vm.invoke("main", vec![]).unwrap(), Value::Int(36));
    let wasm = WasmCompiler::compile(&module).unwrap();
    let engine = Engine::default();
    let m = WasmModule::new(&engine, &wasm).unwrap();
    let mut store = Store::new(&engine, ());
    let inst = Instance::new(&mut store, &m, &[]).unwrap();
    let f: TypedFunc<(), i32> = inst.get_typed_func(&mut store, "main").unwrap();
    assert_eq!(f.call(&mut store, ()).unwrap(), 36);
}
