//! Every ```lisp block in PROMPT_GUIDE_FOR_AIS.md and README.md is a complete
//! module whose `main` must return the value the document states, in the VM
//! and compiled under wasmtime (with WASI and a scratch directory, since some
//! examples do I/O). A doc example that stops working fails here instead of
//! misleading the next agent that copies it.

use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::parser::Parser;
use aipl_core::vm::{Value, VM};
use std::path::Path;
use wasmtime::{Engine, Linker, Module as WasmModule, Store, TypedFunc};
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::{FsPerms, WasiCtxBuilder};

/// Module name -> value `main` returns, as stated in the docs. Modules without
/// a `main` (library-style examples) are checked and compiled only.
const EXPECTED: &[(&str, i32)] = &[
    ("demo", 21),
    ("search", 49),
    ("list_demo", 55),
    ("parse_demo", 1233),
    ("io_demo", 15),
];

fn lisp_blocks(doc: &str) -> Vec<String> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(doc)).unwrap();
    text.split("```lisp\n").skip(1).map(|b| b.split("```").next().unwrap().to_string()).collect()
}

fn run_wasm(wasm: &[u8], dir: &Path) -> i32 {
    let engine = Engine::default();
    let module = WasmModule::new(&engine, wasm).unwrap();
    let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).unwrap();
    let ctx = WasiCtxBuilder::new().preopened_dir(dir, ".", FsPerms::ReadWrite).unwrap().build_p1();
    let mut store = Store::new(&engine, ctx);
    let instance = linker.instantiate(&mut store, &module).unwrap();
    let main: TypedFunc<(), i32> = instance.get_typed_func(&mut store, "main").unwrap();
    main.call(&mut store, ()).unwrap()
}

#[test]
fn doc_examples_run_in_both_backends() {
    let scratch = std::env::temp_dir().join(format!("aipl_doc_examples_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    // The VM resolves file paths against the cwd; this test binary has one test, so chdir is safe.
    let prev = std::env::current_dir().unwrap();
    std::env::set_current_dir(&scratch).unwrap();

    let mut seen = Vec::new();
    for doc in ["PROMPT_GUIDE_FOR_AIS.md", "README.md"] {
        for src in lisp_blocks(doc) {
            let module = Parser::parse(&src).unwrap_or_else(|e| panic!("{doc}: {e}\n{src}"));
            TypeChecker::new().check_module(&module).unwrap_or_else(|e| panic!("{doc}: {e}\n{src}"));
            let wasm = WasmCompiler::compile(&module).unwrap_or_else(|e| panic!("{doc}: {e}\n{src}"));
            let Some(&(_, want)) = EXPECTED.iter().find(|(n, _)| *n == module.name) else {
                assert!(!module.functions.iter().any(|f| f.name == "main"), "{doc}: add '{}' to EXPECTED", module.name);
                continue;
            };
            let mut vm = VM::new();
            vm.load_module(module.clone());
            assert_eq!(vm.invoke("main", vec![]).unwrap(), Value::Int(want as i64), "{doc}: VM, module {}", module.name);
            assert_eq!(run_wasm(&wasm, &scratch), want, "{doc}: wasm, module {}", module.name);
            seen.push(module.name.clone());
        }
    }
    std::env::set_current_dir(prev).unwrap();
    let _ = std::fs::remove_dir_all(&scratch);
    for (name, _) in EXPECTED {
        assert!(seen.iter().any(|s| s == name), "documented example '{name}' not found in the docs");
    }
}
