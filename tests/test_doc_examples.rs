//! Every ```lisp block in PROMPT_GUIDE_FOR_AIS.md and README.md is a complete
//! module whose `main` must return the value the document states, in the VM
//! and compiled under wasmtime (with WASI and a scratch directory, since some
//! examples do I/O). AIPL_SPEC.md's blocks are checked too: complete modules
//! run like the others (with the exceptions in SPEC_SPECIAL), and fragments
//! (a few definitions) are wrapped in a module and type-checked, the one
//! deliberately invalid fragment failing with its documented message. A doc
//! example that stops working fails here instead of misleading the next agent
//! that copies it.

use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::resolver::Resolver;
use aipl_core::vm::{Value, VM};
use std::path::Path;
use wasmtime::{Engine, Linker, Module as WasmModule, Store, TypedFunc};
use wasmtime_wasi::p1::WasiP1Ctx;
use wasmtime_wasi::{FsPerms, WasiCtxBuilder};

/// Module name -> value `main` returns, as stated in the docs. Modules without
/// a `main` (library-style examples) are checked and compiled only.
const EXPECTED: &[(&str, i32)] = &[
    ("search", 49),
    ("list_demo", 55),
    ("parse_demo", 1233),
    ("io_demo", 15),
    ("std_demo", 32),
    ("generics_demo", 17),
    ("expr_demo", 80),
    // AIPL_SPEC.md
    ("points", 33),
    ("gcd_demo", 21),
    ("stack_demo", 42),
    ("hello", 11),
    ("file_demo", 1),
    ("word_count", -1), // no input.txt in the scratch directory
    ("shapes", 28),
    ("geom", 15),
    ("tokens_demo", 110),
    // docs/TOUR.md
    ("tour_hello", 25),
    ("tour_types", 43),
    ("tour_control", 29),
    ("tour_structs", 33),
    ("tour_results", 79),
    ("tour_contracts", 19),
    ("tour_generics", 13),
];

/// README.md and PROMPT_GUIDE_FOR_AIS.md examples whose `main` must fail, as
/// the document shows it failing: the VM's message contains the text, and
/// compiled code traps.
const FAILING: &[(&str, &str)] = &[("gcd", "Pre-condition failed in 'gcd' at 6:10: (req (and (gt a 0) (gt b 0))) with a = 10, b = 0")];

/// AIPL_SPEC.md modules that cannot simply run in both backends, and why.
const SPEC_SPECIAL: &[(&str, &str)] = &[
    ("contracts_demo", "main fails its precondition in the VM (the example's point); wasm has no contracts and traps"),
    ("counter_demo", "a threaded module: needs a wasi-threads host, covered by tests/test_threads.rs; runs in the VM"),
    ("test_suite", "abridged; imports toolchain modules and concatenates strings (VM-only)"),
    ("util", "two files in one block (util.aipl and main.aipl)"),
];

/// AIPL_SPEC.md fragments that must fail to check, with the documented message.
const SPEC_INVALID: &[(&str, &str)] = &[("(fn bad ", "If branch type mismatch: then is void, else is i32")];

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

/// Whether the module's main traps when compiled.
fn wasm_traps(wasm: &[u8], dir: &Path) -> bool {
    let engine = Engine::default();
    let module = WasmModule::new(&engine, wasm).unwrap();
    let mut linker: Linker<WasiP1Ctx> = Linker::new(&engine);
    wasmtime_wasi::p1::add_to_linker_sync(&mut linker, |t: &mut WasiP1Ctx| t).unwrap();
    let ctx = WasiCtxBuilder::new().preopened_dir(dir, ".", FsPerms::ReadWrite).unwrap().build_p1();
    let mut store = Store::new(&engine, ctx);
    let instance = linker.instantiate(&mut store, &module).unwrap();
    let main: TypedFunc<(), i32> = instance.get_typed_func(&mut store, "main").unwrap();
    main.call(&mut store, ()).is_err()
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
    for doc in ["PROMPT_GUIDE_FOR_AIS.md", "README.md", "docs/TOUR.md"] {
        for src in lisp_blocks(doc) {
            // Resolved from a file, so examples may import the standard library.
            let file = scratch.join("example.aipl");
            std::fs::write(&file, &src).unwrap();
            let module = Resolver::resolve(&file).unwrap_or_else(|e| panic!("{doc}: {e}\n{src}"));
            TypeChecker::new().check_module(&module).unwrap_or_else(|e| panic!("{doc}: {e}\n{src}"));
            let wasm = WasmCompiler::compile(&module).unwrap_or_else(|e| panic!("{doc}: {e}\n{src}"));
            if let Some(&(_, msg)) = FAILING.iter().find(|(n, _)| *n == module.name) {
                let mut vm = VM::new();
                vm.load_module(module.clone());
                let e = vm.invoke("main", vec![]).unwrap_err();
                assert!(e.contains(msg), "{doc}: module {}: expected `{msg}`, got `{e}`", module.name);
                assert!(wasm_traps(&wasm, &scratch), "{doc}: module {} must trap when compiled", module.name);
                seen.push(module.name.clone());
                continue;
            }
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
    let mut fragments = 0;
    for src in lisp_blocks("AIPL_SPEC.md") {
        let code: String = src.lines().filter(|l| !l.trim_start().starts_with(";;")).collect::<Vec<_>>().join("\n");
        let file = scratch.join("example.aipl");
        if !code.trim_start().starts_with("(module") {
            // memory.aipl's runner calls functions defined elsewhere in that module
            if code.contains("aipl_heap_alloc") {
                continue;
            }
            std::fs::write(&file, format!("(module fragment\n{code}\n)")).unwrap();
            let checked = Resolver::resolve(&file).and_then(|m| TypeChecker::new().check_module(&m).map(|_| m));
            match SPEC_INVALID.iter().find(|(head, _)| code.trim_start().starts_with(head)) {
                Some((_, msg)) => {
                    let e = checked.err().unwrap_or_else(|| panic!("AIPL_SPEC.md: this fragment is documented as invalid but checks:\n{src}"));
                    assert!(e.contains(msg), "AIPL_SPEC.md: expected `{msg}`, got `{e}`");
                }
                None => {
                    checked.unwrap_or_else(|e| panic!("AIPL_SPEC.md fragment: {e}\n{src}"));
                }
            }
            fragments += 1;
            continue;
        }
        let name = code.trim_start()["(module".len()..].split_whitespace().next().unwrap().to_string();
        if let Some((_, why)) = SPEC_SPECIAL.iter().find(|(n, _)| *n == name) {
            match name.as_str() {
                "contracts_demo" => {
                    std::fs::write(&file, &code).unwrap();
                    let module = Resolver::resolve(&file).unwrap();
                    TypeChecker::new().check_module(&module).unwrap();
                    let mut vm = VM::new();
                    vm.load_module(module);
                    let e = vm.invoke("main", vec![]).unwrap_err();
                    assert!(e.starts_with("Pre-condition failed in 'safe_div' at 3:10: (req (neq den 0)) with num = 10, den = 0"), "{e}");
                }
                "counter_demo" => {
                    std::fs::write(&file, &code).unwrap();
                    let module = Resolver::resolve(&file).unwrap();
                    TypeChecker::new().check_module(&module).unwrap();
                    WasmCompiler::compile(&module).unwrap();
                    let mut vm = VM::new();
                    vm.load_module(module);
                    assert_eq!(vm.invoke("main", vec![]).unwrap(), Value::Int(2000));
                }
                _ => eprintln!("AIPL_SPEC.md: not run: {name}: {why}"),
            }
            seen.push(name);
            continue;
        }
        std::fs::write(&file, &code).unwrap();
        let module = Resolver::resolve(&file).unwrap_or_else(|e| panic!("AIPL_SPEC.md: {e}\n{src}"));
        TypeChecker::new().check_module(&module).unwrap_or_else(|e| panic!("AIPL_SPEC.md: {e}\n{src}"));
        let wasm = WasmCompiler::compile(&module).unwrap_or_else(|e| panic!("AIPL_SPEC.md: {e}\n{src}"));
        if let Some(&(_, want)) = EXPECTED.iter().find(|(n, _)| *n == module.name) {
            let mut vm = VM::new();
            vm.load_module(module.clone());
            assert_eq!(vm.invoke("main", vec![]).unwrap(), Value::Int(want as i64), "AIPL_SPEC.md: VM, module {}", module.name);
            assert_eq!(run_wasm(&wasm, &scratch), want, "AIPL_SPEC.md: wasm, module {}", module.name);
        } else {
            assert!(!module.functions.iter().any(|f| f.name == "main"), "AIPL_SPEC.md: add '{}' to EXPECTED", module.name);
        }
        seen.push(module.name.clone());
    }
    assert!(fragments >= 10, "expected the spec's fragments to be checked, got {fragments}");
    for (name, _) in SPEC_SPECIAL {
        assert!(seen.iter().any(|s| s == name), "AIPL_SPEC.md example '{name}' not found");
    }

    std::env::set_current_dir(prev).unwrap();
    let _ = std::fs::remove_dir_all(&scratch);
    for name in EXPECTED.iter().map(|(n, _)| n).chain(FAILING.iter().map(|(n, _)| n)) {
        assert!(seen.iter().any(|s| s == name), "documented example '{name}' not found in the docs");
    }
}
