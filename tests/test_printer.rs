//! The printer turns a resolved module back into source for the self-hosted
//! compiler, so the round trip must be exact: for every AIPL file in the
//! repository, the printed program re-parses, prints identically, and compiles
//! to the same wasm bytes as the original.

use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::parser::Parser;
use aipl_core::printer::print_module;
use aipl_core::resolver::Resolver;
use std::path::Path;

#[test]
fn every_repository_program_round_trips_through_the_printer() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for dir in ["aipl_src", "aipl_src/std", "examples"] {
        let Ok(entries) = std::fs::read_dir(root.join(dir)) else { continue };
        for e in entries {
            let p = e.unwrap().path();
            if p.extension().is_some_and(|x| x == "aipl") && !p.ends_with("hello_browser.aipl") {
                files.push(p);
            }
        }
    }
    files.sort();
    assert!(files.len() >= 12, "expected the repository's programs, found {}", files.len());
    for path in &files {
        let module = Resolver::resolve(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let printed = print_module(&module);
        let reparsed = Parser::parse(&printed).unwrap_or_else(|e| panic!("{}: printed source does not parse: {e}", path.display()));
        assert_eq!(print_module(&reparsed), printed, "{}: printing is not stable", path.display());
        TypeChecker::new().check_module(&reparsed).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        if let Ok(original) = WasmCompiler::compile(&module) {
            assert_eq!(WasmCompiler::compile(&reparsed).unwrap(), original, "{}: different wasm", path.display());
        }
    }
}

#[test]
fn literals_print_in_forms_both_tokenizers_read() {
    let src = r#"(module lits (fn f [] -> f64 (let a:i64 -9223372036854775808i64) (let s:str "q\"\\\n\t") (let z:f64 -0.0) (let t:f64 7.0) 0.0000000000000000000001))"#;
    let printed = print_module(&Parser::parse(src).unwrap());
    assert!(printed.contains("-9223372036854775808i64"), "{printed}");
    assert!(printed.contains(r#""q\"\\\n\t""#), "{printed}");
    assert!(printed.contains("-0.0") && printed.contains("7.0"), "{printed}");
    assert!(printed.contains("0.0000000000000000000001") && !printed.contains("1e-"), "{printed}");
}
