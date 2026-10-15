//! Visibility (AIPL_SPEC.md 11.1): items private to their module unless
//! (pub); fields private, (read x:T), or (pub x:T); a struct made outside
//! its module only when every field is (pub). Both resolvers report the
//! same first error, and what is allowed runs in both backends.

use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::resolver::Resolver;
use aipl_core::vm::{Value, VM};
use std::path::PathBuf;
use wasmtime::{Engine, Instance, Module as WasmModule, Store};

const LIB: &str = r#"(module lib
  (import cmp)
  (struct S [(read r:i32) (pub w:i32) p:i32] (pub))
  (struct Open [(pub a:i32) (pub b:i32)] (pub))
  (struct Hidden [x:i32])
  (struct (Box T) [(read v:T) q:T] (pub))
  (const MAX:i32 9)
  (const LIM:i32 3 (pub))
  (enum Color [red green] (pub))
  (enum Secret [one two])
  (union U [(a n:i32) (b)] (pub))
  (interface (Size T) (fn size [x:T] -> i32))
  (interface (Len T) (pub) (fn len [x:T] -> i32))
  (impl (Size i32) (fn size [x:i32] -> i32 x))
  (impl (Len i32) (fn len [x:i32] -> i32 x))
  (fn hidden [] -> i32 1)
  (fn shown [] -> i32 (pub) 2)
  (fn (gen T) [x:T] -> T x)
  (fn (pgen T) [x:T] -> T (pub) x)
  (fn make_s [] -> (ptr S) (pub) (new S)))"#;

/// main.aipl (importing lib) with `body` as main's, and lib.aipl.
fn program(name: &str, lib: &str, body: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aipl_visibility_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("lib.aipl"), lib).unwrap();
    std::fs::write(dir.join("main.aipl"), format!("(module main (import lib) (import alloc) (fn main [] -> i32 {body}))")).unwrap();
    dir.join("main.aipl")
}

#[test]
fn what_a_module_gives_is_usable_in_both_backends() {
    let cases: &[(&str, i32)] = &[
        (r#"(call lib.shown)"#, 2),
        (r#"(let s:(ptr lib.S) (call lib.make_s)) (get s lib.S.r)"#, 0),
        (r#"(let s:(ptr lib.S) (call lib.make_s)) (put s lib.S.w 1) (get s lib.S.w)"#, 1),
        (r#"(let o:(ptr lib.Open) (new lib.Open)) 0"#, 0),
        (r#"lib.LIM"#, 3),
        (r#"(enum.ord lib.Color.green)"#, 1),
        (r#"(match (make lib.U.a 4) (lib.U.a [n] n) (lib.U.b 0))"#, 4),
        (r#"(call (lib.pgen i32) 3)"#, 3),
        (r#"(call (lib.Len.len i32) 3)"#, 3),
    ];
    for (k, (body, want)) in cases.iter().enumerate() {
        let entry = program(&format!("ok_{k}"), LIB, body);
        let m = Resolver::resolve(&entry).unwrap_or_else(|e| panic!("{body}: {e}"));
        TypeChecker::new().check_module(&m).unwrap_or_else(|e| panic!("{body}: {e}"));
        let mut vm = VM::new();
        vm.load_module(m.clone());
        assert_eq!(vm.invoke("main", vec![]).unwrap(), Value::Int(*want as i64), "{body}");
        let wasm = WasmCompiler::compile(&m).unwrap();
        let engine = Engine::default();
        let module = WasmModule::new(&engine, &wasm).unwrap();
        let mut store = Store::new(&engine, ());
        let inst = Instance::new(&mut store, &module, &[]).unwrap();
        let got = inst.get_typed_func::<(), i32>(&mut store, "main").unwrap().call(&mut store, ()).unwrap();
        assert_eq!(got, *want, "{body}");
        // the AIPL resolver accepts it too
        aipl_core::selfhost::resolve_with_aipl(&entry).unwrap_or_else(|e| panic!("{body}: aipl: {e}"));
    }
}

#[test]
fn what_a_module_keeps_is_an_error_in_both_resolvers() {
    let cases: &[(&str, &str, &str)] = &[
        // a struct type argument lets the template make one (alloc.create)
        (LIB, r#"(let s:(ptr lib.S) (call (alloc.create lib.S) (call alloc.default))) 0"#, r#"1:100: 'lib.S' has fields private to module lib, so only lib can make one"#),
        (LIB, r#"(call lib.hidden)"#, r#"1:67: 'lib.hidden' is private to module lib: mark it (pub) there to use it here"#),
        (LIB, r#"(let s:(ptr lib.S) (call lib.make_s)) (put s lib.S.r 1) 0"#, r#"1:106: field 'r' of lib.S is read-only outside module lib"#),
        (LIB, r#"(let s:(ptr lib.S) (call lib.make_s)) (get s lib.S.p)"#, r#"1:106: field 'p' of lib.S is private to module lib"#),
        (LIB, r#"(let s:(ptr lib.S) (new lib.S)) 0"#, r#"1:85: 'lib.S' has fields private to module lib, so only lib can make one"#),
        (LIB, r#"(let h:(ptr lib.Hidden) (ptr.null lib.Hidden)) 0"#, r#"1:73: 'lib.Hidden' is private to module lib: mark it (pub) there to use it here"#),
        (LIB, r#"lib.MAX"#, r#"1:61: 'lib.MAX' is private to module lib: mark it (pub) there to use it here"#),
        (LIB, r#"(enum.ord lib.Secret.two)"#, r#"1:71: 'lib.Secret' is private to module lib: mark it (pub) there to use it here"#),
        (LIB, r#"(call (lib.gen i32) 3)"#, r#"1:68: 'lib.gen' is private to module lib: mark it (pub) there to use it here"#),
        (LIB, r#"(call (lib.Size.size i32) 3)"#, r#"1:68: 'lib.Size' is private to module lib: mark it (pub) there to use it here"#),
        (LIB, r#"(let b:(ptr (lib.Box i32)) (ptr.null (lib.Box i32))) (get b (lib.Box i32) q)"#, r#"1:135: field 'q' of lib.Box is private to module lib"#),
        (LIB, r#"(let b:(ptr (lib.Box i32)) (ptr.null (lib.Box i32))) (put b (lib.Box i32) v 1) 0"#, r#"1:135: field 'v' of lib.Box is read-only outside module lib"#),
        (r#"(module lib (fn f [] -> i32 1 (pub)))"#, r#"0"#, r#"1:31: (pub) comes right after the signature: (fn f [params] -> type (pub) ...), (struct S [fields] (pub)), (union U [variants] (pub)), (enum E [members] (pub)), (const NAME:type value (pub)), (interface (Name T) (pub) ...)"#),
        (r#"(module lib (import cmp) (impl (cmp.Eq (ptr P)) (pub) (fn same [a:(ptr P) b:(ptr P)] -> bool true)) (struct P [x:i32] (pub)))"#, r#"0"#, r#"1:49: an implementation takes no (pub): it is as visible as its interface and its type"#),
        (r#"(module lib (struct P [(pub x) y:i32] (pub)))"#, r#"0"#, r#"1:24: a field is written x:T, (read x:T), or (pub x:T)"#),
        (r#"(module lib (struct P [x:i32]) (fn f [p:(ptr P)] -> i32 (pub) 0))"#, r#"0"#, r#"1:46: 'f' is (pub) but its signature uses 'P', which is private: mark 'P' (pub) too"#),
        (r#"(module lib (struct P [x:i32]) (struct Q [(read p:(ptr P))] (pub)))"#, r#"0"#, r#"1:56: 'Q' is (pub) but its signature uses 'P', which is private: mark 'P' (pub) too"#),
        (r#"(module lib (enum E [a b]) (union U [(v e:E)] (pub)))"#, r#"0"#, r#"1:43: 'U' is (pub) but its signature uses 'E', which is private: mark 'E' (pub) too"#),
    ];
    for (k, (lib, body, want)) in cases.iter().enumerate() {
        let entry = program(&format!("err_{k}"), lib, body);
        let rust = Resolver::resolve(&entry).map(|_| ()).expect_err(body);
        assert!(rust.ends_with(want), "{body}\n  expected: {want}\n  rust: {rust}");
        let aipl = aipl_core::selfhost::resolve_with_aipl(&entry).map(|_| ()).expect_err(body);
        assert_eq!(aipl, rust, "{body}");
    }
}

/// A module parsed on its own may carry marks; the parser leaves them out.
#[test]
fn a_module_parsed_alone_ignores_its_marks() {
    let src = "(module m (struct P [(read x:i32) (pub y:i32) z:i32] (pub)) (fn main [] -> i32 (pub) (let p:(ptr P) (new P)) (put p P.x 4) (+ (get p P.x) 1)))";
    let m = aipl_core::parser::Parser::parse(src).unwrap();
    TypeChecker::new().check_module(&m).unwrap();
    let mut vm = VM::new();
    vm.load_module(m);
    assert_eq!(vm.invoke("main", vec![]).unwrap(), Value::Int(5));
}
