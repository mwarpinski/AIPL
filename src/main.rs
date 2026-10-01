use aipl_core::agent_api::server::AgentServer;
use aipl_core::checker::TypeChecker;
use aipl_core::compiler::binary_ast::BinaryAstCompiler;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::parser::Parser;
use aipl_core::resolver::Resolver;
use aipl_core::vm::{Value, VM};
use clap::{Parser as ClapParser, Subcommand};
use std::fs;
use std::path::Path;

#[derive(ClapParser)]
#[command(name = "aipl")]
#[command(about = "AI Programming Language (AIPL) - Machine-native, token-efficient, formally verifiable programming language and self-hosting Wasm compiler.")]
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
    },
    /// Compile an AIPL source file directly into a WebAssembly (.wasm) binary module
    Compile {
        file: String,
        #[arg(short, long, default_value = "out.wasm")]
        output: String,
        /// Compile using the self-hosted codegen.aipl backend and verify bit-for-bit parity with Rust compiler
        #[arg(long = "self")]
        self_flag: bool,
    },
    /// Type-check and formally verify an AIPL file without running it
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
    /// Encode AIPL text S-expression into a compact Binary AST payload (.baipl)
    BinaryEncode {
        file: String,
        #[arg(short, long, default_value = "out.baipl")]
        output: String,
    },
    /// Decode a compact Binary AST payload (.baipl) back to S-expression text
    BinaryDecode { file: String },
    /// Launch the Agent Swarm RPC server for inter-agent remote execution
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
fn describe_divergence(rust: &[u8], selfh: &[u8]) -> String {
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Eval { file, func } => {
            let module = Resolver::resolve(Path::new(&file))?;
            let mut checker = TypeChecker::new();
            checker.check_module(&module)?;

            let mut vm = VM::new();
            vm.load_module(module);
            println!("[AIPL VM] Executing function '{}' from '{}'...", func, file);
            let res = vm.invoke(&func, vec![])?;
            println!("[AIPL Result]: {:?}", res);
        }
        Commands::Compile { file, output, self_flag } => {
            let module = Resolver::resolve(Path::new(&file))?;
            let mut checker = TypeChecker::new();
            checker.check_module(&module)?;

            let rust_bytes = WasmCompiler::compile(&module)?;
            if self_flag {
                println!("[AIPL Self-Host] Compiling '{}' via self-hosted codegen.aipl...", file);
                // The self-hosted compiler takes one import-free module, so it is
                // given the resolved program printed back as source.
                let flat_src = aipl_core::printer::print_module(&module);
                let self_bytes = run_self_hosted_codegen(&flat_src).map_err(|e| format!("Self-host error: {}", e))?;
                if rust_bytes != self_bytes {
                    eprintln!("[AIPL Self-Host ERROR] Mismatch between Rust backend and self-hosted codegen!");
                    eprintln!("{}", describe_divergence(&rust_bytes, &self_bytes));
                    return Err("Byte-parity mismatch between Rust backend and self-hosted codegen!".into());
                }
                println!("[AIPL Self-Host] SUCCESS: Self-hosted codegen produced 100% BIT-FOR-BIT IDENTICAL WebAssembly!");
                fs::write(&output, &self_bytes)?;
            } else {
                fs::write(&output, &rust_bytes)?;
            }
            println!("[AIPL Compiler] Successfully compiled '{}' -> '{}' ({} bytes)", file, output, fs::metadata(&output)?.len());
        }
        Commands::Verify { file } => {
            let module = Resolver::resolve(Path::new(&file))?;
            let mut checker = TypeChecker::new();
            checker.check_module(&module)?;
            println!("[AIPL Verifier] SUCCESS: Module '{}' is 100% type-safe and contracts verified!", module.name);
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
        Commands::BinaryEncode { file, output } => {
            let src = fs::read_to_string(&file)?;
            let module = Parser::parse(&src)?;
            let bytes = BinaryAstCompiler::encode(&module)?;
            fs::write(&output, bytes)?;
            println!("[AIPL Binary Encoder] Encoded '{}' -> '{}' ({} bytes)", file, output, fs::metadata(&output)?.len());
        }
        Commands::BinaryDecode { file } => {
            let bytes = fs::read(&file)?;
            let module = BinaryAstCompiler::decode(&bytes)?;
            println!("[AIPL Binary Decoder] Decoded module '{}' with {} functions:", module.name, module.functions.len());
            for f in &module.functions {
                println!("  - (fn {} ...)", f.name);
            }
        }
        Commands::Serve { addr } => {
            AgentServer::start(&addr)?;
        }
    }

    Ok(())
}
