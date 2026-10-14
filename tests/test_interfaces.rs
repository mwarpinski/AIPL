//! Interfaces and implementations (docs/design/CONSTRAINTS_PLAN.md, IC1):
//! `(interface (Hash K) ...)`, `(impl (Hash i32) ...)`, and
//! `(call (Hash.hash i32) x)`, in both backends, with the AIPL resolver
//! (resolver.aipl + generics.aipl) agreeing with the Rust one on the
//! program and on every error.

use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::resolver::Resolver;
use aipl_core::vm::{Value, VM};
use std::path::{Path, PathBuf};
use wasmtime::{Engine, Instance, Module as WasmModule, Store};

/// Writes `files` into a fresh directory; returns the path of the first.
fn write_program(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aipl_interfaces_{}_{}", std::process::id(), name));
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

/// The AIPL resolver's flat program compiles to the Rust resolver's bytes.
fn assert_aipl_resolver_agrees(entry: &Path) {
    let flat = aipl_core::selfhost::resolve_with_aipl(entry).unwrap_or_else(|e| panic!("aipl resolve: {e}"));
    let m = aipl_core::parser::Parser::parse(&flat).unwrap_or_else(|e| panic!("flat output: {e}\n{flat}"));
    TypeChecker::new().check_module(&m).unwrap_or_else(|e| panic!("flat output does not check: {e}\n{flat}"));
    let same = |w: &[u8]| aipl_core::compiler::wasm::without_lines(w);
    let rust = WasmCompiler::compile(&Resolver::resolve(entry).unwrap()).unwrap();
    assert!(same(&WasmCompiler::compile(&m).unwrap()) == same(&rust), "AIPL and Rust resolvers disagree:\n{flat}");
}

const MAIN: &str = r#"(module main
  (import cmp)
  (import cmp as c)
  (import shapes)
  (struct Pair [a:i32 b:i32])

  ;; an interface of this module, implemented for a type of this module
  ;; and for a built-in one, with a contract every implementation keeps
  (interface (Size T)
    (fn size [x:T] -> i32 (ens (gte res 0))))
  (impl (Size (ptr Pair)) (fn size [x:(ptr Pair)] -> i32 2))
  (impl (Size str) (fn size [x:str] -> i32 (str.len x)))
  ;; std's interface, for this module's type
  (impl (cmp.Eq (ptr Pair))
    (fn same [a:(ptr Pair) b:(ptr Pair)] -> bool
      (and (eq (get a Pair.a) (get b Pair.a)) (eq (get a Pair.b) (get b Pair.b)))))

  (fn pair [a:i32 b:i32] -> (ptr Pair)
    (let p:(ptr Pair) (new Pair))
    (put p Pair.a a)
    (put p Pair.b b)
    p)

  (fn std_impls [] -> i32
    (+ (if (call (cmp.Eq.same str) "abc" "abc") 1 0)
       (+ (* 10 (call (c.Ord.compare i64) 9i64 2i64))
          (if (eq (call (cmp.Hash.hash i32) 5) (call (cmp.Hash.hash i32) 5)) 100 0))))

  (fn own [] -> i32
    (+ (call (Size.size (ptr Pair)) (call pair 1 2))
       (+ (* 10 (call (Size.size str) "four"))
          (if (call (cmp.Eq.same (ptr Pair)) (call pair 3 4) (call pair 3 4)) 100 0))))

  ;; a reference to an implementation's function, and one in another module
  (fn refs [] -> i32
    (let f:(fn [str] -> i32) (ref (Size.size str)))
    (+ (call_ref (fn [str] -> i32) f "abc") (* 10 (call shapes.sides))))

  ;; a generic function calling an implementation for its type argument
  (fn (biggest T) [a:T b:T] -> T
    (if (gt (call (cmp.Ord.compare T) a b) 0) a b))
  (fn generic [] -> i32 (call (biggest i32) 4 9)))"#;

const SHAPES: &str = r#"(module shapes
  (import cmp)
  (struct Square [side:i32])
  (impl (cmp.Eq (ptr Square)) (fn same [a:(ptr Square) b:(ptr Square)] -> bool (eq (get a Square.side) (get b Square.side))))
  (fn sides [] -> i32
    (let s:(ptr Square) (new Square))
    (if (call (cmp.Eq.same (ptr Square)) s s) 4 3)))"#;

#[test]
fn interfaces_work_in_both_backends() {
    let entry = write_program("basic", &[("main.aipl", MAIN), ("shapes.aipl", SHAPES)]);
    assert_eq!(run_both(&entry, "std_impls"), 111);
    assert_eq!(run_both(&entry, "own"), 142);
    assert_eq!(run_both(&entry, "refs"), 43);
    assert_eq!(run_both(&entry, "generic"), 9);
    let m = Resolver::resolve(&entry).unwrap();
    let names: Vec<&str> = m.functions.iter().map(|f| f.name.as_str()).collect();
    for n in ["Size<ptr<Pair>>.size", "Size<str>.size", "cmp.Eq<ptr<Pair>>.same", "cmp.Hash<i32>.hash", "cmp.Eq<ptr<shapes.Square>>.same"] {
        assert!(names.contains(&n), "{n} missing from {names:?}");
    }
    assert_aipl_resolver_agrees(&entry);
}

/// An implementation keeps the interface's contracts, reported where the
/// interface wrote them.
#[test]
fn an_implementation_keeps_the_interfaces_contracts() {
    let src = "(module m\n  (interface (H K) (fn h [k:K] -> i32 (ens (gte res 0))))\n  (impl (H i32) (fn h [k:i32] -> i32 k))\n  (fn main [] -> i32 (call (H.h i32) -5)))";
    let entry = write_program("contract", &[("main.aipl", src)]);
    let module = Resolver::resolve(&entry).unwrap();
    TypeChecker::new().check_module(&module).unwrap();
    let mut vm = VM::new();
    vm.load_module(module);
    let e = vm.invoke("main", vec![]).unwrap_err();
    assert!(e.starts_with("Post-condition failed in 'H<i32>.h' at 2:44: (ens (gte res 0)) with k = -5, res = -5"), "{e}");
}

#[test]
fn malformed_interfaces_and_implementations_are_errors_in_both_resolvers() {
    let cases: &[(&str, &str)] = &[
        (r#"(module m (interface Hash (fn h [k:i32] -> i32)) (fn main [] -> i32 0))"#, "an interface is written (interface (Name T) (fn name [params] -> type) ...)"),
        (r#"(module m (interface (H A B) (fn h [k:A] -> i32)) (fn main [] -> i32 0))"#, "an interface takes one type parameter: (interface (Name T) ...)"),
        (r#"(module m (interface (H K)) (fn main [] -> i32 0))"#, "an interface declares at least one function"),
        (r#"(module m (interface (H K) (fn h [k:K] -> i32 0)) (fn main [] -> i32 0))"#, "an interface's function has no body: its implementations have one"),
        (r#"(module m (interface (H K) (fn h [k:K] -> i32) (fn h [k:K] -> i32)) (fn main [] -> i32 0))"#, "'h' is declared twice in interface H"),
        (r#"(module m (interface (H K) (fn h [k:K])) (fn main [] -> i32 0))"#, "an interface's function is written (fn name [params] -> type contracts...)"),
        (r#"(module m (interface (H K) (fn h [k:K] -> i32)) (interface (H K) (fn h [k:K] -> i32)) (fn main [] -> i32 0))"#, "interface 'H' is defined twice"),
        (r#"(module m (impl (H i32) (fn h [k:i32] -> i32 k)) (fn main [] -> i32 0))"#, "'H' is not an interface"),
        (r#"(module m (interface (H K) (fn h [k:K] -> i32)) (impl H (fn h [k:i32] -> i32 k)) (fn main [] -> i32 0))"#, "an implementation is written (impl (Interface type) (fn name [params] -> type body...) ...)"),
        (r#"(module m (interface (H K) (fn h [k:K] -> i32)) (impl (H i32 i64) (fn h [k:i32] -> i32 k)) (fn main [] -> i32 0))"#, "an implementation names one type: (impl (Interface type) ...)"),
        (r#"(module m (interface (H K) (fn h [k:K] -> i32)) (impl (H i32) (fn h [k:i32] -> i32 k)) (impl (H i32) (fn h [k:i32] -> i32 k)) (fn main [] -> i32 0))"#, "a second implementation of H<i32>: a type has at most one implementation of each interface"),
        (r#"(module m (interface (H K) (fn h [k:K] -> i32)) (impl (H i32) (fn g [k:i32] -> i32 k)) (fn main [] -> i32 0))"#, "'g' is not a function of interface H"),
        (r#"(module m (interface (H K) (fn h [k:K] -> i32)) (impl (H i32) (fn h [k:i32] -> i32 k) (fn h [k:i32] -> i32 k)) (fn main [] -> i32 0))"#, "'h' is defined twice in this implementation"),
        (r#"(module m (interface (H K) (fn h [k:K] -> i32)) (impl (H i32) (fn h [x:i32] -> i32 x)) (fn main [] -> i32 0))"#, "'h' must be declared as in H: (fn h [k:i32] -> i32)"),
        (r#"(module m (interface (H K) (fn h [k:K] -> i32)) (impl (H i32) (fn h [k:i64] -> i32 0)) (fn main [] -> i32 0))"#, "'h' must be declared as in H: (fn h [k:i32] -> i32)"),
        (r#"(module m (interface (H K) (fn h [k:K] -> i32) (fn g [k:K] -> bool)) (impl (H i32) (fn h [k:i32] -> i32 k)) (fn main [] -> i32 0))"#, "the implementation of H<i32> is missing 'g'"),
        (r#"(module m (interface (H K) (fn h [k:K] -> i32)) (impl (H i32) 5) (fn main [] -> i32 0))"#, "an implementation's function is written (fn name [params] -> type body...)"),
        (r#"(module m (interface (H K) (fn h [k:K] -> i32)) (fn main [] -> i32 (call (H.h i64) 5i64)))"#, "no implementation of (H i64): write (impl (H i64) ...)"),
        (r#"(module m (interface (H K) (fn h [k:K] -> i32)) (impl (H i32) (fn h [k:i32] -> i32 k)) (fn main [] -> i32 (call (H.g i32) 5)))"#, "'g' is not a function of interface H"),
        (r#"(module m (interface (H K) (fn h [k:K] -> i32)) (impl (H i32) (fn h [k:i32] -> i32 k)) (fn main [] -> i32 (call (H.h i32 i64) 5)))"#, "a call to an interface's function names one type: (call (Interface.function type) args...)"),
        (r#"(module m (import cmp) (impl (cmp.Eq f64) (fn same [a:f64 b:f64] -> bool (eq a b))) (fn main [] -> i32 0))"#, "this implementation must be in the module that defines cmp.Eq, since f64 is built in"),
        (r#"(module m (import cmp) (import geo) (impl (cmp.Eq (ptr geo.Point)) (fn same [a:(ptr geo.Point) b:(ptr geo.Point)] -> bool true)) (fn main [] -> i32 0))"#, "this implementation must be in the module that defines cmp.Eq or the one that defines geo.Point"),
    ];
    for (src, want) in cases {
        let entry = write_program("err", &[("main.aipl", src), ("geo.aipl", "(module geo (struct Point [x:i32]))")]);
        let rust = Resolver::resolve(&entry).map(|_| ()).expect_err(src);
        assert!(rust.ends_with(want), "{src}\n  expected: {want}\n  rust: {rust}");
        let aipl = aipl_core::selfhost::resolve_with_aipl(&entry).map(|_| ()).expect_err(src);
        assert_eq!(aipl, rust, "{src}");
    }
}
