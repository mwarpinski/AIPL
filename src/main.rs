use aipl_core::agent_api::server::AgentServer;
use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::resolver::Resolver;
use aipl_core::vm::{Value, VM};
use clap::{Parser as ClapParser, Subcommand};
use std::fs;
use std::path::Path;

#[derive(ClapParser)]
#[command(name = "aipl")]
#[command(about = "AIPL: an unambiguous, statically typed S-expression language for AI agents. Runs in a reference VM and compiles to WebAssembly + WASI, with a self-hosted compiler written in AIPL.")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Evaluate an AIPL source file directly using the VM interpreter
    Eval {
        file: String,
        #[arg(short, long, default_value = "main")]
        func: String,
        /// Command-line arguments for the program (after `--`); with FILE as
        /// argv[0] they are what std/os.arg reports.
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Compile an AIPL program to a WebAssembly module, or with --exe to a standalone executable
    Compile {
        file: String,
        #[arg(short, long, default_value = "out.wasm")]
        output: String,
        /// Also compile with the self-hosted toolchain (resolver.aipl + codegen.aipl, in the VM)
    /// and fail unless its bytes equal the Rust compiler's
        #[arg(long = "self")]
        self_flag: bool,
        /// Write a standalone executable: native machine code on Linux x86-64, else the
        /// aipl-run launcher with the module appended (see --target)
        #[arg(long)]
        exe: bool,
        /// With --exe: the program may only access the working directory, not absolute paths
        #[arg(long, requires = "exe")]
        sandbox: bool,
        /// With --exe: `native` (Linux x86-64; the default there) or `wasm` (the launcher bundle;
        /// the default elsewhere)
        #[arg(long, requires = "exe", value_parser = ["native", "wasm"])]
        target: Option<String>,
    },
    /// Run a compiled module with the aipl-run launcher: aipl run prog.wasm [-- ARGS...]
    Run {
        file: String,
        #[arg(long)]
        sandbox: bool,
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Parse and type-check an AIPL file without running it (contracts are
    /// type-checked, not proven)
    Verify { file: String },
    /// Run an AIPL test entrypoint and report pass/fail via the process exit
    /// code. The entrypoint owns all test/reporting logic (via sys.print) and
    /// must return an i32 failure count: 0 = every group passed, N = N
    /// groups failed. This is deliberately thin on the Rust side - see
    /// aipl_src/test_suite.aipl for the actual test logic.
    Test {
        file: String,
        #[arg(short, long, default_value = "run_all")]
        func: String,
    },
    /// Serve /eval, /verify and /compile over HTTP for agents (JSON responses)
    Serve {
        #[arg(short, long, default_value = "127.0.0.1:8080")]
        addr: String,
    },
}

fn run_self_hosted_codegen(src: &str) -> Result<Vec<u8>, String> {
    let codegen_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("aipl_src/codegen.aipl");
    let codegen_path = codegen_path.as_path();
    let module = Resolver::resolve(&codegen_path).map_err(|e| format!("resolve codegen.aipl: {}", e))?;
    TypeChecker::new().check_module(&module).map_err(|e| format!("check codegen.aipl: {}", e))?;
    let mut vm = VM::new();
    vm.load_module(module);
    
    vm.invoke("init_keywords", vec![]).map_err(|e| format!("init_keywords: {}", e))?;
    
    let src_bytes = src.as_bytes();
    let alloc_res = vm.invoke("alloc_src", vec![Value::Int((src_bytes.len() + 16) as i64)]).map_err(|e| format!("alloc_src: {}", e))?;
    let src_ptr = match alloc_res {
        Value::Int(p) => p as i32,
        other => return Err(format!("expected Int from alloc_src, got {:?}", other)),
    };
    
    vm.write_bytes(src_ptr as usize, src_bytes);
    
    let out_len_val = vm.invoke("compile_module", vec![Value::Int(src_ptr as i64), Value::Int(src_bytes.len() as i64)]).map_err(|e| format!("compile_module: {}", e))?;
    let out_len = match out_len_val {
        Value::Int(l) => l as i32,
        other => return Err(format!("expected Int from compile_module, got {:?}", other)),
    };
    if out_len <= 0 {
        // Cell 4 holds the compile error code (AIPL_SPEC.md 6.4).
        let code = u32::from_le_bytes(vm.read_bytes(4, 4)[..4].try_into().unwrap());
        return Err(format!("compile_module returned {} with compile error {} (see AIPL_SPEC.md 6.4)", out_len, code));
    }
    
    let out_ptr_bytes = vm.read_bytes(60, 4);
    let out_ptr = u32::from_le_bytes(out_ptr_bytes[..4].try_into().unwrap()) as usize;
    Ok(vm.read_bytes(out_ptr, out_len as usize))
}

/// Locates the first differing byte of two wasm modules: which section (and,
/// in the code section, which function body) it falls in, plus a hex window
/// of each side around it.
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
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let result = std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(|| run().map_err(|e| e.to_string()))?
        .join()
        .map_err(|_| "aipl: command thread panicked")?;
    result.map_err(|e| e.into())
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

/// A standalone executable (AIPL_SPEC.md 6.5): the launcher, then the module,
/// then a 16-byte trailer [flags u32 LE][module length u32 LE]["AIPLEXE1"].
/// Rust only because WASI cannot set the executable bit; the format is plain.
fn write_executable(output: &str, wasm: &[u8], sandbox: bool) -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = fs::read(runner_path()?)?;
    if bytes.ends_with(b"AIPLEXE1") {
        return Err("the aipl-run launcher already has a program appended".into());
    }
    bytes.extend_from_slice(wasm);
    bytes.extend_from_slice(&(sandbox as u32).to_le_bytes());
    bytes.extend_from_slice(&(wasm.len() as u32).to_le_bytes());
    bytes.extend_from_slice(b"AIPLEXE1");
    fs::write(output, &bytes)?;
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
        Commands::Eval { file, func, args } => {
            let module = Resolver::resolve(Path::new(&file))?;
            let mut checker = TypeChecker::new();
            checker.check_module(&module)?;

            let mut vm = VM::new();
            vm.set_args(std::iter::once(file.clone()).chain(args).collect());
            vm.load_module(module);
            println!("[AIPL VM] Executing function '{}' from '{}'...", func, file);
            let res = vm.invoke(&func, vec![])?;
            println!("[AIPL Result]: {:?}", res);
        }
        Commands::Compile { file, output, self_flag, exe, sandbox, target } => {
            let module = Resolver::resolve(Path::new(&file))?;
            let mut checker = TypeChecker::new();
            checker.check_module(&module)?;

            let rust_bytes = WasmCompiler::compile(&module)?;
            if self_flag {
                println!("[AIPL Self-Host] Compiling '{}' via self-hosted codegen.aipl...", file);
                // Self-hosted end to end: resolver.aipl flattens the imports into
                // one module, which codegen.aipl compiles.
                let flat_src = aipl_core::selfhost::resolve_with_aipl(Path::new(&file))
                    .map_err(|e| format!("Self-hosted resolver error: {}", e))?;
                let self_bytes = run_self_hosted_codegen(&flat_src).map_err(|e| format!("Self-host error: {}", e))?;
                if rust_bytes != self_bytes {
                    eprintln!("[AIPL Self-Host ERROR] Mismatch between Rust backend and self-hosted codegen!");
                    eprintln!("{}", describe_divergence(&rust_bytes, &self_bytes));
                    let self_out = format!("{}.self.wasm", output);
                    fs::write(&rust_out_path(&output), &rust_bytes)?;
                    fs::write(&self_out, &self_bytes)?;
                    eprintln!("  wrote both outputs: {} and {}", rust_out_path(&output), self_out);
                    return Err("Byte-parity mismatch between Rust backend and self-hosted codegen!".into());
                }
                println!("[AIPL Self-Host] SUCCESS: Self-hosted codegen produced 100% BIT-FOR-BIT IDENTICAL WebAssembly!");
                fs::write(&output, &self_bytes)?;
            } else {
                fs::write(&output, &rust_bytes)?;
            }
            if exe {
                // `output` holds the module; replace it with the executable
                let wasm = fs::read(&output)?;
                let native = match target.as_deref() {
                    Some("native") if !aipl_core::native::supported() => {
                        return Err("--target native: the native backend targets Linux x86-64 only (use --target wasm)".into())
                    }
                    Some("native") => true,
                    Some(_) => false,
                    None => aipl_core::native::supported(),
                };
                if native {
                    let exe_bytes = aipl_core::native::executable(&wasm, sandbox)?;
                    fs::write(&output, &exe_bytes)?;
                    // WASI cannot set the execute bit, so the host does (LANGUAGE_GAPS.md 1)
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        fs::set_permissions(&output, fs::Permissions::from_mode(0o755))?;
                    }
                } else {
                    // launcher + module + trailer
                    write_executable(&output, &wasm, sandbox)?;
                }
            }
            println!("[AIPL Compiler] Successfully compiled '{}' -> '{}' ({} bytes)", file, output, fs::metadata(&output)?.len());
        }
        Commands::Run { file, sandbox, args } => {
            let mut cmd = std::process::Command::new(runner_path()?);
            if sandbox {
                cmd.arg("--sandbox");
            }
            let status = cmd.arg(&file).args(&args).status()?;
            std::process::exit(status.code().unwrap_or(1));
        }
        Commands::Verify { file } => {
            let module = Resolver::resolve(Path::new(&file))?;
            let mut checker = TypeChecker::new();
            checker.check_module(&module)?;
            println!("[AIPL Verifier] OK: module '{}' type-checks. Contracts are type-checked, not proven; req/ens run on every call, in the VM and in compiled code.", module.name);
        }
        Commands::Test { file, func } => {
            let module = Resolver::resolve(Path::new(&file))?;
            let mut checker = TypeChecker::new();
            checker.check_module(&module)?;

            let mut vm = VM::new();
            vm.load_module(module);
            println!("[AIPL Test] Running '{}' from '{}'...\n", func, file);
            match vm.invoke(&func, vec![]) {
                Ok(Value::Int(0)) => {
                    println!("\n[AIPL Test] All groups passed.");
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
                    eprintln!("\n[AIPL Test] Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Serve { addr } => {
            AgentServer::start(&addr)?;
        }
    }

    Ok(())
}
