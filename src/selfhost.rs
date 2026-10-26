//! Host side of the self-hosted toolchain (P14): runs aipl_src/resolver.aipl
//! and aipl_src/driver.aipl, compiled to wasm by the Rust toolchain, under
//! wasmtime. `aipl compile --self` and the tests use it. (They ran in the VM
//! until 2026-10-20, many times slower; tests/test_selfhost.rs still runs
//! codegen.aipl compiling itself in the VM, which keeps the VM tested on the
//! whole compiler.)

use crate::checker::TypeChecker;
use crate::compiler::wasm::WasmCompiler;
use crate::resolver::Resolver;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use wasmtime::{Engine, Linker, Module as WasmModule, Store, TypedFunc};
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::{FsPerms, WasiCtxBuilder};

/// The standard library directory, as `Resolver` uses it. The AIPL resolver
/// reads AIPL_PATH itself (the VM sees the process environment).
pub fn std_dir() -> String {
    format!("{}/aipl_src/std/", env!("CARGO_MANIFEST_DIR"))
}

/// One of the toolchain's modules (`aipl_src/NAME.aipl`) compiled to wasm
/// by the Rust toolchain and then by wasmtime to machine code, once per
/// process, and kept between processes in the module cache (cache_dir):
/// wasmtime alone takes about 1.8 s of CPU for the driver, which every
/// optimized `aipl compile` runs.
pub(crate) fn toolchain(name: &'static str, cell: &'static OnceLock<Result<(Engine, WasmModule), String>>) -> Result<&'static (Engine, WasmModule), String> {
    cell.get_or_init(|| {
        let mut config = wasmtime::Config::new();
        // the compiler recurses once per nesting level of the program it
        // reads; on_big_stack gives the thread the room
        config.async_stack_size(80 << 20).max_wasm_stack(64 << 20);
        let engine = Engine::new(&config).map_err(|e| e.to_string())?;
        // native/native is cached as native_native
        let stem = name.replace('/', "_");
        let cached = cache_dir().map(|d| (d.join(format!("{stem}-{:016x}.cwasm", source_key(name))), d));
        if let Some((file, _)) = &cached {
            // SAFETY: deserializing trusts the file to be wasmtime's own
            // output for this engine; it is, written below by this function
            // into the user's own cache (the trust given the aipl binary
            // itself), and wasmtime refuses one made by another version or
            // configuration, which is then compiled again.
            if let Ok(m) = unsafe { WasmModule::deserialize_file(&engine, file) } {
                return Ok((engine, m));
            }
        }
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("aipl_src/{name}.aipl"));
        let module = Resolver::resolve(&path).map_err(|e| format!("resolve {name}.aipl: {e}"))?;
        TypeChecker::new().check_module(&module).map_err(|e| format!("check {name}.aipl: {e}"))?;
        let wasm = WasmCompiler::compile(&module).map_err(|e| format!("compile {name}.aipl: {e}"))?;
        let m = WasmModule::new(&engine, &wasm).map_err(|e| e.to_string())?;
        if let Some((file, dir)) = cached {
            // a cache that cannot be written only costs time
            let _ = keep(&m, &stem, &file, &dir);
        }
        Ok((engine, m))
    })
    .as_ref()
    .map_err(|e| e.clone())
}

/// The module cache's directory: $AIPL_CACHE (empty turns the cache off),
/// else $XDG_CACHE_HOME/aipl, else ~/.cache/aipl.
fn cache_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("AIPL_CACHE") {
        return if d.is_empty() { None } else { Some(d.into()) };
    }
    if let Some(d) = std::env::var_os("XDG_CACHE_HOME").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d).join("aipl"));
    }
    std::env::var_os("HOME").filter(|d| !d.is_empty()).map(|h| PathBuf::from(h).join(".cache/aipl"))
}

/// What a cached module was made from: the Rust sources (the code generator
/// that compiled it, build.rs) and every file below aipl_src/, read now
/// (the toolchain reads them at run time, so an edit there is seen).
fn source_key(name: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    fn files(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                files(&p, out);
            } else if p.extension().is_some_and(|x| x == "aipl") {
                out.push(p);
            }
        }
    }
    let mut all = Vec::new();
    files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("aipl_src"), &mut all);
    all.sort();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (env!("AIPL_SRC_HASH"), name).hash(&mut h);
    for f in &all {
        f.hash(&mut h);
        std::fs::read(f).unwrap_or_default().hash(&mut h);
    }
    h.finish()
}

/// Writes m to file (whole or not at all: a temporary file renamed), and
/// removes the cache's other modules of the same name, made from older
/// sources.
fn keep(m: &WasmModule, name: &str, file: &Path, dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, m.serialize().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, file).map_err(|e| e.to_string())?;
    for e in std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
        let p = e.path();
        let n = e.file_name().to_string_lossy().into_owned();
        if p != file && n.starts_with(&format!("{name}-")) && n.ends_with(".cwasm") {
            let _ = std::fs::remove_file(p);
        }
    }
    Ok(())
}

/// Calls `entry` (resolve_file or compile_file) of a toolchain module with
/// the path and the standard library's directory, in a fresh instance with
/// the working directory and `/` preopened (as aipl-run gives them): the
/// three words of the Out it returns, and the instance's memory.
fn call_toolchain(t: &(Engine, WasmModule), entry: &str, path: &str, extra: Option<i32>) -> Result<([i32; 3], Vec<u8>), String> {
    let (engine, module) = t;
    let mut linker: Linker<WasiP1Ctx> = Linker::new(engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |c: &mut WasiP1Ctx| c).map_err(|e| e.to_string())?;
    let mut b = WasiCtxBuilder::new();
    b.inherit_stderr().inherit_env();
    b.preopened_dir(".", ".", FsPerms::ReadWrite).map_err(|e| e.to_string())?;
    b.preopened_dir("/", "/", FsPerms::ReadWrite).map_err(|e| e.to_string())?;
    let mut store = Store::new(engine, b.build_p1());
    let inst = linker.instantiate(&mut store, module).map_err(|e| e.to_string())?;
    let memory = inst.get_memory(&mut store, "memory").ok_or("no memory")?;
    let alloc: TypedFunc<i32, i32> = inst.get_typed_func(&mut store, "host_alloc").map_err(|e| e.to_string())?;
    let mut put = |s: &str| -> Result<i32, String> {
        let a = alloc.call(&mut store, s.len() as i32 + 1).map_err(|e| e.to_string())?;
        memory.write(&mut store, a as usize, s.as_bytes()).map_err(|e| e.to_string())?;
        Ok(a)
    };
    let dirs = std_dir();
    let (p, d) = (put(path)?, put(&dirs)?);
    // a (ptr Out): the address in the low half (GENERATIONS_PLAN.md)
    let out = match extra {
        None => {
            let f: TypedFunc<(i32, i32, i32, i32), i64> = inst.get_typed_func(&mut store, entry).map_err(|e| e.to_string())?;
            f.call(&mut store, (p, path.len() as i32, d, dirs.len() as i32))
        }
        Some(x) => {
            let f: TypedFunc<(i32, i32, i32, i32, i32), i64> = inst.get_typed_func(&mut store, entry).map_err(|e| e.to_string())?;
            f.call(&mut store, (p, path.len() as i32, d, dirs.len() as i32, x))
        }
    }
    .map_err(|e| format!("{entry} trapped: {e}"))? as u32 as usize;
    let data = memory.data(&store).to_vec();
    let word = |at: usize| i32::from_le_bytes(data[at..at + 4].try_into().unwrap());
    Ok(([word(out), word(out + 4), word(out + 8)], data))
}

fn bytes(data: &[u8], addr: i32, len: i32) -> Vec<u8> {
    data[addr as usize..addr as usize + len as usize].to_vec()
}

/// Runs f on a thread with a stack the wasm stack above fits in.
pub(crate) fn on_big_stack<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    std::thread::Builder::new()
        .stack_size(128 << 20)
        .spawn(f)
        .map_err(|e| e.to_string())?
        .join()
        .map_err(|_| "the toolchain thread panicked".to_string())?
}

/// Resolves the program whose entry file is `path` with
/// `resolver.resolve_file`: the flat, import-free source, or the
/// resolver's error message.
pub fn resolve_with_aipl(path: &Path) -> Result<String, String> {
    static RESOLVER: OnceLock<Result<(Engine, WasmModule), String>> = OnceLock::new();
    let t = toolchain("resolver", &RESOLVER)?;
    let path = path.to_string_lossy().into_owned();
    // Out [ok addr len]
    let ([ok, addr, len], data) = on_big_stack(move || call_toolchain(t, "resolve_file", &path, None))?;
    let text = String::from_utf8(bytes(&data, addr, len)).map_err(|e| e.to_string())?;
    if ok == 1 { Ok(text) } else { Err(text) }
}

/// Compiles the program whose entry file is `path` with the self-hosted
/// toolchain (`driver.compile_file`: resolver, checker, code generator):
/// the module's bytes, or its error. What `aiplc` does natively.
pub fn compile_with_aipl(path: &Path) -> Result<Vec<u8>, String> {
    compile_with_aipl_limited(path, crate::vm::MAX_PAGES as u32)
}

/// compile_with_aipl with memory's maximum lowered to `max_pages`
/// (`driver.compile_file_limited`, as `--max-memory`).
pub fn compile_with_aipl_limited(path: &Path, max_pages: u32) -> Result<Vec<u8>, String> {
    driver_compile("compile_file_limited", path, max_pages)
}

/// The program whose entry file is `path` compiled through the optimizer
/// (`driver.compile_file_opt`, or `compile_tests_opt` for its test build):
/// what `aipl compile` writes unless `--no-opt` (docs/design/OPT_PLAN.md).
pub fn compile_optimized(path: &Path, max_pages: u32, tests: bool) -> Result<Vec<u8>, String> {
    driver_compile(if tests { "compile_tests_opt" } else { "compile_file_opt" }, path, max_pages)
}

/// Calls one of driver.aipl's compile entries: the module's bytes, or the error.
fn driver_compile(entry: &'static str, path: &Path, max_pages: u32) -> Result<Vec<u8>, String> {
    static DRIVER: OnceLock<Result<(Engine, WasmModule), String>> = OnceLock::new();
    let t = toolchain("driver", &DRIVER)?;
    let path = path.to_string_lossy().into_owned();
    // Out [status addr len]: 0 the module, 1 and 3 an error's text, 2 a compile error's code
    let ([status, addr, len], data) = on_big_stack(move || call_toolchain(t, entry, &path, Some(max_pages as i32)))?;
    match status {
        0 => Ok(bytes(&data, addr, len)),
        2 => Err(format!("compile error {len} (see AIPL_SPEC.md 6.4)")),
        _ => Ok(String::from_utf8_lossy(&bytes(&data, addr, len)).into_owned()).and_then(Err),
    }
}
