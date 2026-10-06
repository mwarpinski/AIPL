//! Generics (AIPL_SPEC.md 4.H): templates expanded into concrete items before
//! type checking. Every program runs in the VM and in wasmtime and must
//! agree, and the AIPL resolver (resolver.aipl + generics.aipl) must produce
//! a program that compiles to the same bytes as the Rust resolver's.

use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::resolver::Resolver;
use aipl_core::vm::{Value, VM};
use std::path::{Path, PathBuf};
use wasmtime::{Engine, Instance, Module as WasmModule, Store};

/// Writes `files` into a fresh directory; returns the path of the first.
fn write_program(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aipl_generics_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (f, src) in files {
        std::fs::write(dir.join(f), src).unwrap();
    }
    dir.join(files[0].0)
}

/// Runs zero-argument i32 function `f` in the VM and in wasmtime.
fn run_both(entry: &Path, f: &str) -> i32 {
    let module = Resolver::resolve(entry).unwrap_or_else(|e| panic!("resolve: {e}"));
    TypeChecker::new().check_module(&module).unwrap_or_else(|e| panic!("check: {e}"));
    let mut vm = VM::new();
    vm.load_module(module.clone());
    let v = match vm.invoke(f, vec![]).unwrap() {
        Value::Int(i) => i as i32,
        other => panic!("{other:?}"),
    };
    let wasm = WasmCompiler::compile(&module).unwrap();
    let engine = Engine::default();
    let m = WasmModule::new(&engine, &wasm).unwrap();
    let mut store = Store::new(&engine, ());
    let inst = Instance::new(&mut store, &m, &[]).unwrap();
    let w = inst.get_typed_func::<(), i32>(&mut store, f).unwrap().call(&mut store, ()).unwrap();
    assert_eq!(v, w, "VM and wasm disagree on {f}");
    v
}

fn rust_bytes(entry: &Path) -> Vec<u8> {
    WasmCompiler::compile(&Resolver::resolve(entry).unwrap()).unwrap()
}

/// The AIPL resolver's flat program compiles to the Rust resolver's bytes.
fn assert_aipl_resolver_agrees(entry: &Path) {
    let flat = aipl_core::selfhost::resolve_with_aipl(entry).unwrap_or_else(|e| panic!("aipl resolve: {e}"));
    let m = aipl_core::parser::Parser::parse(&flat).unwrap_or_else(|e| panic!("flat output: {e}\n{flat}"));
    TypeChecker::new().check_module(&m).unwrap_or_else(|e| panic!("flat output does not check: {e}\n{flat}"));
    assert!(WasmCompiler::compile(&m).unwrap() == rust_bytes(entry), "AIPL and Rust resolvers disagree:\n{flat}");
}

const BOX: &str = r#"(module box
  (struct (Box T) [value:T next:(ptr (Box T))])
  (fn (make T) [v:T] -> (ptr (Box T))
    (let b:(ptr (Box T)) (new (Box T)))
    (put b (Box T) value v)
    (put b (Box T) next (ptr.null (Box T)))
    b)
  (fn (value T) [b:(ptr (Box T))] -> T (get b (Box T) value))
  ;; a generic calling a generic, and a two-parameter struct
  (struct (Pair A B) [first:A second:B])
  (fn (pair A B) [a:A b:B] -> (ptr (Pair A B))
    (let p:(ptr (Pair A B)) (new (Pair A B)))
    (put p (Pair A B) first a)
    (put p (Pair A B) second b)
    p)
  (fn (boxed_pair A B) [a:A b:B] -> (ptr (Box (ptr (Pair A B))))
    (call (make (ptr (Pair A B))) (call (pair A B) a b))))"#;

const MAIN: &str = r#"(module main
  (import box as bx)
  (struct Point [x:i32 y:i32])
  (fn (twice T) [f:(fn [T] -> T) x:T] -> T (call_ref (fn [T] -> T) f (call_ref (fn [T] -> T) f x)))
  (fn inc [x:i32] -> i32 (+ x 1))
  (fn scalars [] -> i32
    (let a:(ptr (bx.Box i32)) (call (bx.make i32) 40))
    (let b:(ptr (bx.Box i64)) (call (bx.make i64) 2i64))
    (+ (call (bx.value i32) a) (i32.wrap (call (bx.value i64) b))))
  (fn structs [] -> i32
    (let p:(ptr Point) (new Point))
    (put p Point.x 7)
    (let c:(ptr (bx.Box (ptr Point))) (call (bx.make (ptr Point)) p))
    (get (call (bx.value (ptr Point)) c) Point.x))
  (fn nested [] -> i32
    (let bp:(ptr (bx.Box (ptr (bx.Pair i32 bool)))) (call (bx.boxed_pair i32 bool) 30 true))
    (let pair:(ptr (bx.Pair i32 bool)) (call (bx.value (ptr (bx.Pair i32 bool))) bp))
    (+ (get pair (bx.Pair i32 bool) first) (if (get pair (bx.Pair i32 bool) second) 1 0)))
  (fn refs [] -> i32 (call (twice i32) (ref inc) 5))
  (fn sizes [] -> i32 (+ (* 100 (sizeof (bx.Box i64))) (sizeof (bx.Pair i32 i32)))))"#;

#[test]
fn generic_structs_and_functions_work_in_both_backends() {
    let entry = write_program("basic", &[("main.aipl", MAIN), ("box.aipl", BOX)]);
    assert_eq!(run_both(&entry, "scalars"), 42);
    assert_eq!(run_both(&entry, "structs"), 7);
    assert_eq!(run_both(&entry, "nested"), 31);
    assert_eq!(run_both(&entry, "refs"), 7);
    // Box<i64>: i64 at 0, pointer at 8, 16 bytes; Pair<i32,i32>: 8 bytes
    assert_eq!(run_both(&entry, "sizes"), 1608);
}

#[test]
fn instances_are_ordinary_named_items() {
    let entry = write_program("names", &[("main.aipl", MAIN), ("box.aipl", BOX)]);
    let m = Resolver::resolve(&entry).unwrap();
    let names: Vec<&str> = m.functions.iter().map(|f| f.name.as_str()).collect();
    for n in ["box.make<i32>", "box.value<i64>", "box.pair<i32,bool>", "box.make<ptr<box.Pair<i32,bool>>>", "twice<i32>"] {
        assert!(names.contains(&n), "{n} missing from {names:?}");
    }
    assert!(!names.contains(&"box.make"), "templates are not emitted");
    let structs: Vec<&str> = m.structs.iter().map(|s| s.name.as_str()).collect();
    assert!(structs.contains(&"box.Box<ptr<box.Pair<i32,bool>>>"), "{structs:?}");
}

#[test]
fn the_aipl_resolver_expands_generics_identically() {
    let entry = write_program("parity", &[("main.aipl", MAIN), ("box.aipl", BOX)]);
    assert_aipl_resolver_agrees(&entry);
}

fn resolve_err(src: &str) -> String {
    let entry = write_program("err", &[("main.aipl", src)]);
    match Resolver::resolve(&entry) {
        Ok(_) => panic!("expected an error"),
        Err(e) => e,
    }
}

#[test]
fn malformed_generics_are_errors() {
    let e = resolve_err("(module m (struct (B T) [v:T]) (fn f [] -> i32 (sizeof (B i32 i64))))");
    assert!(e.contains("generic 'B' takes 1 type argument(s) (T), got 2"), "{e}");
    let e = resolve_err("(module m (fn (f t) [x:t] -> t x) (fn g [] -> i32 (call (f i32) 1)))");
    assert!(e.contains("a type parameter is a name starting with an uppercase letter"), "{e}");
    let e = resolve_err("(module m (struct (B T) [v:T]) (fn f [p:(ptr (B i32))] -> i32 (get p (B i32))))");
    assert!(e.contains("a generic struct field is written (get p (Name T...) field)"), "{e}");
    let e = resolve_err("(module m (fn (f T) [x:T] -> i32 (call (f (ptr T)) x)) (fn g [] -> i32 (call (f i32) 1)))");
    assert!(e.contains("generic instance name longer than 1024 characters"), "{e}");
    let e = resolve_err("(module m (fn (get T) [x:T] -> T x))");
    assert!(e.contains("generic name 'get' is a built-in form"), "{e}");
    let e = resolve_err("(module m (fn (f T) [x:T] -> T x) (fn (f T) [x:T] -> T x))");
    assert!(e.contains("generic 'f' is defined twice"), "{e}");
    // a type error inside an instance points at the template's source line
    let entry = write_program("type_err", &[("main.aipl", "(module m\n  (fn (f T) [x:T] -> T\n    (+ x true))\n  (fn g [] -> i32 (call (f i32) 1)))")]);
    let m = Resolver::resolve(&entry).unwrap();
    let e = TypeChecker::new().check_module(&m).unwrap_err();
    assert!(e.starts_with("3:"), "{e}");
}

/// An imported module calling its own generic function with its own struct
/// type: `(alloc Item)` inside module lib must name lib.Item (it once stayed
/// `Item`, so the instance's type differed from the struct's; found by
/// std/arena's self-test when the binarytrees benchmark imported it).
#[test]
fn a_modules_own_struct_as_a_type_argument_is_qualified() {
    let entry = write_program(
        "own_struct_arg",
        &[
            ("main.aipl", "(module main (import lib) (fn main [] -> i32 (+ (call lib.use_own) (call lib.nested))))"),
            (
                "lib.aipl",
                "(module lib
                   (struct Item [x:i32 y:i64])
                   (struct (Wrap T) [inner:(ptr T)])
                   (fn (make T) [] -> (ptr T) (new T))
                   (fn use_own [] -> i32 (let it:(ptr Item) (call (make Item))) (put it Item.x 40) (+ (get it Item.x) (sizeof Item)))
                   (fn nested [] -> i32
                     (let w:(ptr (Wrap Item)) (call (make (Wrap Item))))
                     (put w (Wrap Item) inner (call (make Item)))
                     (get (get w (Wrap Item) inner) Item.x)))",
            ),
        ],
    );
    assert_eq!(run_both(&entry, "main"), 56);
    assert_aipl_resolver_agrees(&entry);
}
