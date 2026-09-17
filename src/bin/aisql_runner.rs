use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::parser::Parser;
use aipl_core::vm::VM;
use std::env;
use std::fs;

const AISQL_ENGINE_SRC: &str = include_str!("../../examples/aipl_database/aisql_engine.aipl");
const AISQL_DEMO_SRC: &str = include_str!("../../examples/aipl_database/demo_aisql.aipl");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let command = if args.len() > 1 { &args[1] } else { "demo" };

    // 1. Parse & Type-Check AISQL Database Engine
    let engine_module = Parser::parse(AISQL_ENGINE_SRC)?;
    let mut checker = TypeChecker::new();
    checker.check_module(&engine_module)?;

    match &command[..] {
        "demo" => {
            println!("\x1b[48;5;39m\x1b[30m\x1b[1m  AISQL / AIPL-DB: AI-Native Vector & Relational Database System  \x1b[0m");
            let demo_module = Parser::parse(AISQL_DEMO_SRC)?;
            checker.check_module(&demo_module)?;

            let mut vm = VM::new();
            vm.load_module(demo_module);
            println!("\x1b[32m[AISQL Engine]: Running Relational + AI Vector Similarity Suite...\x1b[0m");
            let res = vm.invoke("run_aisql_suite", vec![])?;
            println!("\x1b[36m[AISQL Query Output]: Vector Similarity Dot Product Score -> {:?}\x1b[0m", res);
            println!("\x1b[32m[AISQL Success]: Table schema, index hash lookup, and AI RAG vector search executed cleanly!\x1b[0m");
        }
        "compile" => {
            let out_file = if args.len() > 2 { &args[2] } else { "aisql_engine.wasm" };
            let wasm_bytes = WasmCompiler::compile(&engine_module)?;
            fs::write(out_file, wasm_bytes)?;
            println!("[AISQL Compiler] Compiled AISQL Engine -> '{}' ({} bytes)", out_file, fs::metadata(out_file)?.len());
        }
        "inspect" => {
            println!("\x1b[34m[AISQL S-Expression Engine Bytecode Inspection]\x1b[0m");
            println!("{}", AISQL_ENGINE_SRC);
        }
        _ => {
            println!("\x1b[1mAISQL Database System Options:\x1b[0m");
            println!("  aisql demo                 Execute Relational + AI Vector Embedding Query Suite");
            println!("  aisql compile <out.wasm>   Compile AISQL Database Engine to WebAssembly");
            println!("  aisql inspect              Inspect raw AISQL database engine S-expressions");
        }
    }

    Ok(())
}
