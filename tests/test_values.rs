//! Value structs (AIPL_SPEC.md 4.E, docs/design/VALUES_PLAN.md): the same
//! result in every backend for tests/aipl/values.aipl, every checker rule
//! with its message, visibility across modules, the kinds of generic type
//! arguments, and the printer round trip.
use aipl_core::checker::TypeChecker;
use aipl_core::parser::Parser;
use aipl_core::printer::print_module;
use aipl_core::resolver::Resolver;
use aipl_core::vm::{Value, VM};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_values_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Resolves and checks the files (name, text), the first the program; the
/// error if either fails.
fn check_files(name: &str, files: &[(&str, &str)]) -> Result<aipl_core::ast::Module, String> {
    let dir = scratch(name);
    for (f, src) in files {
        std::fs::write(dir.join(f), src).unwrap();
    }
    let m = Resolver::resolve(&dir.join(files[0].0));
    let _ = std::fs::remove_dir_all(&dir);
    let m = m?;
    TypeChecker::new().check_module(&m)?;
    Ok(m)
}

/// The programs and the checksum each one's `checksum` returns: every form, and
/// every way through control flow.
const PROGRAMS: [(&str, i64); 4] = [
    ("tests/aipl/values.aipl", 649),
    ("tests/aipl/values_flow.aipl", 219),
    ("tests/aipl/unions_by_value.aipl", 239),
    ("tests/aipl/results_by_value.aipl", 521),
];

#[test]
fn values_programs_run_in_the_vm() {
    for (path, sum) in PROGRAMS {
        let m = Resolver::resolve(&root().join(path)).unwrap();
        TypeChecker::new().check_module(&m).unwrap();
        let mut vm = VM::new();
        vm.load_module(m);
        assert_eq!(vm.invoke("checksum", vec![]).unwrap(), Value::Int(sum), "{path}");
    }
}

#[test]
fn printer_round_trips_value_structs() {
    let m = Resolver::resolve(&root().join("tests/aipl/values.aipl")).unwrap();
    let printed = print_module(&m);
    for want in [
        "(struct Vec3 [x:f64 y:f64 z:f64] (value))",
        "(arr.set Body bs 1 Body.vel Vec3.z 7.5)",
        "(put n Node.cell Cell.id 9)",
        "(with a Vec3.y 20.0)",
        "(make Vec3 1.0 2.0 3.0)",
    ] {
        assert!(printed.contains(want), "missing {want} in\n{printed}");
    }
    let reparsed = Parser::parse(&printed).unwrap();
    assert_eq!(print_module(&reparsed), printed);
    TypeChecker::new().check_module(&reparsed).unwrap();
}

/// Every rule of a value struct, with its message (after the position).
#[test]
fn value_struct_rules_have_messages() {
    let header = "(module m (import alloc)\n  (struct V [x:i32 y:i32] (value))\n  (struct P [v:V n:i32])\n  ";
    let cases: &[(&str, &str)] = &[
        ("(struct E [] (value)) (fn main [] -> i32 0)", "value struct 'E' has no fields"),
        ("(struct S [s:str] (value)) (fn main [] -> i32 0)", "Field 's' of value struct 'S': a value struct holds numbers, bools, enums, pointers, arrays, function references, and value structs, not str"),
        ("(union U [(a)]) (struct S [u:U] (value)) (fn main [] -> i32 0)", "Field 'u' of value struct 'S': a value struct holds numbers, bools, enums, pointers, arrays, function references, and value structs, not U"),
        ("(struct R [a:i32 r:R] (value)) (fn main [] -> i32 0)", "value struct 'R' contains itself (through field 'r'): a value has a fixed size; hold it through a pointer"),
        ("(struct A [b:B] (value)) (struct B [a:A] (value)) (fn main [] -> i32 0)", "value struct 'A' contains itself (through field 'b'): a value has a fixed size; hold it through a pointer"),
        ("(struct Big [a:V b:V c:V d:V e:i32] (value)) (fn main [] -> i32 0)", "value struct 'Big' holds 9 scalars, counting the value structs in it; at most 8: keep a larger record behind a pointer"),
        ("(fn main [] -> i32 (let p:(ptr V) (ptr.null V)) 0)", "'V' is a value struct, which has no address: write V, not (ptr V)"),
        ("(fn main [] -> i32 (let v:V (new V (call alloc.forever))) 0)", "alloc.create<V>: T must be a struct used through a pointer (where (struct T)), and V is a value struct, which has no address"),
        ("(fn main [] -> i32 (let v:V (make V 1)) 0)", "make V takes 2 field value(s) (x y), got 1"),
        ("(fn main [] -> i32 (let v:V (make V 1 true)) 0)", "make V: field 'y' is i32, got bool"),
        ("(fn main [] -> i32 (let v:V (make V 1 2)) (put v V.x 3) 0)", "a value struct's fields do not change: (with v V.x x) is a copy with one changed"),
        ("(fn main [] -> i32 (let v:V (make V 1 2)) (if (eq v v) 1 0))", "eq on value struct 'V': value structs do not compare; implement cmp.Eq for it (section 4.H) and call (cmp.Eq.same V)"),
        ("(fn main [] -> i32 (let v:V (make V 1 2)) (let w:V (with v V.z 3)) 0)", "Struct 'V' has no field 'z'"),
        ("(fn main [] -> i32 (let v:V (make V 1 2)) (let w:V (with v V.x true)) 0)", "with V.x: expected i32, got bool"),
        ("(fn main [] -> i32 (let p:(ptr P) (new P (call alloc.forever))) (let w:V (with p V.x 1)) 0)", "with V.x needs a V, got (ptr P)"),
        ("(fn main [] -> i32 (let p:(ptr P) (new P (call alloc.forever))) (let w:(ptr P) (with p P.n 1)) 0)", "with makes a copy of a value struct, and 'P' is not one: change a field with (put p P.n x)"),
        ("(fn main [] -> i32 (let p:(ptr P) (new P (call alloc.forever))) (get p V.x))", "get V.x needs a V, got (ptr P)"),
        ("(fn main [] -> i32 (let p:(ptr P) (new P (call alloc.forever))) (put p P.n V.x 3) 0)", "put path: P.n is i32, not the value struct V"),
        ("(fn main [] -> i32 (let p:(ptr P) (new P (call alloc.forever))) (put p P.v V.x true) 0)", "Type mismatch writing to field 'V.x': expected i32, got bool"),
        ("(fn main [] -> i32 (let a:(arr i32) (arr.new i32 (call alloc.forever) 2)) (arr.set i32 a 0 V.x 3) 0)", "arr.set path: the elements are i32, not the value struct V"),
        ("(fn main [] -> i32 (let a:(arr V) (arr.new V (call alloc.forever) 2)) (arr.set V a 0 V.x true) 0)", "arr.set value mismatch: expected i32, got bool"),
        ("(fn main [] -> i32 (make P 1 2))", "make names a value struct or a union variant (Union.variant), got P"),
        ("(fn main [] -> i32 (let a:V (make V 1 2)) (put (new P (call alloc.forever)) P.v (get a V.x)) 0)", "Type mismatch writing to field 'P.v': expected V, got i32"),
    ];
    for (k, (body, want)) in cases.iter().enumerate() {
        let src = format!("{}{})", header, body);
        let err = check_files(&format!("rule{k}"), &[("m.aipl", &src)]).expect_err(body);
        let msg = err.split_once(": ").map(|(_, m)| m).unwrap_or(&err);
        let msg = msg.split_once(": ").filter(|(at, _)| at.bytes().all(|b| b.is_ascii_digit() || b == b':')).map_or(msg, |(_, m)| m);
        assert_eq!(msg, *want, "{body}");
    }
    // and what is allowed
    for body in [
        "(fn main [] -> i32 (let p:(ptr P) (new P (call alloc.forever))) (get p P.v V.x))",
        "(fn main [] -> i32 (let a:(arr V) (arr.new V (call alloc.forever) 2)) (arr.set V a 0 V.y 3) (arr.get V a 0 V.y))",
        "(fn f [v:V] -> V (with v V.x (+ (get v V.x) 1))) (fn main [] -> i32 (get (call f (make V 1 2)) V.x))",
        "(struct W [a:V b:(ptr P) c:(arr V) d:(fn [V] -> V)] (value)) (fn main [] -> i32 (sizeof W))",
    ] {
        let src = format!("{}{})", header, body);
        check_files("allowed", &[("m.aipl", &src)]).unwrap_or_else(|e| panic!("{body}: {e}"));
    }
}

/// Outside its module, a value struct is made only if every field is
/// (pub); `with` writes, so its field must be (pub); `get` and paths read.
#[test]
fn value_structs_across_modules() {
    let geo = "(module geo
      (struct Pt [(pub x:i32) (pub y:i32)] (value) (pub))
      (struct Sealed [(read a:i32) b:i32] (value) (pub))
      (struct Holder [(pub p:Pt) (pub s:Sealed)] (pub))
      (fn sealed [n:i32] -> Sealed (pub) (make Sealed n (* n 2)))
      (fn moved [p:Pt dx:i32] -> Pt (pub) (with p Pt.x (+ (get p Pt.x) dx))))";
    let main = |body: &str| {
        format!(
            "(module main (import alloc) (import geo)
               (fn main [] -> i32
                 (let p:geo.Pt (make geo.Pt 3 4))
                 (let s:geo.Sealed (call geo.sealed 5))
                 (let h:(ptr geo.Holder) (new geo.Holder (call alloc.forever)))
                 {body}
                 0))"
        )
    };
    for ok in [
        "(let q:geo.Pt (with (call geo.moved p 1) geo.Pt.y 9))",
        "(let a:i32 (get s geo.Sealed.a))",
        "(put h geo.Holder.p geo.Pt.x 5)",
        "(let a:i32 (get h geo.Holder.s geo.Sealed.a))",
    ] {
        check_files("vis_ok", &[("main.aipl", &main(ok)), ("geo.aipl", geo)]).unwrap_or_else(|e| panic!("{ok}: {e}"));
    }
    for (bad, want) in [
        ("(let t:geo.Sealed (make geo.Sealed 1 2))", "'geo.Sealed' has fields private to module geo, so only geo can make one"),
        ("(let t:geo.Sealed (with s geo.Sealed.a 1))", "field 'a' of geo.Sealed is read-only outside module geo"),
        ("(let b:i32 (get s geo.Sealed.b))", "field 'b' of geo.Sealed is private to module geo"),
        ("(let b:i32 (get h geo.Holder.s geo.Sealed.b))", "field 'b' of geo.Sealed is private to module geo"),
        ("(put h geo.Holder.s geo.Sealed.a 1)", "field 'a' of geo.Sealed is read-only outside module geo"),
    ] {
        let err = check_files("vis_bad", &[("main.aipl", &main(bad)), ("geo.aipl", geo)]).expect_err(bad);
        assert!(err.ends_with(want), "{bad}: {err}");
    }
}

/// A value struct is a type argument of the value kind, never of the
/// struct kind (which takes a pointer to it).
#[test]
fn value_structs_as_type_arguments() {
    let ok = "(module m (import vec) (import alloc)
      (struct V [x:i32 y:i32] (value))
      (struct (Pair T) [a:T b:T] (value))
      (fn main [] -> i32
        (let v:(ptr (vec.Vec V)) (call (vec.make V) (call alloc.default) 2))
        (call (vec.push V) v (make V 1 2))
        (let p:(Pair V) (make (Pair V) (make V 3 4) (call (vec.at V) v 0)))
        (+ (get (get p (Pair V) a) V.x) (sizeof (Pair V)))))";
    let m = check_files("kinds_ok", &[("m.aipl", ok)]).unwrap();
    let mut vm = VM::new();
    vm.load_module(m);
    assert_eq!(vm.invoke("main", vec![]).unwrap(), Value::Int(3 + 16));
    let bad = "(module m (import alloc)
      (struct V [x:i32] (value))
      (fn main [] -> i32 (let p:(ptr V) (new V (call alloc.default))) 0))";
    let err = check_files("kinds_bad", &[("m.aipl", bad)]).unwrap_err();
    assert!(err.ends_with("alloc.create<V>: T must be a struct used through a pointer (where (struct T)), and V is a value struct, which has no address"), "{err}");
    // (pub) after (value), not before
    let misplaced = "(module lib (struct V [x:i32] (pub) (value)))";
    let err = check_files("pub_place", &[("main.aipl", "(module main (import lib) (fn main [] -> i32 0))"), ("lib.aipl", misplaced)]).unwrap_err();
    assert!(err.contains("(struct S [fields] (value) (pub))"), "{err}");
}

/// The AIPL resolver (resolver.aipl + generics.aipl) resolves value structs
/// as the Rust one does: the same program, printed (until the code
/// generators compile value structs, V2, this compares the printed module).
#[test]
fn the_aipl_resolver_agrees_on_value_structs() {
    let entry = root().join("tests/aipl/values.aipl");
    let rust = print_module(&Resolver::resolve(&entry).unwrap());
    let flat = aipl_core::selfhost::resolve_with_aipl(&entry).unwrap_or_else(|e| panic!("aipl resolve: {e}"));
    let m = Resolver::resolve_source(&flat, &entry).unwrap_or_else(|e| panic!("flat output: {e}\n{flat}"));
    TypeChecker::new().check_module(&m).unwrap_or_else(|e| panic!("flat output does not check: {e}"));
    assert_eq!(print_module(&m), rust);
    // the kinds of type arguments, and malformed forms, in both resolvers
    for (src, want) in [
        (
            "(module m (import alloc) (struct V [x:i32] (value)) (fn (mk T) [] -> (ptr T) (where (struct T)) (new T (call alloc.forever))) (fn main [] -> i32 (let p:(ptr V) (call (mk V))) 0))",
            "mk<V>: T must be a struct used through a pointer (where (struct T)), and V is a value struct, which has no address",
        ),
        (
            "(module m (struct (Pair T) [a:T b:T] (value)) (fn main [] -> i32 (get (make (Pair i32) 1 2) (Pair i32))))",
            "a generic struct field is written (get p (Name T...) field)",
        ),
    ] {
        let dir = scratch("aipl_kinds");
        let file = dir.join("main.aipl");
        std::fs::write(&file, src).unwrap();
        let rust = Resolver::resolve(&file).map(|_| ()).expect_err(src);
        assert!(rust.contains(want), "{src}\n  rust: {rust}");
        let aipl = aipl_core::selfhost::resolve_with_aipl(&file).map(|_| ()).expect_err(src);
        assert_eq!(aipl, rust, "{src}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// main's result under wasmtime, or the error.
fn wasm_call(wasm: &[u8], name: &str) -> Result<i32, String> {
    use wasmtime::{Engine, Linker, Module, Store};
    use wasmtime_wasi::p1::WasiP1Ctx;
    let engine = Engine::default();
    let module = Module::new(&engine, wasm).map_err(|e| format!("{e:?}"))?;
    let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).unwrap();
    let mut store = Store::new(&engine, wasmtime_wasi::WasiCtxBuilder::new().build_p1());
    let inst = linker.instantiate(&mut store, &module).map_err(|e| format!("{e:?}"))?;
    let main = inst.get_typed_func::<(), i32>(&mut store, name).unwrap();
    main.call(&mut store, ()).map_err(|e| format!("{e:?}"))
}

fn wasm_main(wasm: &[u8]) -> Result<i32, String> {
    wasm_call(wasm, "main")
}

#[test]
fn values_programs_run_under_wasmtime() {
    for (path, sum) in PROGRAMS {
        let m = Resolver::resolve(&root().join(path)).unwrap();
        TypeChecker::new().check_module(&m).unwrap();
        let wasm = aipl_core::compiler::wasm::WasmCompiler::compile(&m).unwrap();
        wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all()).validate_all(&wasm).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!(wasm_call(&wasm, "checksum"), Ok(sum as i32), "{path}");
    }
}

/// The self-hosted toolchain end to end (aipl_src/driver.aipl, as `aipl
/// compile --self`), which hands codegen.aipl each file as written, field
/// paths in reads included: the same bytes as the Rust toolchain.
#[test]
fn values_programs_compile_alike_through_the_self_hosted_toolchain() {
    for (path, _) in PROGRAMS {
        let entry = root().join(path);
        let m = Resolver::resolve(&entry).unwrap();
        TypeChecker::new().check_module(&m).unwrap();
        let rust = aipl_core::compiler::wasm::WasmCompiler::compile(&m).unwrap();
        assert_eq!(aipl_core::selfhost::compile_with_aipl(&entry).unwrap(), rust, "{path}");
    }
}

/// A failed contract shows a value struct as the make that builds it (in
/// the VM; compiled code shows no value structs, AIPL_SPEC.md 7.6).
#[test]
fn contract_messages_show_values_as_aipl() {
    let src = "(module m
      (struct V [a:i32 b:f64] (value))
      (struct W [v:V n:i64] (value))
      (fn f [w:W n:i32] -> i32 (req (gt n 0)) n)
      (fn g [n:i32] -> V (ens (gt (get res V.a) 0)) (make V n 0.5))
      (fn first [] -> i32 (call f (make W (make V 1 2.5) 7i64) -3))
      (fn second [] -> i32 (get (call g -2) V.a)))";
    let m = Parser::parse(src).unwrap();
    TypeChecker::new().check_module(&m).unwrap();
    let mut vm = VM::new();
    vm.load_module(m);
    let e = vm.invoke("first", vec![]).unwrap_err();
    assert!(e.ends_with("with w = (make W (make V 1 2.5) 7i64), n = -3"), "{e}");
    let e = vm.invoke("second", vec![]).unwrap_err();
    assert!(e.ends_with("with n = -2, res = (make V -2 0.5)"), "{e}");
}

/// Which unions are held by value (AIPL_SPEC.md 4.J), and a layout: the
/// tag, then the slots the variants share by kind (i32, f32, i64, f64).
#[test]
fn unions_by_value_are_decided_as_specified() {
    let m = Resolver::resolve(&root().join("tests/aipl/unions_by_value.aipl")).unwrap();
    let st = aipl_core::checker::Structs::new(&m.structs, &m.unions);
    // every union is a value: the program's own, and the Options it names
    let mut values: Vec<&str> = st.by_value.keys().map(String::as_str).filter(|n| !n.contains('<') || n.contains("Vec2") || n.contains("Outer")).collect();
    values.sort();
    assert_eq!(values, ["Chain", "HoldsWide", "Inner", "List", "Named", "Outer", "Shape", "Tagged", "Wide", "alloc.Allocator", "option.Option<Outer>", "option.Option<Vec2>"]);
    assert_eq!(st.unions.len(), st.by_value.len());
    // a str is one slot; Tagged holds a pointer where its Shape was
    assert_eq!(st.by_value["Tagged"].slots.len(), 3);
    assert_eq!(st.by_value["Named"].slots, [aipl_core::ast::Type::I32, aipl_core::ast::Type::I32]);
    assert_eq!(st.by_value["Outer"].slots.len(), 4);
    use aipl_core::ast::Type::*;
    let shape = &st.by_value["Shape"];
    assert_eq!(shape.slots, [I32, I32, F32, F32, I64, F64, F64, F64]);
    assert_eq!(shape.offsets, [0, 4, 8, 12, 16, 24, 32, 40]);
    assert_eq!((shape.size, shape.align), (48, 8));
    // dot; circle (at.x at.y r); rect (w h); big (n color)
    assert_eq!(shape.variants, [vec![], vec![5, 6, 7], vec![2, 3], vec![4, 1]]);
}

/// std/map's find returns (option.Option V), a union by value when V fits
/// (AIPL_SPEC.md 4.J): a million lookups move the heap cursor not at all,
/// in the VM and under wasmtime (a cell per Option would take 8 MB).
#[test]
fn a_million_map_finds_allocate_nothing() {
    let dir = scratch("finds");
    let file = dir.join("main.aipl");
    std::fs::write(
        &file,
        "(module main (import map) (import alloc) (import option)
          (fn main [] -> i32
            (let m:(ptr (map.Map i32 i64)) (call (map.make i32 i64) (call alloc.default) 16))
            (loop k 0 99 1 (call (map.set i32 i64) m k (i64.extend_s (* k 3))))
            (let before:i32 (unsafe (mem.load32 0)))
            (let sum:i64 0i64)
            (loop i 1 1000000 1
              (match (call (map.find i32 i64) m (% i 128))
                ((option.Option i64) some [v] (set! sum (+ sum v)))
                ((option.Option i64) none (set! sum (- sum 1i64)))))
            ;; -1 if the heap cursor moved
            (if (eq (unsafe (mem.load32 0)) before) (i32.wrap (% sum 1000000i64)) -1)))",
    )
    .unwrap();
    let m = Resolver::resolve(&file).unwrap();
    TypeChecker::new().check_module(&m).unwrap();
    let mut vm = VM::new();
    vm.load_module(m.clone());
    assert_eq!(vm.invoke("main", vec![]).unwrap(), Value::Int(795704));
    assert_eq!(wasm_main(&aipl_core::compiler::wasm::WasmCompiler::compile(&m).unwrap()), Ok(795704));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A module with a generic function named make (as std/vec and std/map
/// have) still builds unions with the make form: the template is named
/// only after call or ref. Both resolvers, and both compilers.
#[test]
fn a_template_named_make_leaves_the_make_form_alone() {
    let dir = scratch("make_named");
    std::fs::write(
        dir.join("lib.aipl"),
        "(module lib
          (union U [(one n:i32) (two)] (pub))
          (fn (make T) [x:T] -> U (pub) (make U.one 7))
          (fn both [] -> i32 (pub)
            (match (call (make i32) 3) (U.one [n] (match (make U.one n) (U.one [k] (+ k 1)) (U.two 0))) (U.two 0))))",
    )
    .unwrap();
    let entry = dir.join("main.aipl");
    std::fs::write(&entry, "(module main (import lib) (fn main [] -> i32 (call lib.both)))").unwrap();
    let m = Resolver::resolve(&entry).unwrap();
    TypeChecker::new().check_module(&m).unwrap();
    let flat = aipl_core::selfhost::resolve_with_aipl(&entry).unwrap_or_else(|e| panic!("aipl resolve: {e}"));
    let again = Resolver::resolve_source(&flat, &entry).unwrap();
    assert_eq!(print_module(&again), print_module(&m));
    let mut vm = VM::new();
    vm.load_module(m.clone());
    assert_eq!(vm.invoke("main", vec![]).unwrap(), Value::Int(8));
    let rust = aipl_core::compiler::wasm::WasmCompiler::compile(&m).unwrap();
    assert_eq!(wasm_main(&rust), Ok(8));
    assert_eq!(aipl_core::selfhost::compile_with_aipl(&entry).unwrap(), rust);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Which results are held by value (AIPL_SPEC.md 4.F), and a layout: the
/// tag, then the slots the two sides share by kind.
#[test]
fn results_by_value_are_decided_as_specified() {
    let m = Resolver::resolve(&root().join("tests/aipl/results_by_value.aipl")).unwrap();
    let st = aipl_core::checker::Structs::new(&m.structs, &m.unions);
    use aipl_core::ast::Type::*;
    let r = |a: aipl_core::ast::Type, b: aipl_core::ast::Type| ResultType(Box::new(a), Box::new(b));
    let vec2 = Struct("Vec2".into());
    let shape = Union("Shape".into());
    // the tag and one shared i32 slot
    assert_eq!(st.result_layout(&I32, &I32).unwrap().slots, [I32, I32]);
    // Vec2's two f64s and an i64: tag, i64, f64, f64
    let l = st.result_layout(&vec2, &I64).unwrap();
    assert_eq!(l.slots, [I32, I64, F64, F64]);
    assert_eq!(l.offsets, [0, 8, 16, 24]);
    assert_eq!(l.variants, [vec![2, 3], vec![1]]);
    // a union by value as a side; a result of a result
    assert!(st.result_layout(&shape, &Bool).is_some());
    assert!(st.result_layout(&r(I64, Bool), &I32).is_some());
    // Vec2's two f64s beside Shape's slots (tag, two i32s, f64): six
    assert_eq!(st.result_layout(&vec2, &shape).unwrap().slots.len(), 6);
    // a str side: the tag and one shared i32 slot
    assert_eq!(st.result_layout(&I32, &Str).unwrap().slots, [I32, I32]);
    // four f64s beside four i32s: nine, which the checker refuses
    let quad4 = aipl_core::ast::StructDef { name: "Quad4".into(), is_value: true, ..m.structs.iter().find(|s| s.name == "Quad").unwrap().clone() };
    let st = aipl_core::checker::Structs::new(&[m.structs.clone(), vec![quad4]].concat(), &m.unions);
    assert_eq!(st.result_slots(&Struct("Quad4".into()), &Struct("Ints".into())), 9);
    assert_eq!(st.result_layout(&Struct("Quad4".into()), &Struct("Ints".into())), None);
}

/// A result is held by value, so one that needs more than eight slots is
/// refused (MEMORY_PLAN.md M4), where its type is written and where one is
/// made, with the same error from both toolchains.
#[test]
fn a_result_over_eight_slots_is_refused_by_both_toolchains() {
    let defs = "  (struct Q [a:f64 b:f64 c:f64 d:f64] (value))\n  (struct I [a:i32 b:i32 c:i32 d:i32] (value))\n";
    for (name, body, want) in [
        ("written", "  (fn f [r:(result Q I)] -> i32 0)\n  (fn main [] -> i32 0))\n", "(result Q I) needs 9 slots"),
        ("made", "  (fn main [] -> i32 (match_result (err:Q (make I 1 2 3 4)) (ok q 1) (err e 0))))\n", "4:36: (result Q I) needs 9 slots"),
    ] {
        let dir = scratch(&format!("wide_result_{name}"));
        let file = dir.join("m.aipl");
        std::fs::write(&file, format!("(module m\n{defs}{body}")).unwrap();
        let module = Resolver::resolve(&file).unwrap();
        let rust = aipl_core::checker::TypeChecker::new().check_module(&module).map(|_| ()).expect_err(name);
        let aipl = aipl_core::selfhost::compile_with_aipl(&file).map(|_| ()).expect_err(name);
        assert!(rust.contains(want), "{name}: {rust}");
        assert!(rust.contains("more than the 8 a value holds: keep the larger side's data in a struct that is not (value) and make that side a (ptr S)"), "{rust}");
        assert!(aipl.contains(&rust), "{name}: {aipl} / {rust}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Every union is a value (MEMORY_PLAN.md M4), so one that contains
/// itself, directly or through another union, or needs more than eight
/// slots is refused, with the same error from both toolchains.
#[test]
fn a_union_that_cannot_be_a_value_is_refused_by_both_toolchains() {
    let shown = |n: &str| format!("union '{n}' contains itself (");
    for (name, body, want) in [
        ("direct", "  (union List [(nil) (cons head:i32 tail:List)])\n", format!("2:3: {}List.cons's field 'tail' has type List): a union is a value, so hold that field through a pointer, a (ptr S) to a struct that holds it", shown("List"))),
        ("through", "  (union A [(one b:B) (none)])\n  (union B [(two a:A) (none)])\n", format!("2:3: {}A.one's field 'b' has type B)", shown("A"))),
        ("wide", "  (union W [(small n:i32) (nine a:i64 b:i64 c:i64 d:i64 e:i64 f:i64 g:i64 h:i64)])\n", "2:3: union 'W' needs 10 slots, more than the 8 a value holds: move the fields of its widest variant, W.nine (8 slots), into a struct and hold a (ptr S)".to_string()),
    ] {
        let dir = scratch(&format!("refused_union_{name}"));
        let file = dir.join("m.aipl");
        std::fs::write(&file, format!("(module m\n{body}  (fn main [] -> i32 0))\n")).unwrap();
        let module = Resolver::resolve(&file).unwrap();
        let rust = aipl_core::checker::TypeChecker::new().check_module(&module).map(|_| ()).expect_err(name);
        let aipl = aipl_core::selfhost::compile_with_aipl(&file).map(|_| ()).expect_err(name);
        assert!(rust.contains(&want), "{name}: {rust}");
        assert!(aipl.contains(&rust), "{name}: {aipl} / {rust}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Malformed items the program fuzzer found (tools/fuzz.py): an empty item
/// (the AIPL resolver crashed) and a colon where a struct's field list goes
/// (it made the colon an empty list). Both toolchains give the same error.
#[test]
fn malformed_items_get_the_same_error_from_both_toolchains() {
    for (name, src) in [
        ("empty", "(module m\n  (fn main [] -> i32 0)\n  ()\n)\n"),
        ("colon", "(module m\n  (struct Out :[b:i32])\n  (fn main [] -> i32 0))\n"),
    ] {
        let dir = scratch(&format!("malformed_{name}"));
        let file = dir.join("m.aipl");
        std::fs::write(&file, src).unwrap();
        let rust = Resolver::resolve(&file).map(|_| ()).expect_err(src);
        let aipl = aipl_core::selfhost::compile_with_aipl(&file).map(|_| ()).expect_err(src);
        assert_eq!(aipl, rust, "{name}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
