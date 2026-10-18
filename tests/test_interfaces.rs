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
    // the flat program can hold constants (std/gen's), which need the resolver
    let m = Resolver::resolve_source(&flat, entry).unwrap_or_else(|e| panic!("flat output: {e}\n{flat}"));
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
  (fn (biggest T) [a:T b:T] -> T (where (cmp.Ord T))
    (if (gt (call (cmp.Ord.compare T) a b) 0) a b))
  (fn generic [] -> i32 (call (biggest i32) 4 9)))"#;

const SHAPES: &str = r#"(module shapes
  (import cmp)
  (struct Square [side:i32])
  (impl (cmp.Eq (ptr Square)) (fn same [a:(ptr Square) b:(ptr Square)] -> bool (eq (get a Square.side) (get b Square.side))))
  (fn sides [] -> i32 (pub)
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
        let entry = write_program("err", &[("main.aipl", src), ("geo.aipl", "(module geo (struct Point [x:i32] (pub)))")]);
        let rust = Resolver::resolve(&entry).map(|_| ()).expect_err(src);
        assert!(rust.ends_with(want), "{src}\n  expected: {want}\n  rust: {rust}");
        let aipl = aipl_core::selfhost::resolve_with_aipl(&entry).map(|_| ()).expect_err(src);
        assert_eq!(aipl, rust, "{src}");
    }
}

/// Where clauses (IC2): a template asks for implementations, and every use
/// of it is checked for them.
const WHERE: &str = r#"(module main
  (import cmp)
  (struct (Set K) [items:(arr K) n:i32] (where (cmp.Eq K)))
  (fn (contains K) [xs:(arr K) x:K] -> bool (where (cmp.Eq K))
    (loop i 0 (- (arr.len xs) 1) 1
      (if (call (cmp.Eq.same K) (arr.get K xs i) x) (return true) (block)))
    false)
  (fn (largest T) [xs:(arr T)] -> T (where (cmp.Ord T)) (where (cmp.Eq T)) (req (gt (arr.len xs) 0))
    (let best:T (arr.get T xs 0))
    (loop i 1 (- (arr.len xs) 1) 1
      (if (gt (call (cmp.Ord.compare T) (arr.get T xs i) best) 0) (set! best (arr.get T xs i)) (block)))
    best)
  (fn main [] -> i32
    (let xs:(arr i64) (arr.new i64 3))
    (arr.set i64 xs 1 7i64)
    (let s:(ptr (Set str)) (new (Set str)))
    (put s (Set str) n 100)
    (+ (get s (Set str) n) (+ (if (call (contains i64) xs 7i64) 10 0) (i32.wrap (call (largest i64) xs))))))"#;

#[test]
fn where_clauses_work_in_both_backends() {
    let entry = write_program("where", &[("main.aipl", WHERE)]);
    assert_eq!(run_both(&entry, "main"), 117);
    assert_aipl_resolver_agrees(&entry);
}

#[test]
fn where_clauses_are_checked_in_both_resolvers() {
    let cases: &[(&str, &str)] = &[
        // a use without the implementation asked for
        ("(module m (import cmp) (struct P [x:i32]) (fn (has K) [xs:(arr K) x:K] -> bool (where (cmp.Eq K)) false) (fn main [] -> i32 (if (call (has (ptr P)) (arr.new (ptr P) 1) (ptr.null P)) 1 0)))",
         "1:135: has<ptr<P>> needs (cmp.Eq (ptr P)): write (impl (cmp.Eq (ptr P)) ...)"),
        ("(module m (import cmp) (struct P [x:i32]) (struct (S K) [k:K] (where (cmp.Hash K))) (fn main [] -> i32 (sizeof (S (ptr P)))))",
         "1:112: S<ptr<P>> needs (cmp.Hash (ptr P)): write (impl (cmp.Hash (ptr P)) ...)"),
        // where clauses only on templates, and right after the signature
        ("(module m (import cmp) (fn f [x:i32] -> i32 (where (cmp.Eq i32)) x) (fn main [] -> i32 0))",
         "1:45: only a generic function, struct, or union takes where clauses"),
        ("(module m (import cmp) (fn (f T) [x:T] -> T (req true) (where (cmp.Eq T)) x) (fn main [] -> i32 (call (f i32) 1)))",
         "1:56: where clauses come right after the signature: after a function's return type, a struct's fields, or a union's variants"),
        ("(module m (import cmp) (struct (S K) (where (cmp.Eq K)) [k:K]) (fn main [] -> i32 0))",
         "1:38: where clauses come right after the signature: after a function's return type, a struct's fields, or a union's variants"),
        // the clause's own form
        ("(module m (import cmp) (fn (f T) [x:T] -> T (where cmp.Eq) x) (fn main [] -> i32 (call (f i32) 1)))",
         "1:45: a where clause is written (where (Interface T)), (where (struct T)), or (where (plain T)), T a type parameter of this generic"),
        ("(module m (import cmp) (fn (f T) [x:T] -> T (where (cmp.Eq U)) x) (fn main [] -> i32 (call (f i32) 1)))",
         "1:45: a where clause is written (where (Interface T)), (where (struct T)), or (where (plain T)), T a type parameter of this generic"),
        ("(module m (fn (f T) [x:T] -> T (where (Nope T)) x) (fn main [] -> i32 (call (f i32) 1)))",
         "1:32: 'Nope' is not an interface"),
    ];
    for (src, want) in cases {
        let entry = write_program("where_err", &[("main.aipl", src)]);
        let rust = Resolver::resolve(&entry).map(|_| ()).expect_err(src);
        assert!(rust.ends_with(want), "{src}\n  expected: {want}\n  rust: {rust}");
        let aipl = aipl_core::selfhost::resolve_with_aipl(&entry).map(|_| ()).expect_err(src);
        assert_eq!(aipl, rust, "{src}");
    }
}

/// Templates checked once (IC3): every template at placeholders for its
/// type parameters, so an error in a template is found in the template,
/// used or not; and each use passes types of the right kind. Both toolchains,
/// the whole way (the AIPL one is driver.aipl in the VM), with the same
/// messages.
#[test]
fn templates_are_checked_once_in_both_toolchains() {
    let cases: &[(&str, &str)] = &[
        (r#"(module m (fn (f T) [a:T b:T] -> bool (eq a b)) (fn main [] -> i32 0))"#, "1:39: eq on T: a type parameter has no equality of its own; add (where (cmp.Eq T)) and call (cmp.Eq.same T)"),
        (r#"(module m (fn (f T) [a:T b:T] -> bool (lt a b)) (fn main [] -> i32 0))"#, "1:39: lt on T: a type parameter has no order of its own; add (where (cmp.Ord T)) and call (cmp.Ord.compare T)"),
        (r#"(module m (fn (f T) [a:T] -> T (+ a a)) (fn main [] -> i32 0))"#, "1:32: + on T: a type parameter has no arithmetic"),
        (r#"(module m (fn (f T) [a:T] -> i32 (match a (else 0))) (fn main [] -> i32 0))"#, "1:34: match on T: a type parameter has no variants or members to match"),
        (r#"(module m (fn (f T) [a:T] -> i32 (enum.ord a)) (fn main [] -> i32 0))"#, "1:34: enum.ord on T: a type parameter is not an enum"),
        (r#"(module m (fn (f T) [p:(ptr T)] -> i32 0) (fn main [] -> i32 0))"#, "1:11: T is a type parameter that is not a struct: to use it as one, add (where (struct T)) (parameter 'p' of 'f<T>')"),
        (r#"(module m (fn (f T) [x:T] -> i32 (where (struct T)) 0) (fn main [] -> i32 0))"#, "1:11: 'T' is a struct, which is only used through a pointer: write (ptr T) (parameter 'x' of 'f<T>')"),
        (r#"(module m (import vec) (fn (g T) [v:(ptr (vec.Vec T)) x:T] -> i32 (call (vec.index_of T) v x)) (fn main [] -> i32 0))"#, "1:73: vec.index_of<T> needs (cmp.Eq T): add (where (cmp.Eq T)) to g"),
        (r#"(module m (import cmp) (fn (g T) [a:T] -> bool (call (cmp.Eq.same T) a a)) (fn main [] -> i32 0))"#, "1:54: no implementation of (cmp.Eq T): add (where (cmp.Eq T)) to g"),
        (r#"(module m (import alloc) (fn (g T) [] -> (ptr T) (call (alloc.create T) (call alloc.default))) (fn main [] -> i32 0))"#, "1:56: alloc.create<T>: T must be a struct, so g needs (where (struct T))"),
        (r#"(module m (import vec) (struct Point [x:i32]) (fn main [] -> i32 (sizeof (vec.Vec Point))))"#, "1:74: vec.Vec<Point>: T is a value type, and Point is a struct, used only through a pointer: write (ptr Point)"),
        (r#"(module m (import alloc) (fn main [] -> i32 (sizeof (ptr.null i32)) (i32.wrap 0i64) (let p:(ptr i32) (call (alloc.create i32) (call alloc.default))) 0))"#, "1:108: alloc.create<i32>: T must be a struct (where (struct T)), and i32 is not one"),
        (r#"(module m (import option) (fn main [] -> i32 (let o:(option.Option void) (make (option.Option void) none)) 0))"#, "1:53: option.Option<void>: T is a value type, and void cannot be stored"),
        (r#"(module m (fn (f T) [] -> (ptr T) (ptr.null T)) (fn main [] -> i32 0))"#, "1:11: T is a type parameter that is not a struct: to use it as one, add (where (struct T)) (return type of 'f<T>')"),
        (r#"(module m (fn (f T) [a:T] -> i32 (req 5) 0) (fn main [] -> i32 0))"#, "1:39: Contract expression in 'f<T>' must evaluate to bool, got i32"),
        (r#"(module m (fn (g T) [a:T] -> i32 0) (fn (f T) [a:T] -> i32 (call (g T) 5)) (fn main [] -> i32 0))"#, "1:60: Arg 0 of 'g<T>' expects T, got i32"),
        (r#"(module m (fn (g T) [a:T] -> i32 0) (fn (f T) [a:T] -> i32 (call (g T))) (fn main [] -> i32 0))"#, "1:60: Function 'g<T>' expects 1 arguments, got 0"),
        (r#"(module m (union (B T) [(full v:T) (empty)]) (fn (f T) [] -> (B T) (make (B T) full 5)) (fn main [] -> i32 0))"#, "1:68: make B<T>.full: field 'v' is T, got i32"),
        (r#"(module m (union (B T) [(full v:T) (empty)]) (fn (f T) [b:(B T)] -> i32 (match b ((B T) nope 0) (else 1))) (fn main [] -> i32 0))"#, "1:82: B<T> has no member 'nope'"),
        (r#"(module m (struct (S T) [v:T]) (fn (f T) [p:(ptr (S T))] -> void (put p (S T) v 5)) (fn main [] -> i32 0))"#, "1:66: Type mismatch writing to field 'S<T>.v': expected T, got i32"),
        (r#"(module m (fn (u T) [a:T] -> i32 (unsafe) 0) (fn (f T) [a:T] -> i32 (call (u T) a)) (fn main [] -> i32 0))"#, "1:69: 'u<T>' is marked (unsafe): use it inside (unsafe ...), or mark this function (unsafe)"),
        (r#"(module m (fn (f T) [a:T] -> i32 a) (fn main [] -> i32 0))"#, "1:11: Function 'f<T>' expects return type i32, but body returned T"),
        (r#"(module m (struct (S T) [v:T v:T]) (fn main [] -> i32 0))"#, "1:11: Duplicate field 'v' in struct 'S<T>'"),
        (r#"(module m (union (B T) [(full v:T v:T)]) (fn main [] -> i32 0))"#, "1:11: variant 'B<T>.full' has two fields named 'v'"),
        (r#"(module m (fn (f T) [a:T] -> i32 (let x:T 5) 0) (fn main [] -> i32 0))"#, "1:34: Type mismatch in 'let': expected T, got i32"),
        (r#"(module m (fn (f T) [a:T] -> T (return 5)) (fn main [] -> i32 0))"#, "1:32: return value has type i32, but the function returns T"),
        (r#"(module m (union (B T) [(full v:T) (empty)]) (fn (f T) [b:(B T)] -> i32 (match b ((B T) full [v] 0))) (fn main [] -> i32 0))"#, "1:73: match is missing B<T>.empty (add those arms or an else arm)"),
        (r#"(module m (struct (S T) [v:T]) (fn (f T) [p:(ptr (S T))] -> i32 (get p (S T) w)) (fn main [] -> i32 0))"#, "1:65: Struct 'S<T>' has no field 'w'"),
    ];
    std::thread::scope(|scope| {
        for (k, (src, want)) in cases.iter().enumerate() {
            scope.spawn(move || {
                let entry = write_program(&format!("sealed_{k}"), &[("main.aipl", src)]);
                let rust = Resolver::resolve(&entry).and_then(|m| TypeChecker::new().check_module(&m)).expect_err(src);
                assert!(rust.ends_with(want), "{src}\n  expected: {want}\n  rust: {rust}");
                let aipl = aipl_core::selfhost::compile_with_aipl(&entry).map(|_| ()).expect_err(src);
                assert_eq!(aipl, rust, "{src}");
            });
        }
    });
}

/// (where (plain T)): a type argument holding a pointer or an array, at
/// any depth, is refused in both toolchains with the same message; plain
/// ones (scalars, str, enums, function references, and value
/// structs and unions of plain fields, generic ones too) are accepted and run.
#[test]
fn plain_type_arguments_are_checked_in_both_toolchains() {
    let header = "(module m (struct Box [n:i32]) (struct Holds [b:(ptr Box)] (value)) (struct Deep [h:Holds] (value)) (struct Ok2 [x:i32 y:f64] (value)) (union U [(a p:(ptr Box)) (b)]) (union V [(a x:f64) (b)]) (enum E [red green]) (struct (Pair A B) [a:A b:B] (value)) (struct (Wrap T) [p:(Pair T i32)] (value)) (union (Opt T) [(none) (some v:T)]) (fn (need T) [x:T] -> i32 (where (plain T)) 7)";
    let refused: &[(&str, &str)] = &[
        ("(ptr Box)", "(call (need (ptr Box)) (new Box))"),
        ("(arr i32)", "(call (need (arr i32)) (arr.new i32 1))"),
        ("Holds", "(call (need Holds) (make Holds (new Box)))"),
        ("Deep", "(call (need Deep) (make Deep (make Holds (new Box))))"),
        ("U", "(call (need U) (make U.b))"),
        // generic instances are looked into, at any depth
        ("Pair<ptr<Box>,i32>", "(call (need (Pair (ptr Box) i32)) (make (Pair (ptr Box) i32) (new Box) 1))"),
        ("Wrap<arr<i32>>", "(call (need (Wrap (arr i32))) (make (Wrap (arr i32)) (make (Pair (arr i32) i32) (arr.new i32 1) 1)))"),
        ("Opt<ptr<Box>>", "(call (need (Opt (ptr Box))) (make (Opt (ptr Box)) none))"),
        ("Opt<Wrap<Pair<i32,arr<f64>>>>", "(call (need (Opt (Wrap (Pair i32 (arr f64))))) (make (Opt (Wrap (Pair i32 (arr f64)))) none))"),
    ];
    for (k, (ty, call)) in refused.iter().enumerate() {
        let src = format!("{header} (fn main [] -> i32 {call}))");
        let entry = write_program(&format!("plain_no_{k}"), &[("main.aipl", &src)]);
        let rust = Resolver::resolve(&entry).and_then(|m| TypeChecker::new().check_module(&m)).expect_err(&src);
        let want = format!("T must be plain (where (plain T)), and {ty} may hold a pointer or an array");
        assert!(rust.contains(&want), "{src}\n  expected: {want}\n  rust: {rust}");
        let aipl = aipl_core::selfhost::compile_with_aipl(&entry).map(|_| ()).expect_err(&src);
        assert_eq!(aipl, rust, "{src}");
    }
    // inside a template: its own parameter needs the clause too
    let src = format!("{header} (fn (wrap T) [x:T] -> i32 (call (need T) x)) (fn main [] -> i32 0))");
    let entry = write_program("plain_sealed", &[("main.aipl", &src)]);
    let rust = Resolver::resolve(&entry).and_then(|m| TypeChecker::new().check_module(&m)).expect_err(&src);
    assert!(rust.ends_with("need<T>: T must be plain, so wrap needs (where (plain T))"), "{rust}");
    assert_eq!(aipl_core::selfhost::compile_with_aipl(&entry).map(|_| ()).expect_err(&src), rust);
    // plain ones: 7 each (a result is never a type argument; enums are
    // plain too, but this helper's flat output keeps their names)
    let src = format!(
        "{header} (fn (wrap T) [x:T] -> i32 (where (plain T)) (call (need T) x)) (fn main [] -> i32 (+ (call (need i32) 1) (+ (call (need str) \"s\") (+ (call (need Ok2) (make Ok2 1 2.0)) (+ (call (need V) (make V.b)) (+ (call (need (fn [i32] -> i32)) (ref main2)) (call (wrap i64) 5i64))))))) (fn main2 [x:i32] -> i32 x))"
    );
    let entry = write_program("plain_yes", &[("main.aipl", &src)]);
    assert_eq!(run_both(&entry, "main"), 42);
    assert_aipl_resolver_agrees(&entry);
    // and generic ones whose fields, with the arguments put in, are plain
    let src = format!(
        "{header} (fn main [] -> i32 (+ (call (need (Pair i32 f64)) (make (Pair i32 f64) 1 2.0)) (+ (call (need (Wrap f64)) (make (Wrap f64) (make (Pair f64 i32) 1.0 2))) (call (need (Opt (Pair i32 i32))) (make (Opt (Pair i32 i32)) none))))))"
    );
    let entry = write_program("plain_generic_yes", &[("main.aipl", &src)]);
    assert_eq!(run_both(&entry, "main"), 21);
    assert_aipl_resolver_agrees(&entry);
}

/// What sealing allows: a template using only what its kinds and where
/// clauses give it, a recursive generic union, a struct parameter, and a
/// template never used, all checking in both toolchains and running.
#[test]
fn templates_within_their_clauses_check_and_run() {
    let src = r#"(module main
  (import cmp)
  (import alloc)
  (union (Tree T) [(leaf) (node l:(Tree T) v:T r:(Tree T))])
  (fn (insert T) [t:(Tree T) x:T] -> (Tree T) (where (cmp.Ord T))
    (match t
      ((Tree T) leaf (make (Tree T) node (make (Tree T) leaf) x (make (Tree T) leaf)))
      ((Tree T) node [l v r]
        (if (lt (call (cmp.Ord.compare T) x v) 0)
            (make (Tree T) node (call (insert T) l x) v r)
            (make (Tree T) node l v (call (insert T) r x))))))
  (fn (size T) [t:(Tree T)] -> i32
    (match t ((Tree T) leaf 0) ((Tree T) node [l _ r] (+ 1 (+ (call (size T) l) (call (size T) r))))))
  (struct Cell [n:i32])
  (fn (fresh S) [] -> (ptr S) (where (struct S)) (call (alloc.create S) (call alloc.default)))
  (fn (never_used T) [x:T] -> T x)
  (fn main [] -> i32
    (let t:(Tree str) (make (Tree str) leaf))
    (set! t (call (insert str) t "pear"))
    (set! t (call (insert str) t "apple"))
    (set! t (call (insert str) t "fig"))
    (let c:(ptr Cell) (call (fresh Cell)))
    (put c Cell.n 40)
    (+ (get c Cell.n) (call (size str) t))))"#;
    let entry = write_program("sealed_ok", &[("main.aipl", src)]);
    assert_eq!(run_both(&entry, "main"), 43);
    let rust = WasmCompiler::compile(&Resolver::resolve(&entry).unwrap()).unwrap();
    assert_eq!(aipl_core::selfhost::compile_with_aipl(&entry).unwrap(), rust);
}
