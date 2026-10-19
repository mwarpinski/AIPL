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
    // resolved again, not just parsed: the flat program keeps its constants
    // (std/gen's, through std/alloc), which resolving expands
    let flat_file = entry.with_file_name("flat.aipl");
    std::fs::write(&flat_file, &flat).unwrap();
    let m = Resolver::resolve(&flat_file).unwrap_or_else(|e| panic!("flat output: {e}\n{flat}"));
    TypeChecker::new().check_module(&m).unwrap_or_else(|e| panic!("flat output does not check: {e}\n{flat}"));
    // positions are in the flat text there: compare without them
    let same = |w: &[u8]| aipl_core::compiler::wasm::without_lines(w);
    assert!(same(&WasmCompiler::compile(&m).unwrap()) == same(&rust_bytes(entry)), "AIPL and Rust resolvers disagree:\n{flat}");
}

const BOX: &str = r#"(module box (import alloc)
  (struct (Box T) [value:T next:(ptr (Box T))] (pub))
  (fn (make T) [v:T] -> (ptr (Box T)) (pub)
    (let b:(ptr (Box T)) (call (alloc.create (Box T)) (call alloc.forever)))
    (put b (Box T) value v)
    (put b (Box T) next (ptr.null (Box T)))
    b)
  (fn (value T) [b:(ptr (Box T))] -> T (pub) (get b (Box T) value))
  ;; a generic calling a generic, and a two-parameter struct
  (struct (Pair A B) [(read first:A) (read second:B)] (pub))
  (fn (pair A B) [a:A b:B] -> (ptr (Pair A B))
    (let p:(ptr (Pair A B)) (call (alloc.create (Pair A B)) (call alloc.forever)))
    (put p (Pair A B) first a)
    (put p (Pair A B) second b)
    p)
  (fn (boxed_pair A B) [a:A b:B] -> (ptr (Box (ptr (Pair A B)))) (pub)
    (call (make (ptr (Pair A B))) (call (pair A B) a b))))"#;

const MAIN: &str = r#"(module main (import alloc)
  (import box as bx)
  (struct Point [x:i32 y:i32])
  (fn (twice T) [f:(fn [T] -> T) x:T] -> T (call_ref (fn [T] -> T) f (call_ref (fn [T] -> T) f x)))
  (fn inc [x:i32] -> i32 (+ x 1))
  (fn scalars [] -> i32
    (let a:(ptr (bx.Box i32)) (call (bx.make i32) 40))
    (let b:(ptr (bx.Box i64)) (call (bx.make i64) 2i64))
    (+ (call (bx.value i32) a) (i32.wrap (call (bx.value i64) b))))
  (fn structs [] -> i32
    (let p:(ptr Point) (call (alloc.create Point) (call alloc.forever)))
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
    // a type error inside a template is found in the template, at its file
    // and line, whatever it is used with (it is checked once, sealed)
    let entry = write_program("type_err", &[("main.aipl", "(module m\n  (fn (f T) [x:T] -> T\n    (+ x true))\n  (fn g [] -> i32 (call (f i32) 1)))")]);
    let e = Resolver::resolve(&entry).map(|_| ()).unwrap_err();
    assert_eq!(e, format!("{}: 3:5: + on T: a type parameter has no arithmetic", entry.display()));
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
                "(module lib (import alloc)
                   (struct Item [x:i32 y:i64])
                   (struct (Wrap T) [inner:(ptr T)] (where (struct T)))
                   (fn (make T) [] -> (ptr T) (where (struct T)) (call (alloc.create T) (call alloc.forever)))
                   (fn use_own [] -> i32 (pub) (let it:(ptr Item) (call (make Item))) (put it Item.x 40) (+ (get it Item.x) (sizeof Item)))
                   (fn nested [] -> i32 (pub)
                     (let w:(ptr (Wrap Item)) (call (make (Wrap Item))))
                     (put w (Wrap Item) inner (call (make Item)))
                     (get (get w (Wrap Item) inner) Item.x)))",
            ),
        ],
    );
    assert_eq!(run_both(&entry, "main"), 56);
    assert_aipl_resolver_agrees(&entry);
}

/// Generic unions (AIPL_SPEC.md 4.J): `(union (Name T...) [...])`, built with
/// `(make (Name t...) variant ...)` and taken apart with the arm
/// `((Name t...) variant [binders] ...)`; across an import and an alias,
/// recursive (the rest behind a pointer to a generic struct), with 64-bit
/// and float fields, and std/option.
const UNIONS: &str = r#"(module main
  (import option)
  (import shapes as sh)
  (import alloc)
  (union (List T) [(cons head:T tail:(ptr (Rest T))) (nil)])
  (struct (Rest T) [l:(List T)])

  (fn (cons T) [a:alloc.Allocator h:T t:(List T)] -> (List T)
    (let r:(ptr (Rest T)) (call (alloc.create (Rest T)) a))
    (put r (Rest T) l t)
    (make (List T) cons h r))
  (fn (sum T) [l:(List T) f:(fn [T] -> i64)] -> i64
    (match l
      ((List T) cons [h t] (+ (call_ref (fn [T] -> i64) f h) (call (sum T) (get t (Rest T) l) f)))
      ((List T) nil 0i64)))
  (fn id [x:i64] -> i64 x)
  (fn tenth [x:f64] -> i64 (i64.trunc_f64_s (* x 10.0)))

  (fn lists [] -> i32
    (let al:alloc.Allocator (call alloc.default))
    (let a:(List i64) (call (cons i64) al 40i64 (call (cons i64) al 2i64 (make (List i64) nil))))
    (let b:(List f64) (call (cons f64) al 0.5 (make (List f64) nil)))
    (i32.wrap (+ (call (sum i64) a (ref id)) (call (sum f64) b (ref tenth)))))

  (fn options [] -> i32
    (let some:(option.Option i32) (make (option.Option i32) some 7))
    (let none:(option.Option i32) (make (option.Option i32) none))
    (+ (call (option.unwrap_or i32) some 0)
       (+ (call (option.unwrap_or i32) none 30)
          (if (call (option.is_none i32) none) 100 0))))

  (fn imported [] -> i32
    (let e:(sh.Either i32 (ptr sh.Point)) (call sh.right_point 5))
    (match e
      ((sh.Either i32 (ptr sh.Point)) left [n] n)
      ((sh.Either i32 (ptr sh.Point)) right [p] (get p sh.Point.x))))

  (fn nested [] -> i32
    (let o:(option.Option (option.Option bool)) (make (option.Option (option.Option bool)) some (make (option.Option bool) some true)))
    (match o
      ((option.Option (option.Option bool)) some [inner]
        (match inner ((option.Option bool) some [b] (if b 1 2)) (else 3)))
      (else 4))))"#;

const SHAPES: &str = r#"(module shapes (import alloc)
  (struct Point [(read x:i32) y:i32] (pub))
  (union (Either L R) [(left value:L) (right value:R)] (pub))
  (fn right_point [x:i32] -> (Either i32 (ptr Point)) (pub)
    (let p:(ptr Point) (call (alloc.create Point) (call alloc.forever)))
    (put p Point.x x)
    (make (Either i32 (ptr Point)) right p)))"#;

#[test]
fn generic_unions_work_in_both_backends() {
    let entry = write_program("unions", &[("main.aipl", UNIONS), ("shapes.aipl", SHAPES)]);
    assert_eq!(run_both(&entry, "lists"), 47);
    assert_eq!(run_both(&entry, "options"), 137);
    assert_eq!(run_both(&entry, "imported"), 5);
    assert_eq!(run_both(&entry, "nested"), 1);
    let m = Resolver::resolve(&entry).unwrap();
    let unions: Vec<&str> = m.unions.iter().map(|u| u.name.as_str()).collect();
    for n in ["List<i64>", "List<f64>", "option.Option<i32>", "option.Option<option.Option<bool>>", "shapes.Either<i32,ptr<shapes.Point>>"] {
        assert!(unions.contains(&n), "{n} missing from {unions:?}");
    }
    assert_aipl_resolver_agrees(&entry);
}

#[test]
fn malformed_generic_union_forms_are_errors_in_both_resolvers() {
    for (src, want) in [
        ("(module m (union (O T) [(some v:T) (none)]) (fn f [] -> (O i32) (make (O i32))))", "a generic union variant is written (make (Name T...) variant fields...)"),
        ("(module m (union (O T) [(some v:T) (none)]) (fn f [] -> (O i32) (make (O i32) O.some 1)))", "a generic union variant is written (make (Name T...) variant fields...)"),
        ("(module m (union (O T) [(some v:T) (none)]) (fn f [o:(O i32)] -> i32 (match o ((O i32) [v] v) (else 0))))", "a generic union arm is written ((Name T...) variant [binders] body...)"),
    ] {
        let entry = write_program("union_err", &[("main.aipl", src)]);
        let rust = Resolver::resolve(&entry).expect_err("the Rust resolver accepted it");
        assert!(rust.contains(want), "{src}\n  rust: {rust}");
        let aipl = aipl_core::selfhost::resolve_with_aipl(&entry).expect_err("the AIPL resolver accepted it");
        assert_eq!(aipl, rust, "{src}");
    }
}

/// A union's variant or an enum's member may share its name with a generic
/// function of the same module: in an imported module the member must stay
/// a member (`(custom c:...)` once became `(alloc.custom c:...)` when
/// std/alloc's `custom` became generic).
#[test]
fn members_named_like_a_template_keep_their_names() {
    let entry = write_program(
        "member_names",
        &[
            ("main.aipl", "(module main (import lib) (fn main [] -> i32 (call lib.use_both)))"),
            (
                "lib.aipl",
                "(module lib
                   (union U [(pick n:i32) (none)])
                   (enum E [red (pick2 5)])
                   (fn (pick T) [x:T] -> T x)
                   (fn (pick2 T) [x:T] -> T x)
                   (fn use_both [] -> i32 (pub)
                     (match (make U.pick (call (pick i32) 3))
                       (U.pick [n] (+ n (enum.ord E.pick2)))
                       (U.none (call (pick2 i32) 0)))))",
            ),
        ],
    );
    assert_eq!(run_both(&entry, "main"), 8);
    // the whole AIPL toolchain (this file's flat-output check cannot expand
    // enum members), byte for byte
    let rust = WasmCompiler::compile(&Resolver::resolve(&entry).unwrap()).unwrap();
    assert_eq!(aipl_core::selfhost::compile_with_aipl(&entry).unwrap(), rust);
}

/// An instance's expressions are the template's, in the template's file,
/// even where a type argument from another file sits at the same line and
/// column (the Rust toolchain once named the argument's file in the line
/// table there). Both toolchains, byte for byte, line table included.
#[test]
fn an_instance_keeps_its_template_file_where_positions_coincide() {
    let lib = "(module lib (fn g [] -> i32 1) (fn (f T) [x:T] -> i32 (pub) (call g)))";
    let col = lib.find("(call g)").unwrap() + 1;
    let head = "(module main (import lib) (fn main [] -> i32 (call (lib.f";
    let main = format!("{head}{}i32) 5)))", " ".repeat(col - 1 - head.len()));
    assert_eq!(main.find("i32) 5").unwrap() + 1, col);
    let dir = std::env::temp_dir().join(format!("aipl_coincide_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("lib.aipl"), lib).unwrap();
    std::fs::write(dir.join("main.aipl"), &main).unwrap();
    let entry = dir.join("main.aipl");
    let rust = WasmCompiler::compile(&Resolver::resolve(&entry).unwrap()).unwrap();
    assert_eq!(aipl_core::selfhost::compile_with_aipl(&entry).unwrap(), rust);
    let _ = std::fs::remove_dir_all(&dir);
}
