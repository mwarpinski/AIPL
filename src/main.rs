use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::resolver::Resolver;
use aipl_core::vm::{Value, VM};
use clap::{Parser as ClapParser, Subcommand};
use std::fs;
use std::path::Path;

#[derive(ClapParser)]
#[command(name = "aipl", version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("AIPL_COMMIT"), ")"))]
#[command(about = "AIPL: an unambiguous, statically typed S-expression language for AI agents. Runs in a reference VM and compiles to WebAssembly + WASI, with a self-hosted compiler written in AIPL.")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Evaluate an AIPL source file directly using the VM interpreter; or,
    /// with -e, expressions, printing the last one's value and type
    /// (aipl eval -e '(call str.find ...)' [--with FILE] [--wasm | --native])
    Eval {
        /// The program to run (not with -e)
        #[arg(required_unless_present = "expr")]
        file: Option<String>,
        #[arg(short, long, default_value = "main")]
        func: String,
        /// Expressions to evaluate; the last one's value is printed with its type
        #[arg(short = 'e', long = "expr", conflicts_with = "file")]
        expr: Option<String>,
        /// With -e: evaluate inside this file's module (its functions, private
        /// ones included, are in scope; its main is not run)
        #[arg(long, requires = "expr")]
        with: Option<String>,
        /// With -e: compile to wasm and run under aipl-run instead of the VM
        #[arg(long, requires = "expr", conflicts_with = "native")]
        wasm: bool,
        /// With -e: compile to a native executable and run it instead of the VM
        #[arg(long, requires = "expr")]
        native: bool,
        /// An address and port the program may listen on, as 127.0.0.1:8080
        /// or [::1]:8080 (repeatable; none by default)
        #[arg(long = "listen", value_name = "ADDR:PORT")]
        listen: Vec<String>,
        /// A program the program may start with proc.spawn or proc.exec, as
        /// /bin/sh, or * for any (repeatable; none by default)
        #[arg(long = "allow-run", value_name = "PROGRAM")]
        allow_run: Vec<String>,
        /// The most memory the program may use, from 1M to 2G (the default) in
        /// whole 64K pages, as 512M (what `aipl compile --max-memory` records)
        #[arg(long = "max-memory", value_name = "SIZE")]
        max_memory: Option<String>,
        /// Command-line arguments for the program (after `--`); with FILE as
        /// argv[0] they are what std/os.arg reports.
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Compile an AIPL program to a WebAssembly module, or with --exe to a standalone executable;
    /// through the optimizer unless --no-opt
    Compile {
        file: String,
        #[arg(short, long, default_value = "out.wasm")]
        output: String,
        /// Build without the optimizer (aipl_src/codegen_opt.aipl, which runs by default):
        /// the Rust code generator's module, byte for byte the self-hosted toolchain's
        #[arg(long = "no-opt")]
        no_opt: bool,
        /// Also compile with the self-hosted toolchain (driver.aipl: resolver, checker, codegen;
        /// compiled to wasm by the Rust toolchain and run under wasmtime)
        /// and fail unless its bytes equal the Rust compiler's; implies --no-opt
        #[arg(long = "self")]
        self_flag: bool,
        /// Write a standalone executable: native machine code on Linux x86-64, else the
        /// aipl-run launcher with the module appended (see --target)
        #[arg(long)]
        exe: bool,
        /// With --exe: the program may only access the working directory, not absolute paths
        #[arg(long, requires = "exe")]
        sandbox: bool,
        /// With --exe: an address and port the program may listen on, fixed in the
        /// executable (repeatable; none by default)
        #[arg(long = "listen", value_name = "ADDR:PORT", requires = "exe")]
        listen: Vec<String>,
        /// With --exe: a program the program may start with proc.spawn or
        /// proc.exec, as /bin/sh, or * for any, fixed in the executable
        /// (repeatable; none by default)
        #[arg(long = "allow-run", value_name = "PROGRAM", requires = "exe")]
        allow_run: Vec<String>,
        /// With --exe: serve on this address a fresh copy of the program per connection,
        /// its standard input the connection and its output the answer (std/serve's once);
        /// a trap answers 500 and the next connection is served (docs/design/NET_PLAN.md mode B)
        #[arg(long = "serve", value_name = "ADDR:PORT", requires = "exe")]
        serve: Option<String>,
        /// The most memory the program may use, from 1M to 2G (the default) in whole
        /// 64K pages, as 512M; recorded in the module, so every host stops there
        #[arg(long = "max-memory", value_name = "SIZE")]
        max_memory: Option<String>,
        /// With --exe: `native` (Linux x86-64; the default there), `wasm` (the launcher bundle;
        /// the default elsewhere), or `arm64` (a Linux AArch64 executable, which runs on an AArch64
        /// machine: begun, docs/design/ARM64_PLAN.md)
        #[arg(long, requires = "exe", value_parser = ["native", "wasm", "arm64"])]
        target: Option<String>,
        /// Compile the file's test build: its (test ...) items as functions, and a _start
        /// running the test its first argument names (aipl test runs this)
        #[arg(long)]
        tests: bool,
    },
    /// Serve a program a fresh instance per connection (docs/design/NET_PLAN.md
    /// mode B): aipl serve --fresh FILE --listen ADDR:PORT [--requests N]. The
    /// program reads one request from standard input and answers on standard
    /// output (std/serve's once); a trap answers 500 and the next request is
    /// served. A long-running server is a program that calls serve.run: run it
    /// with aipl eval --listen ADDR:PORT FILE.
    Serve {
        file: String,
        /// A fresh instance per connection (the only way aipl serve runs)
        #[arg(long)]
        fresh: bool,
        /// The address and port to serve on
        #[arg(long = "listen", value_name = "ADDR:PORT")]
        listen: String,
        /// Stop after answering this many connections (no end by default)
        #[arg(long)]
        requests: Option<usize>,
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Run a compiled module with the aipl-run launcher: aipl run prog.wasm [-- ARGS...]
    Run {
        file: String,
        #[arg(long)]
        sandbox: bool,
        /// An address and port the program may listen on (repeatable)
        #[arg(long = "listen", value_name = "ADDR:PORT")]
        listen: Vec<String>,
        /// A program the program may start (repeatable; * for any)
        #[arg(long = "allow-run", value_name = "PROGRAM")]
        allow_run: Vec<String>,
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Parse and type-check an AIPL file without running it (contracts are
    /// type-checked, not proven)
    Verify { file: String },
    /// Run a file (or, with --test, its tests) again each time it or a file it
    /// imports changes: aipl watch FILE [--test] [-- ARGS...]
    Watch {
        file: String,
        /// Run the file's tests instead of its main
        #[arg(long)]
        test: bool,
        /// (internal) stop after this many runs
        #[arg(long, hide = true)]
        max_runs: Option<usize>,
        /// Arguments for the program (after `--`)
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Show a module's public functions and types with their comments:
    /// aipl doc [MODULE | MODULE.ITEM | FILE.aipl] [--private], aipl doc
    /// --find WORD, aipl doc --check FILE.aipl... (aipl_src/doc.aipl)
    Doc {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Run an AIPL test entrypoint and report pass/fail via the process exit
    /// code. The entrypoint owns all test/reporting logic (via sys.print) and
    /// must return an i32 failure count: 0 = every group passed, N = N
    /// groups failed. This is deliberately thin on the Rust side - see
    /// aipl_src/test_suite.aipl for the actual test logic.
    ///
    /// A file with (test "name" ...) items, or a directory of them, runs each
    /// test in a fresh process: aipl test FILE|DIR [-k TEXT] [--vm | --native
    /// | --all-backends].
    Test {
        file: String,
        #[arg(short, long, default_value = "run_all")]
        func: String,
        /// Only the tests whose names contain TEXT
        #[arg(short = 'k')]
        filter: Option<String>,
        /// Run the tests in the VM instead of under aipl-run
        #[arg(long, conflicts_with_all = ["native", "all_backends"])]
        vm: bool,
        /// Run the tests as a native executable instead of under aipl-run
        #[arg(long, conflicts_with = "all_backends")]
        native: bool,
        /// Run each test in the VM, under aipl-run, and natively; it passes only if all agree and pass
        #[arg(long)]
        all_backends: bool,
        /// (internal) run test K in the VM, in this process
        #[arg(long, hide = true)]
        run_one: Option<usize>,
    },
}

/// Locates the first differing byte of two wasm modules: which section (and,
/// in the code section, which function body) it falls in, plus a hex window
/// of each side around it.
/// println! that fails the command, rather than panicking, when standard
/// output cannot be written (a full disk, a closed pipe).
macro_rules! say {
    ($($t:tt)*) => {{
        use std::io::Write;
        writeln!(std::io::stdout(), $($t)*).map_err(|e| format!("standard output: {e}"))?;
    }};
}

fn rust_out_path(output: &str) -> String {
    format!("{}.rust.wasm", output)
}

/// Function bodies of a module's code section, as byte slices.
fn code_bodies(b: &[u8]) -> Vec<&[u8]> {
    let leb = |pos: &mut usize| -> usize {
        let (mut v, mut shift) = (0usize, 0);
        while *pos < b.len() {
            let byte = b[*pos];
            *pos += 1;
            v |= ((byte & 0x7f) as usize) << shift;
            shift += 7;
            if byte & 0x80 == 0 {
                break;
            }
        }
        v
    };
    let mut pos = 8;
    while pos < b.len() {
        let id = b[pos];
        pos += 1;
        let size = leb(&mut pos);
        if id == 10 {
            let mut p = pos;
            let n = leb(&mut p);
            let mut out = Vec::new();
            for _ in 0..n {
                let fsize = leb(&mut p);
                out.push(&b[p.min(b.len())..(p + fsize).min(b.len())]);
                p += fsize;
            }
            return out;
        }
        pos += size;
    }
    Vec::new()
}

fn describe_divergence(rust: &[u8], selfh: &[u8]) -> String {
    // A body that differs in length shifts every later byte, so name the first
    // differing function body directly.
    let (rb, sb) = (code_bodies(rust), code_bodies(selfh));
    if let Some(f) = (0..rb.len().min(sb.len())).find(|&i| rb[i] != sb[i]) {
        let at = (0..rb[f].len().max(sb[f].len())).find(|&i| rb[f].get(i) != sb[f].get(i)).unwrap_or(0);
        let w = |b: &[u8]| b[at.saturating_sub(8)..(at + 16).min(b.len())].iter().map(|x| format!("{:02x}", x)).collect::<Vec<_>>().join(" ");
        return format!(
            "first differing function body: code-section function {} ({} vs {} bytes), first difference at body byte {}\n  rust: {}\n  self: {}",
            f, rb[f].len(), sb[f].len(), at, w(rb[f]), w(sb[f])
        );
    }
    describe_byte_divergence(rust, selfh)
}

fn describe_byte_divergence(rust: &[u8], selfh: &[u8]) -> String {
    let first = (0..rust.len().max(selfh.len()))
        .find(|&i| rust.get(i) != selfh.get(i))
        .unwrap_or(0);
    let leb = |b: &[u8], pos: &mut usize| -> usize {
        let (mut v, mut shift) = (0usize, 0);
        while *pos < b.len() {
            let byte = b[*pos];
            *pos += 1;
            v |= ((byte & 0x7f) as usize) << shift;
            shift += 7;
            if byte & 0x80 == 0 {
                break;
            }
        }
        v
    };
    let names = ["custom", "type", "import", "function", "table", "memory", "global", "export", "start", "element", "code", "data"];
    let mut location = String::from("module header");
    let mut pos = 8;
    while pos < rust.len() {
        let id = rust[pos] as usize;
        let start = pos;
        pos += 1;
        let size = leb(rust, &mut pos);
        let body = pos;
        let end = body + size;
        if first < end {
            location = format!("{} section (id {}) starting at byte {}", names.get(id).unwrap_or(&"unknown"), id, start);
            if id == 10 {
                let mut p = body;
                let count = leb(rust, &mut p);
                for f in 0..count {
                    let fsize = leb(rust, &mut p);
                    if first < p + fsize {
                        location.push_str(&format!(", body of code-section function {} (byte {} of the body)", f, first.saturating_sub(p)));
                        break;
                    }
                    p += fsize;
                }
            }
            break;
        }
        pos = end;
    }
    let window = |b: &[u8]| {
        let lo = first.saturating_sub(8);
        let hi = (first + 16).min(b.len());
        b.get(lo..hi).map(|w| w.iter().map(|x| format!("{:02x}", x)).collect::<Vec<_>>().join(" ")).unwrap_or_default()
    };
    format!(
        "Rust {} bytes, self-hosted {} bytes; first difference at byte {} in the {}\n  rust: {}\n  self: {}\n  (windows start at byte {})",
        rust.len(), selfh.len(), first, location, window(rust), window(selfh), first.saturating_sub(8)
    )
}

/// The VM is a tree-walking interpreter that recurses once per nested AIPL
/// call, so commands run on a thread with a large stack (reserved, not
/// committed up front) rather than the default 8 MiB main stack.
fn main() {
    let result = std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(|| run().map_err(|e| e.to_string()))
        .map_err(|e| e.to_string())
        .and_then(|t| t.join().map_err(|_| "aipl: command thread panicked".to_string()))
        .and_then(|r| r);
    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

/// The aipl-run launcher: $AIPL_RUNNER, else `aipl-run` next to this binary.
fn runner_path() -> Result<std::path::PathBuf, String> {
    if let Ok(p) = std::env::var("AIPL_RUNNER") {
        return Ok(p.into());
    }
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let p = me.with_file_name(if cfg!(windows) { "aipl-run.exe" } else { "aipl-run" });
    if p.exists() {
        Ok(p)
    } else {
        Err(format!("cannot find the aipl-run launcher at {} (build it with cargo build, or set AIPL_RUNNER)", p.display()))
    }
}

/// Ends the process with exit status `n`, as compiled programs end: a
/// status outside 0-125 is an error with status 134, as under wasmtime
/// (shells keep 126 and up).
fn exit_with(program: &str, n: i64) -> ! {
    if (0..=125).contains(&n) {
        std::process::exit(n as i32)
    }
    eprintln!("{program}: exit with invalid exit status outside of [0..126)");
    std::process::exit(134)
}

/// A standalone executable (AIPL_SPEC.md 6.5): the launcher, then the module,
/// then the grants (one a line: --listen's ADDR:PORT, or `run:PROGRAM` for each
/// --allow-run), then a 20-byte trailer
/// [flags u32 LE][grants length u32 LE][module length u32 LE]["AIPLEXE2"].
/// Flags: bit 0 sandbox, bit 1 --serve, whose address is then the grants'
/// first line. Rust only because WASI cannot set the executable bit; the
/// format is plain.
/// Writes `bytes` as a new file at `path`, replacing what is there. A file
/// that was run is never opened for writing again: macOS kills the next run
/// of a binary rewritten in place (its code signature is stale), and Linux can
/// refuse the open with "Text file busy" just after the old run ends.
fn write_replacing(path: &str, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = format!("{path}.tmp{}", std::process::id());
    fs::write(&tmp, bytes).and_then(|_| fs::rename(&tmp, path)).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

fn write_executable(output: &str, wasm: &[u8], sandbox: bool, listen: &[String], serve: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = fs::read(runner_path()?)?;
    if bytes.ends_with(b"AIPLEXE1") || bytes.ends_with(b"AIPLEXE2") {
        return Err("the aipl-run launcher already has a program appended".into());
    }
    for g in listen.iter().filter(|g| !g.starts_with("run:")) {
        aipl_core::net::Net::parse_grant(g)?;
    }
    // refused when compiled, as the native backend refuses it
    if serve.is_some() {
        let module = wasmtime::Module::new(&wasmtime::Engine::default(), wasm)?;
        if module.imports().any(|i| i.module() == "wasi" && i.name() == "thread-spawn") {
            return Err("a threaded module cannot be served a fresh instance per connection yet".into());
        }
    }
    let grants: String = serve.iter().map(|s| s.to_string()).chain(listen.iter().cloned()).map(|g| format!("{g}\n")).collect();
    bytes.extend_from_slice(wasm);
    bytes.extend_from_slice(grants.as_bytes());
    bytes.extend_from_slice(&(sandbox as u32 | (serve.is_some() as u32) << 1).to_le_bytes());
    bytes.extend_from_slice(&(grants.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(wasm.len() as u32).to_le_bytes());
    bytes.extend_from_slice(b"AIPLEXE2");
    write_replacing(output, &bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(output, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Eval { file, func, expr, with, wasm, native, listen, allow_run, max_memory, args } => {
            if let Some(exprs) = expr {
                use aipl_core::evalexpr::{run, Backend};
                let backend = if wasm { Backend::Wasm } else if native { Backend::Native } else { Backend::Vm };
                let runner = if wasm { runner_path()? } else { std::path::PathBuf::new() };
                std::process::exit(run(&exprs, with.as_deref().map(Path::new), backend, &runner, &args)?);
            }
            let file = file.unwrap_or_default();
            let module = Resolver::resolve(Path::new(&file))?;
            let mut checker = TypeChecker::new();
            checker.check_module(&module)?;

            let mut vm = VM::new();
            vm.set_args(std::iter::once(file.clone()).chain(args).collect());
            vm.set_listen(listen.iter().map(|g| aipl_core::net::Net::parse_grant(g)).collect::<Result<_, _>>()?);
            vm.set_allow_run(allow_run);
            if let Some(m) = &max_memory {
                vm.set_max_pages(aipl_core::compiler::memory_pages(m)? as usize);
            }
            vm.load_module(module);
            say!("[AIPL VM] Executing function '{}' from '{}'...", func, file);
            // an error is followed by its call chain, as compiled code prints it
            let res = match vm.invoke(&func, vec![]) {
                // the VM's sys.exit ends the run with this error: exit as compiled code does
                Err(e) if e.starts_with("sys.exit(") && e.ends_with(") requested") => {
                    exit_with(&file, e["sys.exit(".len()..e.len() - ") requested".len()].parse().unwrap_or(1))
                }
                r => r.map_err(|e| format!("{e}\n{}", vm.call_chain()).trim_end().to_string())?,
            };
            say!("[AIPL Result]: {:?}", res);
            // main's value is the exit status (AIPL_SPEC.md 6.5), as compiled
            // programs exit: an i32, or a (result i32 str)'s ok, or 1 after
            // err's message on standard error
            if func == "main" {
                use aipl_core::vm::Value;
                match res {
                    Value::Int(n) => exit_with(&file, n),
                    Value::Ok(v) => {
                        if let Value::Int(n) = *v {
                            exit_with(&file, n)
                        }
                    }
                    Value::Err(e) => {
                        if let Value::Str(m) = *e {
                            eprintln!("{m}");
                            std::process::exit(1)
                        }
                    }
                    _ => {}
                }
            }
        }
        Commands::Compile { file, output, no_opt, self_flag, exe, sandbox, listen, allow_run, serve, max_memory, target, tests } => {
            // a grant is checked before anything is compiled (aipl.aipl does the same)
            for g in &listen {
                aipl_core::net::Net::parse_grant(g)?;
            }
            // the grants an executable carries: addresses, then `run:PROGRAM` for each program
            let listen: Vec<String> = listen.into_iter().chain(allow_run.into_iter().map(|p| format!("run:{p}"))).collect();
            if let Some(s) = &serve {
                aipl_core::net::Net::parse_address("--serve", s)?;
            }
            let max_pages = match &max_memory {
                Some(m) => aipl_core::compiler::memory_pages(m)?,
                None => aipl_core::vm::MAX_PAGES as u32,
            };
            let module = if tests { Resolver::resolve_tests(Path::new(&file))?.0 } else { Resolver::resolve(Path::new(&file))? };
            let mut checker = TypeChecker::new();
            checker.check_module(&module)?;

            if self_flag {
                let rust_bytes = WasmCompiler::compile_limited(&module, max_pages)?;
                say!("[AIPL Self-Host] Compiling '{}' via self-hosted codegen.aipl...", file);
                // Self-hosted end to end, as aiplc: driver.aipl resolves,
                // checks, and compiles (aipl_src/driver.aipl), in the VM.
                let self_bytes = aipl_core::selfhost::compile_with_aipl_limited(Path::new(&file), max_pages).map_err(|e| format!("Self-host error: {}", e))?;
                if rust_bytes != self_bytes {
                    eprintln!("[AIPL Self-Host ERROR] Mismatch between Rust backend and self-hosted codegen!");
                    eprintln!("{}", describe_divergence(&rust_bytes, &self_bytes));
                    let self_out = format!("{}.self.wasm", output);
                    fs::write(rust_out_path(&output), &rust_bytes)?;
                    fs::write(&self_out, &self_bytes)?;
                    eprintln!("  wrote both outputs: {} and {}", rust_out_path(&output), self_out);
                    return Err("Byte-parity mismatch between Rust backend and self-hosted codegen!".into());
                }
                say!("[AIPL Self-Host] SUCCESS: Self-hosted codegen produced 100% BIT-FOR-BIT IDENTICAL WebAssembly!");
                write_replacing(&output, &self_bytes)?;
            } else if no_opt {
                write_replacing(&output, &WasmCompiler::compile_limited(&module, max_pages)?)?;
            } else {
                // the optimizer (docs/design/OPT_PLAN.md): driver.aipl's
                // compile_file_opt under wasmtime, after the checks above, so a
                // program's errors read as they always have; its failure on a
                // program those passed is a toolchain bug
                let bytes = aipl_core::selfhost::compile_optimized(Path::new(&file), max_pages, tests)
                    .map_err(|e| format!("{e}\n(the optimized build failed on a program that checks: a toolchain bug; --no-opt builds without the optimizer)"))?;
                write_replacing(&output, &bytes)?;
            }
            if exe {
                // `output` holds the module; replace it with the executable
                let wasm = fs::read(&output)?;
                if target.as_deref() == Some("arm64") {
                    if sandbox || !listen.is_empty() || serve.is_some() {
                        let _ = fs::remove_file(&output);
                        return Err("--target arm64: --sandbox, --listen, --allow-run, and --serve are not built for AArch64 yet".into());
                    }
                    let exe_bytes = aipl_core::native::executable_arm64(&wasm).inspect_err(|_| {
                        let _ = fs::remove_file(&output);
                    })?;
                    write_replacing(&output, &exe_bytes)?;
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        fs::set_permissions(&output, fs::Permissions::from_mode(0o755))?;
                    }
                    say!("[AIPL Compiler] Successfully compiled '{}' -> '{}' ({} bytes)", file, output, fs::metadata(&output)?.len());
                    return Ok(());
                }
                let native = match target.as_deref() {
                    Some("native") if !aipl_core::native::supported() => {
                        return Err("--target native: the native backend targets Linux x86-64 only (use --target wasm)".into())
                    }
                    Some("native") => true,
                    Some(_) => false,
                    None => aipl_core::native::supported(),
                };
                if native {
                    // a native executable keeps each program's path in 256 bytes (aipl_src/native/procs.aipl)
                    if let Some(g) = listen.iter().filter_map(|g| g.strip_prefix("run:")).find(|p| p.len() > 256) {
                        return Err(format!("--allow-run {g}: the program's path is longer than 256 bytes, the most a native executable's grant holds").into());
                    }
                    // on failure, no module is left where the executable was asked for
                    let exe_bytes = aipl_core::native::executable_with(&wasm, sandbox, &listen, serve.as_deref()).inspect_err(|_| {
                        let _ = fs::remove_file(&output);
                    })?;
                    write_replacing(&output, &exe_bytes)?;
                    // WASI cannot set the execute bit, so the host does (ROADMAP.md)
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        fs::set_permissions(&output, fs::Permissions::from_mode(0o755))?;
                    }
                } else {
                    // launcher + module + trailer
                    write_executable(&output, &wasm, sandbox, &listen, serve.as_deref()).inspect_err(|_| {
                        let _ = fs::remove_file(&output);
                    })?;
                }
            }
            say!("[AIPL Compiler] Successfully compiled '{}' -> '{}' ({} bytes)", file, output, fs::metadata(&output)?.len());
        }
        Commands::Serve { file, fresh, listen, requests, args } => {
            if !fresh {
                return Err(format!("aipl serve runs a fresh instance per connection: add --fresh. A long-running server is a program that calls serve.run: run it with aipl eval --listen {listen} {file}").into());
            }
            aipl_core::net::Net::parse_grant(&listen)?;
            let module = Resolver::resolve(Path::new(&file))?;
            TypeChecker::new().check_module(&module)?;
            let wasm = WasmCompiler::compile(&module)?;
            let dir = std::env::temp_dir().join(format!("aipl_serve_{}", std::process::id()));
            fs::create_dir_all(&dir)?;
            let module_path = dir.join("program.wasm");
            fs::write(&module_path, &wasm)?;
            // this process becomes aipl-run, so ending it (by its id) ends the
            // server: no child is left serving; aipl-run removes the module
            // once it has read it
            let mut cmd = std::process::Command::new(runner_path()?);
            cmd.args(["--serve-fresh", &listen, "--temporary"]);
            if let Some(n) = requests {
                cmd.args(["--requests", &n.to_string()]);
            }
            cmd.arg("--name").arg(&file).arg(&module_path).args(&args);
            use std::os::unix::process::CommandExt;
            let e = cmd.exec();
            let _ = fs::remove_dir_all(&dir);
            return Err(format!("cannot start aipl-run: {e}").into());
        }
        Commands::Run { file, sandbox, listen, allow_run, args } => {
            if file.ends_with(".aipl") {
                return Err(format!(
                    "aipl run takes a compiled .wasm file, not source: run {file} with `aipl eval {file}`, or compile it first with `aipl compile {file} -o out.wasm`"
                )
                .into());
            }
            let mut cmd = std::process::Command::new(runner_path()?);
            if sandbox {
                cmd.arg("--sandbox");
            }
            for g in &listen {
                cmd.arg("--listen").arg(g);
            }
            for p in &allow_run {
                cmd.arg("--allow-run").arg(p);
            }
            let status = cmd.arg(&file).args(&args).status()?;
            std::process::exit(status.code().unwrap_or(1));
        }
        Commands::Watch { file, test, max_runs, args } => {
            let me = std::env::current_exe().map_err(|e| e.to_string())?;
            aipl_core::watch::run(Path::new(&file), test, &args, &me, max_runs)?;
        }
        Commands::Doc { args } => {
            std::process::exit(aipl_core::tools::run("doc.aipl", "aipl doc", &args)?);
        }
        Commands::Verify { file } => {
            let module = Resolver::resolve(Path::new(&file))?;
            let mut checker = TypeChecker::new();
            checker.check_module(&module)?;
            say!("[AIPL Verifier] OK: module '{}' type-checks. Contracts are type-checked, not proven; req/ens run on every call, in the VM and in compiled code.", module.name);
        }
        Commands::Test { file, func, filter, vm, native, all_backends, run_one } => {
            use aipl_core::testrun::{self, Backend};
            if let Some(k) = run_one {
                std::process::exit(testrun::run_one_in_vm(Path::new(&file), k)?);
            }
            let path = Path::new(&file);
            let with_tests: Vec<std::path::PathBuf> = testrun::files(path)?.into_iter().filter(|f| !testrun::tests_in(f).is_empty()).collect();
            if !with_tests.is_empty() {
                let backends = if all_backends {
                    vec![Backend::Vm, Backend::Wasm, Backend::Native]
                } else if vm {
                    vec![Backend::Vm]
                } else if native {
                    vec![Backend::Native]
                } else {
                    vec![Backend::Wasm]
                };
                let runner = if backends.contains(&Backend::Wasm) { runner_path()? } else { std::path::PathBuf::new() };
                let me = std::env::current_exe().map_err(|e| e.to_string())?;
                let (mut passed, mut failed) = (0, 0);
                for f in &with_tests {
                    let (p, q) = testrun::run_file(f, filter.as_deref(), &backends, &me, &runner)?;
                    passed += p;
                    failed += q;
                }
                let n = passed + failed;
                say!("{n} test{}: {passed} passed, {failed} failed", if n == 1 { "" } else { "s" });
                std::process::exit(if failed > 0 { 1 } else { 0 });
            }
            if path.is_dir() {
                return Err(format!("{}: no (test ...) in any .aipl file below it", path.display()).into());
            }
            let module = Resolver::resolve(Path::new(&file))?;
            let mut checker = TypeChecker::new();
            checker.check_module(&module)?;

            let mut vm = VM::new();
            vm.load_module(module);
            say!("[AIPL Test] Running '{}' from '{}'...\n", func, file);
            match vm.invoke(&func, vec![]) {
                Ok(Value::Int(0)) => {
                    say!("\n[AIPL Test] All groups passed.");
                }
                Ok(Value::Int(n)) => {
                    eprintln!("\n[AIPL Test] {} group(s) failed.", n);
                    std::process::exit(1);
                }
                Ok(other) => {
                    eprintln!(
                        "\n[AIPL Test] Entrypoint returned {:?}, expected an Int failure count (0 = pass).",
                        other
                    );
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("\n[AIPL Test] Error: {}\n{}", e, vm.call_chain().trim_end());
                    std::process::exit(1);
                }
            }
        }
    }

    Ok(())
}
