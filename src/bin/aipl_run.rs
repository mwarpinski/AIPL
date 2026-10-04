//! aipl-run: the native launcher for compiled AIPL (AIPL_SPEC.md 6.5).
//!
//! This is the one piece of the toolchain that has to be native code: wasm
//! cannot start itself, so something native loads wasmtime, hands it the
//! module, and connects it to the terminal. It contains no compiler logic.
//!
//!   aipl-run [--sandbox] prog.wasm [args...]   run a compiled module
//!   ./prog [args...]                            run the module appended to
//!                                               this file by `aipl compile --exe`
//!
//! The program gets the real stdin/stdout/stderr, the command line (argv[0]
//! is the program), the environment, the working directory as preopened fd 3
//! and `/` as fd 4 (so absolute paths work; `--sandbox` grants only the
//! working directory), the wasi-threads `thread-spawn` import for threaded
//! modules, and its exit status (`sys.exit`, or 0 when `_start` returns).

use std::path::Path;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, OnceLock};
use wasmtime::{Config, Engine, Linker, MemoryType, Module, SharedMemory, Store};
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::{FsPerms, I32Exit, WasiCtxBuilder};

/// An executable made by `aipl compile --exe` is this runner followed by the
/// wasm module and a 16-byte trailer: flags (u32 LE; bit 0 = sandbox),
/// the module's length (u32 LE), and this magic.
const MAGIC: &[u8; 8] = b"AIPLEXE1";
const FLAG_SANDBOX: u32 = 1;

struct Program {
    wasm: Vec<u8>,
    argv: Vec<String>,
    sandbox: bool,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let program = match embedded(&args) {
        Some(p) => p,
        None => match from_command_line(&args) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("aipl-run: {e}");
                std::process::exit(2);
            }
        },
    };
    std::process::exit(run(program));
}

/// The module appended to this executable, if there is one.
fn embedded(args: &[String]) -> Option<Program> {
    let me = std::fs::read(std::env::current_exe().ok()?).ok()?;
    let n = me.len();
    if n < 16 || &me[n - 8..] != MAGIC {
        return None;
    }
    let flags = u32::from_le_bytes(me[n - 16..n - 12].try_into().ok()?);
    let len = u32::from_le_bytes(me[n - 12..n - 8].try_into().ok()?) as usize;
    let start = n.checked_sub(16 + len)?;
    Some(Program { wasm: me[start..n - 16].to_vec(), argv: args.to_vec(), sandbox: flags & FLAG_SANDBOX != 0 })
}

fn from_command_line(args: &[String]) -> Result<Program, String> {
    let mut rest = &args[1.min(args.len())..];
    let sandbox = rest.first().is_some_and(|a| a == "--sandbox");
    if sandbox {
        rest = &rest[1..];
    }
    let file = rest.first().ok_or("usage: aipl-run [--sandbox] PROGRAM.wasm [ARGS...]")?;
    let wasm = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
    Ok(Program { wasm, argv: rest.to_vec(), sandbox })
}

/// What one thread's instance needs: shared by the main thread and every
/// thread started through `thread-spawn`.
struct Host {
    engine: Engine,
    module: Module,
    linker: Linker<WasiP1Ctx>,
    argv: Vec<String>,
    sandbox: bool,
}

fn wasi(argv: &[String], sandbox: bool) -> Result<WasiP1Ctx, String> {
    let mut b = WasiCtxBuilder::new();
    b.inherit_stdio().inherit_env().args(argv);
    b.preopened_dir(".", ".", FsPerms::ReadWrite).map_err(|e| format!("cannot open the working directory: {e}"))?;
    if !sandbox {
        b.preopened_dir("/", "/", FsPerms::ReadWrite).map_err(|e| format!("cannot open /: {e}"))?;
    }
    Ok(b.build_p1())
}

/// Turns a finished call into an exit status, reporting traps.
fn status(r: wasmtime::Result<()>, argv0: &str) -> i32 {
    match r {
        Ok(()) => 0,
        Err(e) => match e.downcast_ref::<I32Exit>() {
            Some(exit) => exit.0,
            None => {
                eprintln!("{argv0}: {e:?}");
                134
            }
        },
    }
}

fn run(p: Program) -> i32 {
    let argv0 = p.argv.first().cloned().unwrap_or_else(|| "aipl-run".into());
    let fail = |e: String| -> i32 {
        eprintln!("{argv0}: {e}");
        1
    };
    let mut config = Config::new();
    config.wasm_threads(true).shared_memory(true);
    let engine = match Engine::new(&config) {
        Ok(e) => e,
        Err(e) => return fail(e.to_string()),
    };
    let module = match Module::new(&engine, &p.wasm) {
        Ok(m) => m,
        Err(e) => return fail(format!("not a valid module: {e}")),
    };
    let threaded = module.imports().any(|i| i.module() == "wasi" && i.name() == "thread-spawn");
    let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
    if let Err(e) = wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t) {
        return fail(e.to_string());
    }
    let ctx = match wasi(&p.argv, p.sandbox) {
        Ok(c) => c,
        Err(e) => return fail(e),
    };
    let mut store = Store::new(&engine, ctx);
    let host: Arc<OnceLock<Host>> = Arc::new(OnceLock::new());
    if threaded {
        // every thread instantiates the module again on one shared memory
        let memory = match SharedMemory::new(&engine, MemoryType::shared(16, 1024)) {
            Ok(m) => m,
            Err(e) => return fail(e.to_string()),
        };
        if let Err(e) = linker.define(&mut store, "env", "memory", memory) {
            return fail(e.to_string());
        }
        let next_tid = Arc::new(AtomicI32::new(1));
        let h = host.clone();
        let spawn = linker.func_wrap("wasi", "thread-spawn", move |_: wasmtime::Caller<'_, WasiP1Ctx>, start_arg: i32| -> i32 {
            let tid = next_tid.fetch_add(1, Ordering::SeqCst);
            let h = h.clone();
            std::thread::spawn(move || {
                let host = h.get().expect("host set before the program starts");
                let code = (|| -> Result<i32, String> {
                    let mut store = Store::new(&host.engine, wasi(&host.argv, host.sandbox)?);
                    let inst = host.linker.instantiate(&mut store, &host.module).map_err(|e| e.to_string())?;
                    let start = inst.get_typed_func::<(i32, i32), ()>(&mut store, "wasi_thread_start").map_err(|e| e.to_string())?;
                    Ok(status(start.call(&mut store, (tid, start_arg)), &host.argv[0]))
                })()
                .unwrap_or_else(|e| {
                    eprintln!("{}: thread {tid}: {e}", host.argv[0]);
                    1
                });
                // a thread that exits or traps ends the whole program, as in wasi-threads
                if code != 0 {
                    std::process::exit(code);
                }
            });
            tid
        });
        if let Err(e) = spawn {
            return fail(e.to_string());
        }
    }
    let instance = match linker.instantiate(&mut store, &module) {
        Ok(i) => i,
        Err(e) => return fail(format!("cannot start: {e}")),
    };
    let _ = host.set(Host { engine: engine.clone(), module: module.clone(), linker: linker.clone(), argv: p.argv.clone(), sandbox: p.sandbox });
    match instance.get_typed_func::<(), ()>(&mut store, "_start") {
        Ok(start) => status(start.call(&mut store, ()), &argv0),
        Err(_) => fail(format!(
            "{} has no _start (compile a module with a zero-argument main)",
            Path::new(&argv0).display()
        )),
    }
}
