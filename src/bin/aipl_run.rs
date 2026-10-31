//! aipl-run: the native launcher for compiled AIPL (AIPL_SPEC.md 6.5).
//!
//! This is the one piece of the toolchain that has to be native code: wasm
//! cannot start itself, so something native loads wasmtime, hands it the
//! module, and connects it to the terminal. It contains no compiler logic.
//!
//!   aipl-run [--sandbox] [--listen ADDR:PORT]... prog.wasm [args...]
//!                                               run a compiled module
//!   aipl-run --serve-fresh ADDR:PORT [--requests N] prog.wasm [args...]
//!                                               serve it, a fresh instance
//!                                               per connection (serve_fresh)
//!   ./prog [args...]                            run the module appended to
//!                                               this file by `aipl compile --exe`
//!
//! The program gets the real stdin/stdout/stderr, the command line (argv[0]
//! is the program), the environment, the working directory as preopened fd 3
//! and `/` as fd 4 (so absolute paths work; `--sandbox` grants only the
//! working directory), the wasi-threads `thread-spawn` import for threaded
//! modules, AIPL's sockets (`aipl_net_v1`, src/net.rs: a listen only on an
//! address `--listen` grants), and its exit status (`sys.exit`, or 0 when
//! `_start` returns).
//!
//! A trap (or another runtime error) prints one line, `<argv[0]>: <reason>`
//! (e.g. `prog: wasm trap: integer divide by zero`), and exits with 134, the
//! same line a native executable prints. `AIPL_BACKTRACE=1` prints
//! wasmtime's full report instead.
//!
//! `AIPL_PROFILE=perfmap` has wasmtime write /tmp/perf-PID.map, which
//! `perf record` / `perf report` read to name each AIPL function
//! (docs/BENCHMARKS.md, "Profiling").

use std::path::Path;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, OnceLock};
use wasmtime::{Config, Engine, Linker, MemoryType, Module, SharedMemory, Store};
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::{FsPerms, I32Exit, WasiCtxBuilder};

/// An executable made by `aipl compile --exe --target wasm` is this runner
/// followed by the wasm module, its grants (one a line: --listen's ADDR:PORT,
/// or --allow-run's `run:PROGRAM`),
/// and a 20-byte trailer: flags (u32 LE; bit 0 = sandbox, bit 1 = serve a
/// fresh instance per connection on the address that is then the grants'
/// first line: --serve), the grants' length and the module's (u32 LE
/// each), and this magic.
const MAGIC: &[u8; 8] = b"AIPLEXE2";
const FLAG_SANDBOX: u32 = 1;
const FLAG_SERVE: u32 = 2;

struct Program {
    wasm: Vec<u8>,
    argv: Vec<String>,
    sandbox: bool,
    listen: Vec<std::net::SocketAddr>,
    /// --allow-connect: the addresses (and ports) net.connect may reach
    connect: Vec<aipl_core::net::ConnectGrant>,
    /// --allow-run: the programs the module may start ("*": any)
    allow_run: Vec<String>,
    /// --serve-fresh: the address to serve on, and how many connections to
    /// answer before stopping (none: no end)
    fresh: Option<(std::net::SocketAddr, Option<usize>)>,
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
    std::process::exit(if program.fresh.is_some() { serve_fresh(program) } else { run(program) });
}

/// The module appended to this executable, if there is one.
fn embedded(args: &[String]) -> Option<Program> {
    let me = std::fs::read(std::env::current_exe().ok()?).ok()?;
    let n = me.len();
    if n < 20 || &me[n - 8..] != MAGIC {
        return None;
    }
    let word = |at: usize| me[at..at + 4].try_into().ok().map(u32::from_le_bytes);
    let (flags, glen, len) = (word(n - 20)?, word(n - 16)? as usize, word(n - 12)? as usize);
    let grants_at = n.checked_sub(20 + glen)?;
    let start = grants_at.checked_sub(len)?;
    // write_executable checked each grant
    let text = std::str::from_utf8(&me[grants_at..n - 20]).ok()?;
    let allow_run = text.lines().filter_map(|g| g.strip_prefix("run:").map(str::to_string)).collect();
    let connect = text.lines().filter_map(|g| g.strip_prefix("connect:").and_then(|g| aipl_core::net::Net::parse_connect_grant(g).ok())).collect();
    let mut lines = text.lines().filter_map(|g| aipl_core::net::Net::parse_grant(g).ok());
    let fresh = if flags & FLAG_SERVE != 0 { Some((lines.next()?, None)) } else { None };
    let listen = lines.collect();
    Some(Program { wasm: me[start..grants_at].to_vec(), argv: args.to_vec(), sandbox: flags & FLAG_SANDBOX != 0, listen, connect, allow_run, fresh })
}

fn from_command_line(args: &[String]) -> Result<Program, String> {
    let mut rest = &args[1.min(args.len())..];
    let (mut sandbox, mut listen, mut fresh, mut requests, mut name) = (false, Vec::new(), None, None, None);
    let mut allow_run: Vec<String> = Vec::new();
    let mut connect: Vec<aipl_core::net::ConnectGrant> = Vec::new();
    let mut temporary = false;
    loop {
        match rest.first().map(String::as_str) {
            Some("--serve-fresh") => {
                let g = rest.get(1).ok_or("--serve-fresh needs ADDR:PORT")?;
                fresh = Some(aipl_core::net::Net::parse_address("--serve-fresh", g)?);
                rest = &rest[2..];
            }
            // the program's argv[0], in place of the module's path (aipl
            // serve names it by its source, as aipl eval does)
            Some("--name") => {
                name = Some(rest.get(1).ok_or("--name needs the program's name")?.clone());
                rest = &rest[2..];
            }
            Some("--requests") => {
                let n = rest.get(1).and_then(|n| n.parse::<usize>().ok()).ok_or("--requests needs a count")?;
                requests = Some(n);
                rest = &rest[2..];
            }
            Some("--sandbox") => {
                sandbox = true;
                rest = &rest[1..];
            }
            // the module is a temporary copy (aipl serve's): removed once
            // read, with its directory if that is then empty
            Some("--temporary") => {
                temporary = true;
                rest = &rest[1..];
            }
            Some("--listen") => {
                let g = rest.get(1).ok_or("--listen needs ADDR:PORT")?;
                listen.push(aipl_core::net::Net::parse_grant(g)?);
                rest = &rest[2..];
            }
            Some("--allow-connect") => {
                let g = rest.get(1).ok_or("--allow-connect needs ADDR:PORT or ADDR:*")?;
                connect.push(aipl_core::net::Net::parse_connect_grant(g)?);
                rest = &rest[2..];
            }
            Some("--allow-run") => {
                allow_run.push(rest.get(1).ok_or("--allow-run needs a program's path, or *")?.clone());
                rest = &rest[2..];
            }
            _ => break,
        }
    }
    let file = rest.first().ok_or("usage: aipl-run [--sandbox] [--listen ADDR:PORT]... [--allow-connect HOST:PORT]... [--allow-run PROGRAM]... [--name NAME] PROGRAM.wasm [ARGS...], or aipl-run --serve-fresh ADDR:PORT [--requests N] [--name NAME] PROGRAM.wasm [ARGS...]")?;
    let wasm = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
    if temporary {
        let _ = std::fs::remove_file(file);
        if let Some(dir) = std::path::Path::new(file).parent() {
            let _ = std::fs::remove_dir(dir);
        }
    }
    if requests.is_some() && fresh.is_none() {
        return Err("--requests goes with --serve-fresh".into());
    }
    let mut argv = rest.to_vec();
    if let Some(n) = name {
        argv[0] = n;
    }
    Ok(Program { wasm, argv, sandbox, listen, connect, allow_run, fresh: fresh.map(|at| (at, requests)) })
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

/// n bytes at `at` of the calling instance's memory (its own, or the
/// threads' shared one), or None past its end.
fn read_mem(caller: &mut wasmtime::Caller<'_, WasiP1Ctx>, shared: &Option<SharedMemory>, at: i32, n: i32) -> Option<Vec<u8>> {
    let (at, n) = (at as u32 as usize, n as u32 as usize);
    match caller.get_export("memory").and_then(|e| e.into_memory()) {
        Some(m) => m.data(&caller).get(at..at.checked_add(n)?).map(|b| b.to_vec()),
        None => read_shared(shared.as_ref()?, at, n),
    }
}

/// Writes bytes at `at` of the calling instance's memory; false past its end.
fn write_mem(caller: &mut wasmtime::Caller<'_, WasiP1Ctx>, shared: &Option<SharedMemory>, at: i32, bytes: &[u8]) -> bool {
    let at = at as u32 as usize;
    let Some(end) = at.checked_add(bytes.len()) else { return false };
    match caller.get_export("memory").and_then(|e| e.into_memory()) {
        Some(m) => match m.data_mut(&mut *caller).get_mut(at..end) {
            Some(d) => {
                d.copy_from_slice(bytes);
                true
            }
            None => false,
        },
        None => match shared.as_ref().and_then(|s| s.data().get(at..end)) {
            Some(cells) => {
                // as fs.read does: the host writes the bytes it was asked to
                for (c, b) in cells.iter().zip(bytes) {
                    unsafe { *c.get() = *b };
                }
                true
            }
            None => false,
        },
    }
}

/// The `aipl_net_v1` imports (src/net.rs), on `net`'s sockets.
fn add_net(linker: &mut Linker<WasiP1Ctx>, net: &aipl_core::net::Net, shared: &Option<SharedMemory>) -> wasmtime::Result<()> {
    let (n, sh) = (net.clone(), shared.clone());
    linker.func_wrap("aipl_net_v1", "listen", move |mut c: wasmtime::Caller<'_, WasiP1Ctx>, addr: i32, len: i32, port: i32| -> i32 {
        match read_mem(&mut c, &sh, addr, len) {
            Some(a) => n.listen(&a, port),
            None => -1,
        }
    })?;
    let (n, sh) = (net.clone(), shared.clone());
    linker.func_wrap("aipl_net_v1", "connect", move |mut c: wasmtime::Caller<'_, WasiP1Ctx>, addr: i32, len: i32, port: i32| -> i32 {
        match read_mem(&mut c, &sh, addr, len) {
            Some(a) => n.connect(&a, port),
            None => -1,
        }
    })?;
    let (n, sh) = (net.clone(), shared.clone());
    linker.func_wrap("aipl_net_v1", "connect_host", move |mut c: wasmtime::Caller<'_, WasiP1Ctx>, name: i32, len: i32, port: i32| -> i32 {
        match read_mem(&mut c, &sh, name, len) {
            Some(a) => n.connect_host(&a, port),
            None => -1,
        }
    })?;
    let (n, sh) = (net.clone(), shared.clone());
    linker.func_wrap("aipl_net_v1", "tls_connect", move |mut c: wasmtime::Caller<'_, WasiP1Ctx>, h: i32, name: i32, len: i32| -> i32 {
        match read_mem(&mut c, &sh, name, len) {
            Some(a) => n.tls_connect(h, &a),
            None => aipl_core::net::BAD_NAME,
        }
    })?;
    let n = net.clone();
    linker.func_wrap("aipl_net_v1", "accept", move |h: i32| -> i32 { n.accept(h) })?;
    let (n, sh) = (net.clone(), shared.clone());
    linker.func_wrap("aipl_net_v1", "recv", move |mut c: wasmtime::Caller<'_, WasiP1Ctx>, h: i32, buf: i32, max: i32| -> i32 {
        if read_mem(&mut c, &sh, buf, max).is_none() {
            return -1;
        }
        let mut into = vec![0u8; max as u32 as usize];
        let got = n.recv(h, &mut into);
        if got > 0 && !write_mem(&mut c, &sh, buf, &into[..got as usize]) {
            return -1;
        }
        got
    })?;
    let (n, sh) = (net.clone(), shared.clone());
    linker.func_wrap("aipl_net_v1", "send", move |mut c: wasmtime::Caller<'_, WasiP1Ctx>, h: i32, buf: i32, len: i32| -> i32 {
        match read_mem(&mut c, &sh, buf, len) {
            Some(b) => n.send(h, &b),
            None => -1,
        }
    })?;
    let n = net.clone();
    linker.func_wrap("aipl_net_v1", "close", move |h: i32| -> i32 { n.close(h) })?;
    Ok(())
}

/// The `aipl_sys_v1` imports (src/sys.rs). A path or buffer outside the
/// program's memory is minus EFAULT, a value and not a trap.
fn add_sys(linker: &mut Linker<WasiP1Ctx>, shared: &Option<SharedMemory>, sandbox: bool, procs: &aipl_core::sys::Procs) -> wasmtime::Result<()> {
    let sh = shared.clone();
    linker.func_wrap("aipl_sys_v1", "chmod", move |mut c: wasmtime::Caller<'_, WasiP1Ctx>, dirfd: i32, path: i32, len: i32, mode: i32| -> i32 {
        if sandbox && dirfd == 4 {
            return -8;
        }
        match read_mem(&mut c, &sh, path, len) {
            Some(p) => aipl_core::sys::chmod_at(dirfd, &p, mode),
            None => -21,
        }
    })?;
    // the process operations (src/sys.rs); a range outside memory is -21
    let (pr, sh) = (procs.clone(), shared.clone());
    linker.func_wrap("aipl_sys_v1", "spawn", move |mut c: wasmtime::Caller<'_, WasiP1Ctx>, path: i32, len: i32, argv: i32, argv_len: i32, stdio: i32| -> i32 {
        match (read_mem(&mut c, &sh, path, len), read_mem(&mut c, &sh, argv, argv_len)) {
            (Some(p), Some(a)) => pr.spawn(&p, &a, stdio),
            _ => -21,
        }
    })?;
    let pr = procs.clone();
    linker.func_wrap("aipl_sys_v1", "stream", move |p: i32, which: i32| -> i32 { pr.stream(p, which) })?;
    let (pr, sh) = (procs.clone(), shared.clone());
    linker.func_wrap("aipl_sys_v1", "read", move |mut c: wasmtime::Caller<'_, WasiP1Ctx>, h: i32, buf: i32, max: i32| -> i32 {
        if read_mem(&mut c, &sh, buf, max).is_none() {
            return -21;
        }
        match pr.read(h, max as u32 as usize) {
            Ok(bytes) => {
                if !bytes.is_empty() && !write_mem(&mut c, &sh, buf, &bytes) {
                    return -21;
                }
                bytes.len() as i32
            }
            Err(e) => e,
        }
    })?;
    let (pr, sh) = (procs.clone(), shared.clone());
    linker.func_wrap("aipl_sys_v1", "write", move |mut c: wasmtime::Caller<'_, WasiP1Ctx>, h: i32, buf: i32, len: i32| -> i32 {
        match read_mem(&mut c, &sh, buf, len) {
            Some(b) => pr.write(h, &b),
            None => -21,
        }
    })?;
    let pr = procs.clone();
    linker.func_wrap("aipl_sys_v1", "close", move |h: i32| -> i32 { pr.close(h) })?;
    let pr = procs.clone();
    linker.func_wrap("aipl_sys_v1", "wait", move |p: i32| -> i32 { pr.wait(p) })?;
    let (pr, sh) = (procs.clone(), shared.clone());
    linker.func_wrap("aipl_sys_v1", "exec", move |mut c: wasmtime::Caller<'_, WasiP1Ctx>, path: i32, len: i32, argv: i32, argv_len: i32| -> i32 {
        match (read_mem(&mut c, &sh, path, len), read_mem(&mut c, &sh, argv, argv_len)) {
            (Some(p), Some(a)) => pr.exec(&p, &a),
            _ => -21,
        }
    })?;
    Ok(())
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

/// A fresh instance per connection (docs/design/NET_PLAN.md mode B): the
/// module is compiled once; each connection gets a new instance whose
/// standard input is the connection and whose standard output is held
/// back. An instance that exits with 0 has what it wrote sent; one that
/// traps or exits otherwise has the client sent a bare 500, and its message
/// (with the call chain) goes to this process's standard error, as does
/// why an answer over FRESH_CAP was not sent. Nothing
/// outlives a request. A threaded module is refused.
/// The most a fresh instance's answer may be (aipl_src/native/fresh.aipl cap).
const FRESH_CAP: usize = 64 << 20;

fn serve_fresh(p: Program) -> i32 {
    use std::io::Write;
    let (at, requests) = p.fresh.expect("serve_fresh needs --serve-fresh");
    let argv0 = p.argv.first().cloned().unwrap_or_else(|| "aipl-run".into());
    let fail = |e: String| -> i32 {
        eprintln!("{argv0}: {e}");
        1
    };
    let mut config = Config::new();
    config.wasm_backtrace_max_frames(std::num::NonZeroUsize::new(1 << 24));
    let engine = match Engine::new(&config) {
        Ok(e) => e,
        Err(e) => return fail(e.to_string()),
    };
    let module = match Module::new(&engine, &p.wasm) {
        Ok(m) => m,
        Err(e) => return fail(format!("not a valid module: {e}")),
    };
    if module.imports().any(|i| i.module() == "wasi" && i.name() == "thread-spawn") {
        return fail("a threaded module cannot be served a fresh instance per connection yet".into());
    }
    let imported = module.imports().filter(|i| matches!(i.ty(), wasmtime::ExternType::Func(_))).count() as u32;
    let _ = LINES.set(LineTable::read(&p.wasm, imported));
    let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
    if let Err(e) = wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t) {
        return fail(e.to_string());
    }
    if let Err(e) = add_sys(&mut linker, &None, p.sandbox, &aipl_core::sys::Procs::new(p.allow_run.clone())) {
        return fail(e.to_string());
    }
    if let Err(e) = add_net(&mut linker, &aipl_core::net::Net::new(p.listen.clone()).with_connect(p.connect.clone()), &None) {
        return fail(e.to_string());
    }
    let listener = match std::net::TcpListener::bind(at) {
        Ok(l) => l,
        Err(e) => return fail(format!("cannot listen on {at}: {e}")),
    };
    let mut served = 0usize;
    while requests.is_none_or(|n| served < n) {
        let Ok((conn, _)) = listener.accept() else { continue };
        served += 1;
        let Ok(input) = conn.try_clone() else { continue };
        let output = wasmtime_wasi::p2::pipe::MemoryOutputPipe::new(FRESH_CAP);
        let mut b = WasiCtxBuilder::new();
        b.stdin(wasmtime_wasi::cli::InputFile::new(std::fs::File::from(std::os::fd::OwnedFd::from(input))));
        b.stdout(output.clone()).inherit_stderr().inherit_env().args(&p.argv);
        let opened = b.preopened_dir(".", ".", FsPerms::ReadWrite).is_ok() && (p.sandbox || b.preopened_dir("/", "/", FsPerms::ReadWrite).is_ok());
        if !opened {
            return fail("cannot open the working directory".into());
        }
        let mut store = Store::new(&engine, b.build_p1());
        let code = match linker.instantiate(&mut store, &module).and_then(|i| i.get_typed_func::<(), ()>(&mut store, "_start").map(|f| (i, f))) {
            Ok((inst, start)) => {
                let r = start.call(&mut store, ());
                let data = inst.get_memory(&mut store, "memory").map(|m| m.data(&store).to_vec()).unwrap_or_default();
                status(r, &argv0, &move |at, n| data.get(at..at.checked_add(n)?).map(|b| b.to_vec()))
            }
            Err(e) => {
                eprintln!("{argv0}: cannot start: {e}");
                1
            }
        };
        // a write that does not fit takes what room is left, so a full pipe
        // means the answer was longer than a fresh instance may write
        if code != 0 && output.contents().len() >= FRESH_CAP {
            eprintln!("{argv0}: {}", aipl_core::net::FRESH_TOO_LONG);
        }
        let mut conn = conn;
        let _ = if code == 0 {
            conn.write_all(&output.contents())
        } else {
            conn.write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/plain; charset=utf-8\r\nConnection: close\r\nContent-Length: 22\r\n\r\nInternal Server Error\n")
        };
        let _ = conn.shutdown(std::net::Shutdown::Both);
    }
    0
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
    if std::env::var_os("AIPL_PROFILE").is_some_and(|v| v == "perfmap") {
        config.profiler(wasmtime::ProfilingStrategy::PerfMap);
    }
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
        // the memory the module imports: 16 pages, to its declared maximum
        // (2 GiB, vm.rs MAX_PAGES, or lower with `aipl compile --max-memory`)
        let max = module
            .imports()
            .find_map(|i| match i.ty() {
                wasmtime::ExternType::Memory(m) if i.module() == "env" && i.name() == "memory" => m.maximum(),
                _ => None,
            })
            .unwrap_or(32768) as u32;
        let memory = match SharedMemory::new(&engine, MemoryType::shared(16, max)) {
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
    if let Err(e) = add_sys(&mut linker, &shared, p.sandbox, &aipl_core::sys::Procs::new(p.allow_run.clone())) {
        return fail(e.to_string());
    }
    if let Err(e) = add_net(&mut linker, &aipl_core::net::Net::new(p.listen.clone()).with_connect(p.connect.clone()), &shared) {
        return fail(e.to_string());
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
