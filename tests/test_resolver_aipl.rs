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
    assert!(!flat.contains("(import"), "{rel}: imports left in the output");
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
    assert!(err.starts_with("circular import: "), "{err}");
    let err = aipl_resolve(&dir.join("lost.aipl")).unwrap_err();
    assert_eq!(err, "cannot find module: nowhere");
    std::fs::remove_dir_all(&dir).unwrap();
}

// ---- The milestone: the self-hosted toolchain compiled to wasm ----

mod wasm_driver {
    use super::*;
    use wasmtime::{Engine, Instance, Linker, Module as WasmModule, Store, TypedFunc};
    use wasmtime_wasi::p1::WasiP1Ctx;
    use wasmtime_wasi::{FsPerms, WasiCtxBuilder};

    pub enum Outcome {
        Wasm(Vec<u8>),
        ResolveError(String),
        CompileError(i32),
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
        let mut put = |store: &mut Store<WasiP1Ctx>, s: &str| -> i32 {
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
    assert_eq!(ours, "two different modules are named x");
    let rust = Resolver::resolve(&dir.join("dots.aipl")).unwrap_err();
    assert!(rust.contains("'../x' is not an import path"), "{rust}");
    let ours = aipl_resolve(&dir.join("dots.aipl")).unwrap_err();
    assert!(ours.starts_with("not an import path"), "{ours}");
    std::fs::remove_dir_all(&dir).unwrap();
}
