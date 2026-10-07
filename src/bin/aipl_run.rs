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
//!
//! A trap (or another runtime error) prints one line, `<argv[0]>: <reason>`
//! (e.g. `prog: wasm trap: integer divide by zero`), and exits with 134, the
//! same line a native executable prints. `AIPL_BACKTRACE=1` prints
//! wasmtime's full report instead.

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
    /// a threaded program's memory, for reading a failed check's message
    shared: Option<SharedMemory>,
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

/// n bytes at `at` of a shared memory, or None past its end.
fn read_shared(m: &SharedMemory, at: usize, n: usize) -> Option<Vec<u8>> {
    let cells = m.data().get(at..at.checked_add(n)?)?;
    // the program has stopped (it trapped); nothing writes these bytes now
    Some(cells.iter().map(|c| unsafe { *c.get() }).collect())
}

/// The message a failed check left before trapping (AIPL_SPEC.md 7.9): the
/// address and length of its text in cells 92 and 96, read with `read`
/// (offset, length -> bytes, or None past the end of memory).
fn failure_message(read: &dyn Fn(usize, usize) -> Option<Vec<u8>>) -> Option<String> {
    let cells = read(92, 8)?;
    let addr = u32::from_le_bytes(cells[0..4].try_into().unwrap()) as usize;
    let len = u32::from_le_bytes(cells[4..8].try_into().unwrap()) as usize;
    if len == 0 || len > 65536 {
        return None;
    }
    Some(String::from_utf8_lossy(&read(addr, len)?).into_owned())
}

/// Turns a finished call into an exit status, reporting traps: a failed
/// contract or bounds check by its message, any other trap by its reason.
fn status(r: wasmtime::Result<()>, argv0: &str, read: &dyn Fn(usize, usize) -> Option<Vec<u8>>) -> i32 {
    match r {
        Ok(()) => 0,
        Err(e) => match e.downcast_ref::<I32Exit>() {
            Some(exit) => exit.0,
            None => {
                let failed_check = match e.downcast_ref::<wasmtime::Trap>() {
                    Some(wasmtime::Trap::UnreachableCodeReached) => failure_message(read),
                    _ => None,
                };
                if std::env::var_os("AIPL_BACKTRACE").is_some_and(|v| v != "0") {
                    eprintln!("{argv0}: {e:?}");
                    return 134;
                }
                match failed_check {
                    Some(m) => eprintln!("{argv0}: {m}"),
                    // the innermost cause: a Trap displays as "wasm trap: <reason>"
                    None => eprintln!("{argv0}: {}", e.root_cause()),
                }
                if e.downcast_ref::<wasmtime::Trap>().is_some() {
                    if let Some(bt) = e.downcast_ref::<wasmtime::WasmBacktrace>() {
                        let lines = LINES.get();
                        let frames = bt.frames().iter().filter_map(|f| {
                            let at = lines.and_then(|t| t.find(f.func_index(), f.module_offset()?));
                            Some((f.func_name()?, at))
                        });
                        eprint!("{}", call_chain(frames));
                    }
                }
                134
            }
        },
    }
}

/// The functions a trap happened in, innermost first, one "  at NAME" line
/// each: the program's own functions (unnamed ones, the generated `_start`
/// and `wasi_thread_start`, and the "aipl." check helpers are left out), at
/// most 32, then "  ... N more". Native executables print the same lines
/// (aipl_src/native/runtime.aipl emit_backtrace_routine).
fn call_chain<'a>(frames: impl Iterator<Item = (&'a str, Option<String>)>) -> String {
    const SHOWN: usize = 32;
    let shown: Vec<_> = frames.filter(|(n, _)| !(n.starts_with("aipl.") || *n == "_start" || *n == "wasi_thread_start")).collect();
    let mut out = String::new();
    for (n, at) in shown.iter().take(SHOWN) {
        match at {
            Some(at) => out.push_str(&format!("  at {n} ({at})\n")),
            None => out.push_str(&format!("  at {n}\n")),
        }
    }
    if shown.len() > SHOWN {
        out.push_str(&format!("  ... {} more\n", shown.len() - SHOWN));
    }
    out
}

/// The module's `aipl.lines` section (docs/design/LINES_PLAN.md): for each
/// function, where its calls and trapping instructions are in the source.
static LINES: OnceLock<LineTable> = OnceLock::new();

#[derive(Default)]
struct LineTable {
    files: Vec<String>,
    /// function index -> (module offset of its body, entries (offset in
    /// the body, file, line, column) by offset)
    funcs: std::collections::HashMap<u32, (usize, Vec<LineEntry>)>,
}

/// (offset in the function's body, file, line, column)
type LineEntry = (u32, u32, u32, u32);

impl LineTable {
    /// "file:line:col" of the instruction at `module_offset` in function `func`.
    fn find(&self, func: u32, module_offset: usize) -> Option<String> {
        let (start, entries) = self.funcs.get(&func)?;
        let at = module_offset.checked_sub(*start)? as u32;
        let k = entries.partition_point(|e| e.0 <= at).checked_sub(1)?;
        let (_, file, line, col) = entries[k];
        Some(format!("{}:{line}:{col}", self.files.get(file as usize)?))
    }

    /// Reads the table from a module with `imported` function imports; an
    /// empty table if the module has none (or it is malformed).
    fn read(wasm: &[u8], imported: u32) -> LineTable {
        Self::try_read(wasm, imported).unwrap_or_default()
    }

    fn try_read(wasm: &[u8], imported: u32) -> Option<LineTable> {
        fn u(b: &[u8], at: &mut usize) -> Option<u64> {
            let (mut v, mut shift) = (0u64, 0);
            loop {
                let byte = *b.get(*at)?;
                *at += 1;
                v |= ((byte & 0x7f) as u64) << shift;
                if byte & 0x80 == 0 {
                    return Some(v);
                }
                shift += 7;
                if shift > 63 {
                    return None;
                }
            }
        }
        fn s(b: &[u8], at: &mut usize) -> Option<i64> {
            let (mut v, mut shift) = (0i64, 0);
            loop {
                let byte = *b.get(*at)?;
                *at += 1;
                v |= ((byte & 0x7f) as i64) << shift;
                shift += 7;
                if byte & 0x80 == 0 {
                    if shift < 64 && byte & 0x40 != 0 {
                        v |= -1i64 << shift;
                    }
                    return Some(v);
                }
                if shift > 63 {
                    return None;
                }
            }
        }
        let mut bodies: Vec<usize> = Vec::new();
        let mut table: Option<&[u8]> = None;
        let mut at = 8;
        while at < wasm.len() {
            let id = wasm[at];
            at += 1;
            let size = u(wasm, &mut at)? as usize;
            let end = at.checked_add(size)?;
            let sec = wasm.get(at..end)?;
            if id == 10 {
                let mut p = 0;
                for _ in 0..u(sec, &mut p)? {
                    let n = u(sec, &mut p)? as usize;
                    bodies.push(at + p);
                    p += n;
                }
            } else if id == 0 {
                let mut p = 0;
                let n = u(sec, &mut p)? as usize;
                if sec.get(p..p + n)? == b"aipl.lines" {
                    table = Some(&sec[p + n..]);
                }
            }
            at = end;
        }
        let t = table?;
        let mut p = 0;
        let mut out = LineTable::default();
        for _ in 0..u(t, &mut p)? {
            let n = u(t, &mut p)? as usize;
            out.files.push(String::from_utf8_lossy(t.get(p..p + n)?).into_owned());
            p += n;
        }
        for _ in 0..u(t, &mut p)? {
            let func = u(t, &mut p)? as u32;
            let start = *bodies.get(func.checked_sub(imported)? as usize)?;
            let (mut off, mut line) = (0u32, 0i64);
            let mut entries = Vec::new();
            for _ in 0..u(t, &mut p)? {
                off += u(t, &mut p)? as u32;
                let file = u(t, &mut p)? as u32;
                line += s(t, &mut p)?;
                let col = u(t, &mut p)? as u32;
                entries.push((off, file, line as u32, col));
            }
            out.funcs.insert(func, (start, entries));
        }
        Some(out)
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
    // every frame, so a trap's call chain counts all of them, as native
    // executables do (wasmtime keeps 20 by default)
    config.wasm_backtrace_max_frames(std::num::NonZeroUsize::new(1 << 24));
    let engine = match Engine::new(&config) {
        Ok(e) => e,
        Err(e) => return fail(e.to_string()),
    };
    let module = match Module::new(&engine, &p.wasm) {
        Ok(m) => m,
        Err(e) => return fail(format!("not a valid module: {e}")),
    };
    let threaded = module.imports().any(|i| i.module() == "wasi" && i.name() == "thread-spawn");
    let imported = module.imports().filter(|i| matches!(i.ty(), wasmtime::ExternType::Func(_))).count() as u32;
    let _ = LINES.set(LineTable::read(&p.wasm, imported));
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
    let mut shared: Option<SharedMemory> = None;
    if threaded {
        // every thread instantiates the module again on one shared memory
        let memory = match SharedMemory::new(&engine, MemoryType::shared(16, 32768) /* vm.rs MAX_PAGES: 2 GiB */) {
            Ok(m) => m,
            Err(e) => return fail(e.to_string()),
        };
        shared = Some(memory.clone());
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
                    let r = start.call(&mut store, (tid, start_arg));
                    let mem = host.shared.clone();
                    Ok(status(r, &host.argv[0], &move |at, n| read_shared(mem.as_ref()?, at, n)))
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
    let _ = host.set(Host { engine: engine.clone(), module: module.clone(), linker: linker.clone(), argv: p.argv.clone(), sandbox: p.sandbox, shared: shared.clone() });
    match instance.get_typed_func::<(), ()>(&mut store, "_start") {
        Ok(start) => {
            let r = start.call(&mut store, ());
            match (instance.get_memory(&mut store, "memory"), shared.clone()) {
                (Some(m), _) => {
                    let data = m.data(&store).to_vec();
                    status(r, &argv0, &move |at, n| data.get(at..at.checked_add(n)?).map(|b| b.to_vec()))
                }
                (None, mem) => status(r, &argv0, &move |at, n| read_shared(mem.as_ref()?, at, n)),
            }
        }
        Err(_) => fail(format!(
            "{} has no _start (compile a module with a zero-argument main)",
            Path::new(&argv0).display()
        )),
    }
}
