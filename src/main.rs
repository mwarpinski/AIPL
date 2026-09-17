use aipl_core::agent_api::server::AgentServer;
use aipl_core::checker::TypeChecker;
use aipl_core::compiler::binary_ast::BinaryAstCompiler;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::parser::Parser;
use aipl_core::resolver::Resolver;
use aipl_core::vm::VM;
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
    },
    /// Type-check and formally verify an AIPL file without running it
    Verify { file: String },
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
        Commands::Compile { file, output } => {
            let module = Resolver::resolve(Path::new(&file))?;
            let mut checker = TypeChecker::new();
            checker.check_module(&module)?;

            let wasm_bytes = WasmCompiler::compile(&module)?;
            fs::write(&output, wasm_bytes)?;
            println!("[AIPL Compiler] Successfully compiled '{}' -> '{}' ({} bytes)", file, output, fs::metadata(&output)?.len());
        }
        Commands::Verify { file } => {
            let module = Resolver::resolve(Path::new(&file))?;
            let mut checker = TypeChecker::new();
            checker.check_module(&module)?;
            println!("[AIPL Verifier] SUCCESS: Module '{}' is 100% type-safe and contracts verified!", module.name);
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
