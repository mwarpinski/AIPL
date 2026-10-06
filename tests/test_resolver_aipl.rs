//! P14: import resolution written in AIPL (aipl_src/resolver.aipl). It runs in
//! the VM and its flat output must compile to the same wasm bytes as the Rust
//! resolver's module (src/resolver.rs stays as the reference).

use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::resolver::Resolver;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn aipl_resolve(path: &Path) -> Result<String, String> {
    aipl_core::selfhost::resolve_with_aipl(path)
}

fn rust_bytes(path: &Path) -> Vec<u8> {
    let m = Resolver::resolve(path).unwrap_or_else(|e| panic!("rust resolve {}: {e}", path.display()));
    WasmCompiler::compile(&m).unwrap()
}

/// The AIPL resolver's flat output (imports gone, generics expanded) as a
/// Module. It goes through the Rust resolver rather than Parser::parse,
/// because constants and enum members are expanded there, after generics;
/// with no imports left, nothing else changes.
fn parse_flat(flat: &str, path: &Path) -> Result<aipl_core::ast::Module, String> {
    Resolver::resolve_source(flat, path)
}

fn assert_resolves_like_rust(rel: &str) {
    let path = root().join(rel);
    let flat = aipl_resolve(&path).unwrap_or_else(|e| panic!("aipl resolve {rel}: {e}"));
    assert!(!flat.lines().any(|l| l.trim_start().starts_with("(import")), "{rel}: imports left in the output");
    let m = parse_flat(&flat, &path).unwrap_or_else(|e| panic!("{rel}: flat output does not parse: {e}\n{flat}"));
    TypeChecker::new().check_module(&m).unwrap_or_else(|e| panic!("{rel}: flat output does not check: {e}"));
    let ours = WasmCompiler::compile(&m).unwrap();
    assert!(ours == rust_bytes(&path), "{rel}: AIPL resolver output compiles to different bytes than the Rust resolver");
}

#[test]
fn word_count_resolves_like_rust() {
    assert_resolves_like_rust("examples/word_count.aipl");
    assert_resolves_like_rust("examples/word_freq.aipl");
}

#[test]
fn std_modules_resolve_like_rust() {
    for m in ["io", "buf", "strmap", "str", "fmt", "vec", "map"] {
        assert_resolves_like_rust(&format!("aipl_src/std/{m}.aipl"));
    }
}

#[test]
fn compiler_sources_resolve_like_rust() {
    assert_resolves_like_rust("aipl_src/codegen.aipl");
    assert_resolves_like_rust("aipl_src/resolver.aipl");
    assert_resolves_like_rust("aipl_src/driver.aipl");
}

/// The AIPL suite uses threads, which the wasm backend rejects, so its flat
/// output is checked by parsing and type-checking instead of by bytes.
#[test]
fn test_suite_resolves_and_checks() {
    let flat = aipl_resolve(&root().join("aipl_src/test_suite.aipl")).unwrap();
    let m = parse_flat(&flat, &root().join("aipl_src/test_suite.aipl")).unwrap_or_else(|e| panic!("flat test_suite does not parse: {e}"));
    TypeChecker::new().check_module(&m).unwrap_or_else(|e| panic!("flat test_suite does not check: {e}"));
    let rust = Resolver::resolve(&root().join("aipl_src/test_suite.aipl")).unwrap();
    let names = |m: &aipl_core::ast::Module| {
        let mut v: Vec<String> = m.functions.iter().map(|f| f.name.clone()).collect();
        v.sort();
        v
    };
    assert_eq!(names(&m), names(&rust), "same functions under the same names");
}

#[test]
fn aliases_diamonds_and_struct_names() {
    let dir = std::env::temp_dir().join(format!("aipl_p14_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("base.aipl"), "(module base\n  (struct P [x:i32 y:i32])\n  (fn mk [x:i32] -> (ptr P) (let p:(ptr P) (new P)) (put p P.x x) p)\n  (fn px [p:(ptr P)] -> i32 (get p P.x)))").unwrap();
    std::fs::write(dir.join("left.aipl"), "(module left (import base as b)\n  (fn l [] -> i32 (call b.px (call b.mk 3))))").unwrap();
    std::fs::write(dir.join("right.aipl"), "(module right (import base)\n  (fn r [] -> i32 (let p:(ptr base.P) (call base.mk 4)) (+ (get p base.P.x) (sizeof base.P))))").unwrap();
    std::fs::write(dir.join("main.aipl"), "(module main (import left) (import right as rr)\n  (fn main [] -> i32 (+ (call left.l) (call rr.r))))").unwrap();
    assert_resolves_like_rust(dir.join("main.aipl").to_str().unwrap());
    let flat = aipl_resolve(&dir.join("main.aipl")).unwrap();
    assert_eq!(flat.matches("(fn base.mk ").count(), 1, "diamond import emitted twice:\n{flat}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// An alias may share a name with an op's prefix: `(import lib as mem)`
/// renames `mem.LIMIT` and `mem.Color.red` but leaves the op `(mem.alloc n)`
/// alone, since the head of a form is never a renamed name.
#[test]
fn aliases_do_not_rename_op_heads() {
    let dir = std::env::temp_dir().join(format!("aipl_alias_ops_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("lib.aipl"), "(module lib (const LIMIT:i32 16) (enum Color [red (green 5)]) (fn f [c:Color] -> i32 (enum.ord c)))").unwrap();
    std::fs::write(
        dir.join("main.aipl"),
        "(module main (import lib as mem)\n  (fn main [] -> i32 (let p:i32 (mem.alloc mem.LIMIT)) (mem.store32 p (call mem.f mem.Color.green)) (mem.load32 p)))",
    )
    .unwrap();
    let m = Resolver::resolve(&dir.join("main.aipl")).unwrap();
    TypeChecker::new().check_module(&m).unwrap();
    let mut vm = aipl_core::vm::VM::new();
    vm.load_module(m);
    assert_eq!(vm.invoke("main", vec![]).unwrap(), aipl_core::vm::Value::Int(5));
    // the AIPL resolver renames the same names (consts.aipl then erases them)
    let flat = aipl_resolve(&dir.join("main.aipl")).unwrap();
    assert!(flat.contains("(mem.alloc lib.LIMIT)") && flat.contains("(call lib.f lib.Color.green)"), "{flat}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn cycles_and_missing_modules_are_errors() {
    let dir = std::env::temp_dir().join(format!("aipl_p14_err_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.aipl"), "(module a (import b) (fn fa [] -> i32 1))").unwrap();
    std::fs::write(dir.join("b.aipl"), "(module b (import a) (fn fb [] -> i32 2))").unwrap();
    std::fs::write(dir.join("main.aipl"), "(module main (import a) (fn main [] -> i32 0))").unwrap();
    std::fs::write(dir.join("lost.aipl"), "(module lost (import nowhere) (fn main [] -> i32 0))").unwrap();
    let err = aipl_resolve(&dir.join("main.aipl")).unwrap_err();
    assert!(err.contains(".aipl: Circular import detected: '") && err.ends_with(".aipl' is imported while already being resolved"), "{err}");
    let err = aipl_resolve(&dir.join("lost.aipl")).unwrap_err();
    assert!(err.contains("lost.aipl: Cannot resolve import 'nowhere': no 'nowhere.aipl' found in '"), "{err}");
    // the same wording as the Rust resolver (the paths searched are listed in each one's own form)
    let rust = Resolver::resolve(&dir.join("lost.aipl")).unwrap_err();
    assert_eq!(rust.split(" found in ").next(), err.split(" found in ").next());
    std::fs::remove_dir_all(&dir).unwrap();
}

// ---- The milestone: the self-hosted toolchain compiled to wasm ----

mod wasm_driver {
    use super::*;
    use wasmtime::{Engine, Linker, Module as WasmModule, Store, TypedFunc};
    use wasmtime_wasi::p1::WasiP1Ctx;
    use wasmtime_wasi::{FsPerms, WasiCtxBuilder};

    pub enum Outcome {
        Wasm(Vec<u8>),
        ResolveError(String),
        CompileError(i32),
        /// status 3: "path: line:col: message"
        TypeError(String),
    }

    /// Runs driver.compile_file (resolver + codegen, compiled to wasm) under
    /// wasmtime with the repository root preopened as ".". `entry` and `dirs`
    /// are relative to the repository root.
    pub fn run(driver: &[u8], entry: &str, dirs: &str) -> Outcome {
        let engine = Engine::default();
        let module = WasmModule::new(&engine, driver).expect("driver module");
        let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
        wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).unwrap();
        let ctx = WasiCtxBuilder::new()
            .inherit_stderr()
            .preopened_dir(root(), ".", FsPerms::ReadWrite)
            .unwrap()
            .build_p1();
        let mut store = Store::new(&engine, ctx);
        let inst = linker.instantiate(&mut store, &module).expect("instantiate driver");
        let memory = inst.get_memory(&mut store, "memory").unwrap();
        let alloc: TypedFunc<i32, i32> = inst.get_typed_func(&mut store, "host_alloc").unwrap();
        let put = |store: &mut Store<WasiP1Ctx>, s: &str| -> i32 {
            let a = alloc.call(&mut *store, s.len() as i32 + 1).unwrap();
            memory.write(&mut *store, a as usize, s.as_bytes()).unwrap();
            a
        };
        let p = put(&mut store, entry);
        let d = put(&mut store, dirs);
        let compile: TypedFunc<(i32, i32, i32, i32), i32> = inst.get_typed_func(&mut store, "compile_file").unwrap();
        let out = compile.call(&mut store, (p, entry.len() as i32, d, dirs.len() as i32)).expect("compile_file trapped") as usize;
        let mut w = [0u8; 12];
        memory.read(&store, out, &mut w).unwrap();
        let word = |i: usize| i32::from_le_bytes(w[i * 4..i * 4 + 4].try_into().unwrap());
        let (status, addr, len) = (word(0), word(1), word(2));
        let bytes = || {
            let mut b = vec![0u8; len as usize];
            memory.read(&store, addr as usize, &mut b).unwrap();
            b
        };
        match status {
            0 => Outcome::Wasm(bytes()),
            1 => Outcome::ResolveError(String::from_utf8(bytes()).unwrap()),
            3 => Outcome::TypeError(String::from_utf8(bytes()).unwrap()),
            _ => Outcome::CompileError(len),
        }
    }
}

fn driver_wasm() -> Vec<u8> {
    rust_bytes(&root().join("aipl_src/driver.aipl"))
}

/// AIPL compiling AIPL with no Rust in the loop: the driver, compiled to wasm,
/// resolves word_count's imports (io, str, fmt) and compiles the program to
/// the same bytes as the Rust toolchain, under a plain WASI host.
#[test]
fn wasm_toolchain_compiles_a_multi_module_program() {
    let driver = driver_wasm();
    match wasm_driver::run(&driver, "examples/word_count.aipl", "aipl_src/std/") {
        wasm_driver::Outcome::Wasm(bytes) => {
            assert!(bytes == rust_bytes(&root().join("examples/word_count.aipl")), "word_count: wasm toolchain output differs from Rust")
        }
        wasm_driver::Outcome::ResolveError(e) => panic!("resolve error: {e}"),
        wasm_driver::Outcome::CompileError(c) => panic!("compile error {c}"),
        wasm_driver::Outcome::TypeError(e) => panic!("type error {e}"),
    }
    match wasm_driver::run(&driver, "examples/missing.aipl", "aipl_src/std/") {
        wasm_driver::Outcome::ResolveError(e) => assert_eq!(e, "cannot read entry file: examples/missing.aipl"),
        _ => panic!("a missing entry file must be a resolve error"),
    }
}

/// Constants and enums across modules (tests/aipl/consts_enums.aipl, which
/// imports tests/aipl/palette.aipl directly and through an alias, and uses
/// enums as generic type arguments): the AIPL resolver renames them like the
/// Rust one, and consts.aipl erases them to the Rust toolchain's bytes.
#[test]
fn wasm_toolchain_compiles_constants_and_enums_across_modules() {
    let driver = driver_wasm();
    let rel = "tests/aipl/consts_enums.aipl";
    match wasm_driver::run(&driver, rel, "aipl_src/std/") {
        wasm_driver::Outcome::Wasm(bytes) => assert!(bytes == rust_bytes(&root().join(rel)), "{rel}: wasm toolchain output differs from Rust"),
        wasm_driver::Outcome::ResolveError(e) => panic!("{rel}: resolve error: {e}"),
        wasm_driver::Outcome::CompileError(c) => panic!("{rel}: compile error {c}"),
        wasm_driver::Outcome::TypeError(e) => panic!("{rel}: type error {e}"),
    }
}

/// Unions and match across modules (tests/aipl/sum_types.aipl, which imports
/// tests/aipl/geometry.aipl directly and through an alias, matches its union
/// and enum, and uses its union as a generic type argument): both resolvers
/// rename the arms' heads alike, and the wasm toolchain compiles the result
/// to the Rust toolchain's bytes.
#[test]
fn wasm_toolchain_compiles_sum_types_across_modules() {
    let rel = "tests/aipl/sum_types.aipl";
    assert_resolves_like_rust(rel);
    let driver = driver_wasm();
    match wasm_driver::run(&driver, rel, "aipl_src/std/") {
        wasm_driver::Outcome::Wasm(bytes) => assert!(bytes == rust_bytes(&root().join(rel)), "{rel}: wasm toolchain output differs from Rust"),
        wasm_driver::Outcome::ResolveError(e) => panic!("{rel}: resolve error: {e}"),
        wasm_driver::Outcome::CompileError(c) => panic!("{rel}: compile error {c}"),
        wasm_driver::Outcome::TypeError(e) => panic!("{rel}: type error {e}"),
    }
}

/// The native backend (aipl_src/native/, reached through subdirectory
/// imports) compiles to the Rust toolchain's bytes with the wasm toolchain.
/// Run here, compiled, rather than in the VM like test_selfhost's parity
/// list, because the backend is large.
#[test]
fn wasm_toolchain_compiles_the_native_backend() {
    let driver = driver_wasm();
    for rel in ["aipl_src/native/x64.aipl", "aipl_src/native/elf.aipl", "aipl_src/native/native.aipl"] {
        match wasm_driver::run(&driver, rel, "aipl_src/std/") {
            wasm_driver::Outcome::Wasm(bytes) => assert!(bytes == rust_bytes(&root().join(rel)), "{rel}: wasm toolchain output differs from Rust"),
            wasm_driver::Outcome::ResolveError(e) => panic!("{rel}: resolve error: {e}"),
            wasm_driver::Outcome::CompileError(c) => panic!("{rel}: compile error {c}"),
            wasm_driver::Outcome::TypeError(e) => panic!("{rel}: type error {e}"),
        }
    }
}

/// The bootstrap fixpoint for the whole toolchain: the wasm driver compiles
/// its own sources (driver, resolver, codegen, compiler, std) to exactly the
/// bytes it was built from.
#[test]
fn wasm_toolchain_compiles_itself() {
    let driver = driver_wasm();
    match wasm_driver::run(&driver, "aipl_src/driver.aipl", "aipl_src/std/") {
        wasm_driver::Outcome::Wasm(bytes) => assert!(bytes == driver, "driver.aipl: self-compiled bytes differ"),
        wasm_driver::Outcome::ResolveError(e) => panic!("resolve error: {e}"),
        wasm_driver::Outcome::CompileError(c) => panic!("compile error {c}"),
        wasm_driver::Outcome::TypeError(e) => panic!("type error {e}"),
    }
}

/// Runs the compiled driver as a WASI command (`_start`) with `args` and `env`
/// and the repository root preopened as "."; returns the exit status.
fn run_driver_command(driver: &[u8], args: &[&str], env: &[(&str, &str)]) -> i32 {
    use wasmtime::{Engine, Linker, Module as WasmModule, Store};
    use wasmtime_wasi::p1::WasiP1Ctx;
    use wasmtime_wasi::{FsPerms, WasiCtxBuilder};
    let engine = Engine::default();
    let module = WasmModule::new(&engine, driver).unwrap();
    let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).unwrap();
    let mut b = WasiCtxBuilder::new();
    b.inherit_stdout().inherit_stderr().args(args);
    for (k, v) in env {
        b.env(k, v);
    }
    b.preopened_dir(root(), ".", FsPerms::ReadWrite).unwrap();
    let mut store = Store::new(&engine, b.build_p1());
    let inst = linker.instantiate(&mut store, &module).unwrap();
    let start = inst.get_typed_func::<(), ()>(&mut store, "_start").unwrap();
    match start.call(&mut store, ()) {
        Ok(()) => 0,
        Err(e) => e.downcast::<wasmtime_wasi::I32Exit>().expect("exit, not a trap").0,
    }
}

/// The driver as a command: `driver ENTRY OUT` writes the module, finds a
/// library through AIPL_PATH, and reports usage and resolve errors with
/// nonzero exit codes.
#[test]
fn wasm_driver_is_a_wasi_command() {
    let driver = driver_wasm();
    let work = root().join("target/p14_command");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(work.join("lib")).unwrap();
    std::fs::create_dir_all(work.join("app")).unwrap();
    std::fs::write(work.join("lib/extra.aipl"), "(module extra (fn seven [] -> i32 7))").unwrap();
    std::fs::write(work.join("app/main.aipl"), "(module app (import extra) (import str)\n  (fn main [] -> i32 (+ (call extra.seven) (get (call str.from_str \"abc\") str.Bytes.len))))").unwrap();

    // word_count, with the standard library at its default location
    let status = run_driver_command(&driver, &["aiplc", "examples/word_count.aipl", "target/p14_command/wc.wasm"], &[]);
    assert_eq!(status, 0);
    let written = std::fs::read(work.join("wc.wasm")).unwrap();
    assert!(written == rust_bytes(&root().join("examples/word_count.aipl")), "command output differs from Rust");

    // a library found only through AIPL_PATH; the result runs
    let env = [("AIPL_PATH", "target/p14_command/lib")];
    let status = run_driver_command(&driver, &["aiplc", "target/p14_command/app/main.aipl", "target/p14_command/app.wasm"], &env);
    assert_eq!(status, 0);
    let engine = wasmtime::Engine::default();
    let m = wasmtime::Module::new(&engine, std::fs::read(work.join("app.wasm")).unwrap()).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let inst = wasmtime::Instance::new(&mut store, &m, &[]).unwrap();
    let main = inst.get_typed_func::<(), i32>(&mut store, "main").unwrap();
    assert_eq!(main.call(&mut store, ()).unwrap(), 10);

    // without AIPL_PATH the import is not found; bad usage is status 2
    let status = run_driver_command(&driver, &["aiplc", "target/p14_command/app/main.aipl", "target/p14_command/app2.wasm"], &[]);
    assert_eq!(status, 1);
    assert!(!work.join("app2.wasm").exists());
    assert_eq!(run_driver_command(&driver, &["aiplc"], &[]), 2);
    std::fs::remove_dir_all(&work).unwrap();
}

/// Imports from subdirectories: `(import lib/util)` is the module `util`;
/// a module reached by two paths (from the entry and from a sibling) is one
/// module; aliases work; both resolvers agree byte for byte.
#[test]
fn subdirectory_imports_resolve_like_rust() {
    let dir = std::env::temp_dir().join(format!("aipl_p_subdir_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("lib/deep")).unwrap();
    std::fs::write(dir.join("lib/deep/thing.aipl"), "(module thing (fn seven [] -> i32 7))").unwrap();
    std::fs::write(dir.join("lib/util.aipl"), "(module util (import deep/thing) (fn eight [] -> i32 (+ 1 (call thing.seven))))").unwrap();
    std::fs::write(
        dir.join("main.aipl"),
        "(module main (import lib/util) (import lib/deep/thing as t)\n  (fn main [] -> i32 (+ (call util.eight) (call t.seven))))",
    )
    .unwrap();
    assert_resolves_like_rust(dir.join("main.aipl").to_str().unwrap());
    let flat = aipl_resolve(&dir.join("main.aipl")).unwrap();
    assert_eq!(flat.matches("(fn thing.seven ").count(), 1, "one module reached two ways:\n{flat}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn subdirectory_import_errors() {
    let dir = std::env::temp_dir().join(format!("aipl_p_subdir_err_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("a")).unwrap();
    std::fs::create_dir_all(dir.join("b")).unwrap();
    std::fs::write(dir.join("a/x.aipl"), "(module x (fn f [] -> i32 1))").unwrap();
    std::fs::write(dir.join("b/x.aipl"), "(module x (fn f [] -> i32 2))").unwrap();
    std::fs::write(dir.join("clash.aipl"), "(module clash (import a/x) (import b/x) (fn main [] -> i32 0))").unwrap();
    std::fs::write(dir.join("dots.aipl"), "(module dots (import ../x) (fn main [] -> i32 0))").unwrap();
    let rust = Resolver::resolve(&dir.join("clash.aipl")).unwrap_err();
    assert!(rust.contains("two different modules are named 'x'"), "{rust}");
    let ours = aipl_resolve(&dir.join("clash.aipl")).unwrap_err();
    // Rust's wording, after the importing file; the two paths in each one's own form
    let wording = |e: &str| e.split_once("clash.aipl: ").map(|(_, m)| m.split(": ").next().unwrap().to_string());
    assert_eq!(wording(&ours), wording(&rust), "{ours}");
    assert!(ours.contains("a/x.aipl and ") && ours.ends_with("b/x.aipl"), "{ours}");
    let rust = Resolver::resolve(&dir.join("dots.aipl")).unwrap_err();
    assert!(rust.contains("'../x' is not an import path"), "{rust}");
    let ours = aipl_resolve(&dir.join("dots.aipl")).unwrap_err();
    assert_eq!(ours, rust);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// CK11: the wasm toolchain type-checks before compiling, and reports the
/// first error in the file it is in, at Rust's line:col with Rust's
/// message: through the resolver's and the generics pass's origin maps.
#[test]
fn wasm_toolchain_reports_type_errors_in_the_users_files() {
    let dir = root().join("target").join(format!("aipl_typeerr_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // shapes checks on its own; its template `bad` fails only when instantiated
    std::fs::write(dir.join("shapes.aipl"), "(module shapes\n  (import vec)\n  (struct Box [w:i32])\n  (fn (first T) [v:(ptr (vec.Vec T))] -> T\n    (call (vec.at T) v 0))\n  (fn (bad T) [x:T] -> i32\n    (+ x 1)))").unwrap();
    std::fs::write(dir.join("broken.aipl"), "(module broken\n  (import shapes)\n  (fn area [b:(ptr shapes.Box)] -> i32\n    (* (get b shapes.Box.w)\n       true)))").unwrap();
    let cases = [
        // in the entry file
        ("entry", "(module entry\n  (import shapes)\n  (fn main [] -> i32\n    (let n:i32 1)\n    (set! n \"x\")\n    n))", "entry.aipl"),
        // at an atom: an undefined variable on its own line
        ("atom", "(module atom\n  (import shapes)\n  (fn main [] -> i32\n    (let n:i32 1)\n        zz))", "atom.aipl"),
        // in an imported module (shapes.area)
        ("uses_area", "(module uses_area\n  (import shapes)\n  (import broken)\n  (fn main [] -> i32 (call broken.area (new shapes.Box))))", "broken.aipl"),
        // inside a generic template, reported at the template
        ("uses_bad", "(module uses_bad\n  (import shapes)\n  (fn main [] -> i32 (call (shapes.bad bool) true)))", "shapes.aipl"),
    ];
    let driver = driver_wasm();
    for (name, src, file) in cases {
        let path = dir.join(format!("{name}.aipl"));
        std::fs::write(&path, src).unwrap();
        let rel = path.strip_prefix(root()).unwrap().to_str().unwrap().to_string();
        let rust = Resolver::resolve(&path).and_then(|m| TypeChecker::new().check_module(&m)).unwrap_err();
        // both toolchains name the same file, with the same message
        let (rust_path, rust) = rust.split_once(".aipl: ").unwrap_or_else(|| panic!("{name}: Rust names no file in {rust}"));
        assert!(format!("{rust_path}.aipl").ends_with(file), "{name}: Rust names {rust_path}, expected {file}");
        match wasm_driver::run(&driver, &rel, "aipl_src/std/") {
            wasm_driver::Outcome::TypeError(e) => {
                let (path_part, msg) = e.split_once(".aipl: ").unwrap_or_else(|| panic!("{name}: no file in {e}"));
                assert!(format!("{path_part}.aipl").ends_with(file), "{name}: error in {path_part}, expected {file}: {e}");
                assert_eq!(msg, rust, "{name}");
            }
            _ => panic!("{name}: expected a type error ({rust})"),
        }
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Errors in several functions, in different files, come back one per line,
/// each naming its own file, the same from both toolchains.
#[test]
fn wasm_toolchain_reports_each_failing_function_with_its_file() {
    let dir = root().join("target").join(format!("aipl_typeerrs_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("lib.aipl"), "(module lib\n  (fn f [] -> i32\n    (+ 1 true)))").unwrap();
    let path = dir.join("main.aipl");
    std::fs::write(&path, "(module main\n  (import lib)\n  (fn g [] -> i32 zz)\n  (fn main [] -> i32 (call lib.f)))").unwrap();
    let rel = path.strip_prefix(root()).unwrap().to_str().unwrap().to_string();
    let rust = Resolver::resolve(&path).and_then(|m| TypeChecker::new().check_module(&m)).unwrap_err();
    let wasm_driver::Outcome::TypeError(ours) = wasm_driver::run(&driver_wasm(), &rel, "aipl_src/std/") else {
        panic!("expected a type error ({rust})")
    };
    // (file, message) per line; the two toolchains spell paths differently
    let lines = |e: &str| -> Vec<(String, String)> {
        e.lines()
            .map(|l| {
                let (p, m) = l.split_once(".aipl: ").unwrap_or_else(|| panic!("no file in {l}"));
                (p.rsplit('/').next().unwrap().to_string(), m.to_string())
            })
            .collect()
    };
    let expected = vec![
        ("lib".to_string(), "3:5: Type mismatch in binary op: i32 vs bool".to_string()),
        ("main".to_string(), "3:19: Undefined variable 'zz'".to_string()),
    ];
    assert_eq!(lines(&rust), expected, "{rust}");
    assert_eq!(lines(&ours), expected, "{ours}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A file that is not one well-formed (module NAME ...) form is rejected by
/// the wasm toolchain with the Rust toolchain's message, word for word, and
/// never compiled (a fuzzing run found the AIPL resolver accepting such files
/// and emitting an empty module; docs/LAUNCH_CHECKLIST.md 1).
#[test]
fn malformed_files_are_rejected_like_rust() {
    let dir = root().join("target").join(format!("aipl_malformed_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("broken_dep.aipl"), "(module broken_dep\n  (fn f [] -> i32 1)").unwrap();
    let cases: &[(&str, &[u8])] = &[
        ("bad_utf8", b"(module m\n  (fn main [] -> i32 \xff 1))"),
        ("truncated_utf8", b"(module m (fn main [] -> i32 1)) ;; \xe2\x82"),
        ("never_closed", b"(module m\n  (fn main [] -> i32\n    (+ 1 2))"),
        ("mismatched", b"(module m\n  (fn main [] -> i32 1]\n)"),
        ("starts_with_closer", b")\n(module m)"),
        ("trailing_form", b"(module m (fn main [] -> i32 1))\n(fn g [] -> i32 2)"),
        ("trailing_atom", b"(module m (fn main [] -> i32 1)) x"),
        ("empty", b"  ;; nothing here\n"),
        ("not_a_module", b"(fn main [] -> i32 1)"),
        ("bare_atom", b"module"),
        ("brackets", b"[module m]"),
        ("no_name", b"(module (x) (fn main [] -> i32 1))"),
        ("import_no_name", b"(module m (import) (fn main [] -> i32 1))"),
        ("import_extra", b"(module m (import a b) (fn main [] -> i32 1))"),
        ("import_alias_missing", b"(module m (import a as) (fn main [] -> i32 1))"),
        ("import_not_symbol", b"(module m (import (a)) (fn main [] -> i32 1))"),
        ("unterminated_string", b"(module m (fn main [] -> i32 (str.len \"abc)))"),
        ("unknown_escape", b"(module m (fn main [] -> i32 (str.len \"a\\qb\")))"),
        ("stray_whitespace", b"(module m\xc2\xa0(fn main [] -> i32 1))"),
        ("unknown_item", b"(module m\n  (imp buf)\n  (fn main [] -> i32 1))"),
        ("stray_atom_item", b"(module m\n  (import str) 9\n  (fn main [] -> i32 1))"),
        ("broken_import", b"(module m (import broken_dep) (fn main [] -> i32 1))"),
        // the generics pass: Rust's messages, at the right place in the right file
        ("generic_arity", b"(module m (import vec) (import alloc)\n  (fn main [] -> i32\n    (let v:(ptr (vec.Vec i32 i32)) (call (vec.make i32) (call alloc.default) 1))\n    0))"),
        ("generic_builtin_name", b"(module m\n  (fn (get T) [x:T] -> T x)\n  (fn main [] -> i32 0))"),
        ("generic_lowercase_param", b"(module m\n  (fn (f t) [x:i32] -> i32 x)\n  (fn main [] -> i32 0))"),
        ("generic_no_params", b"(module m\n  (fn (f) [x:i32] -> i32 x)\n  (fn main [] -> i32 0))"),
        ("generic_twice", b"(module m\n  (fn (f T) [x:T] -> T x)\n  (fn (f T) [x:T] -> T x)\n  (fn main [] -> i32 0))"),
        ("generic_field_form", b"(module m\n  (struct (B T) [v:T])\n  (fn main [] -> i32 (let b:(ptr (B i32)) (new (B i32))) (get b (B i32) 5)))"),
        ("not_a_type", b"(module m (import vec) (import alloc)\n  (fn main [] -> i32\n    (let v:(ptr (vec.Vec (ptr))) (ptr.null (vec.Vec i32)))\n    0))"),
        // an error inside a type argument, reported where the argument is written
        ("bad_type_argument", b"(module m (import vec) (import alloc)\n  (fn main [] -> i32\n    (let v:(ptr (vec.Vec 0x)) (call (vec.make 0x) (call alloc.default) 1))\n    0))"),
        // errors at a closing bracket
        ("if_without_else", b"(module m\n  (fn main [] -> i32\n    (if true 1)))"),
        ("type_without_constructor", b"(module m\n  (fn main [x:()] -> i32 1))"),
    ];
    let driver = driver_wasm();
    for (name, src) in cases {
        let path = dir.join(format!("{name}.aipl"));
        std::fs::write(&path, src).unwrap();
        let rel = path.strip_prefix(root()).unwrap().to_str().unwrap().to_string();
        let rust = Resolver::resolve(&path).and_then(|m| TypeChecker::new().check_module(&m)).unwrap_err();
        let (rust_file, rust_msg) = rust.split_once(".aipl: ").unwrap_or_else(|| panic!("{name}: no file in {rust}"));
        let ours = match wasm_driver::run(&driver, &rel, "aipl_src/std/") {
            wasm_driver::Outcome::ResolveError(e) | wasm_driver::Outcome::TypeError(e) => e,
            wasm_driver::Outcome::Wasm(w) => panic!("{name}: compiled ({} bytes); Rust says {rust}", w.len()),
            wasm_driver::Outcome::CompileError(c) => panic!("{name}: compile error {c}; Rust says {rust}"),
        };
        let (our_file, our_msg) = ours.split_once(".aipl: ").unwrap_or_else(|| panic!("{name}: no file in {ours}"));
        assert_eq!(our_msg, rust_msg, "{name}");
        // the same file is blamed (an import's own error names the import)
        let stem = |f: &str| f.rsplit('/').next().unwrap().to_string();
        assert_eq!(stem(our_file), stem(rust_file), "{name}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}
