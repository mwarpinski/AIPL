use aipl_core::checker::TypeChecker;
use aipl_core::parser::Parser;

#[test]
fn test_diagnostic_missing_closing_paren() {
    let src = "(module test_mod\n  (fn test_op [] -> i32 42)";
    let err = Parser::parse(src).expect_err("Expected parse error for missing ')'");
    assert!(
        err.starts_with("2:27:"),
        "Expected error starting with '2:27:', got '{}'",
        err
    );
}

#[test]
fn test_diagnostic_unknown_op() {
    let src = "(module test_mod\n  (fn test_op [] -> i32\n    (badop 1 2)))";
    let err = Parser::parse(src).expect_err("Expected parse error for unknown op");
    assert!(
        err.starts_with("3:5:"),
        "Expected error starting with '3:5:', got '{}'",
        err
    );
}

#[test]
fn test_diagnostic_type_mismatch_in_let() {
    let src = "(module test_mod\n  (fn test_op [] -> i32\n    (let x:i32 true)\n    0))";
    let module = Parser::parse(src).expect("Parse failed");
    let mut checker = TypeChecker::new();
    let err = checker
        .check_module(&module)
        .expect_err("Expected type mismatch in let");
    assert!(
        err.starts_with("3:5:"),
        "Expected error starting with '3:5:', got '{}'",
        err
    );
}

#[test]
fn test_diagnostic_undefined_variable() {
    let src = "(module test_mod\n  (fn test_op [] -> i32\n    undefined_var))";
    let module = Parser::parse(src).expect("Parse failed");
    let mut checker = TypeChecker::new();
    let err = checker
        .check_module(&module)
        .expect_err("Expected undefined variable error");
    assert!(
        err.starts_with("3:5:"),
        "Expected error starting with '3:5:', got '{}'",
        err
    );
}

#[test]
fn test_diagnostic_extra_paren_after_module() {
    let src = "(module test_mod\n  (fn test_op [] -> i32 0))\n)";
    let err = Parser::parse(src).expect_err("Expected parse error for extra ')' after module");
    assert!(
        err.starts_with("3:1:"),
        "Expected error starting with '3:1:', got '{}'",
        err
    );
    assert!(
        err.contains("unexpected tokens after module end"),
        "Expected message containing 'unexpected tokens after module end', got '{}'",
        err
    );
}

// ---------------------------------------------------------------------------
// Memory layout enforcement (AIPL_SPEC.md, "Memory layout"): literal addresses
// in the wrong part of the runtime block are rejected at check time, before
// either backend runs.
// ---------------------------------------------------------------------------

fn check_err(src: &str) -> String {
    let module = Parser::parse(src).expect("Parse failed");
    TypeChecker::new()
        .check_module(&module)
        .expect_err("Expected a checker error")
}

fn check_ok(src: &str) {
    let module = Parser::parse(src).expect("Parse failed");
    TypeChecker::new()
        .check_module(&module)
        .unwrap_or_else(|e| panic!("Expected program to check, got '{e}'"));
}

#[test]
fn test_diagnostic_lock_on_heap_cursor_is_rejected() {
    let err = check_err("(module m\n  (fn f [] -> void\n    (atomic.lock 0)))");
    assert!(err.starts_with("3:5:"), "got '{err}'");
    assert!(err.contains("heap cursor"), "got '{err}'");
}

#[test]
fn test_diagnostic_store_to_heap_cursor_is_rejected() {
    let err = check_err("(module m\n  (fn f [] -> void\n    (mem.store32 0 42)))");
    assert!(err.starts_with("3:5:"), "got '{err}'");
    assert!(err.contains("heap cursor"), "got '{err}'");
}

#[test]
fn test_diagnostic_store_into_reserved_block_is_rejected() {
    let err = check_err("(module m\n  (fn f [] -> void\n    (mem.store32 512 42)))");
    assert!(err.starts_with("3:5:"), "got '{err}'");
    assert!(err.contains("reserved runtime block"), "got '{err}'");
    let err = check_err("(module m\n  (fn f [] -> i32\n    (mem.load32 700)))");
    assert!(err.contains("reserved runtime block"), "got '{err}'");
}

#[test]
fn test_diagnostic_misaligned_runtime_cell_is_rejected() {
    let err = check_err("(module m\n  (fn f [] -> void\n    (mem.store32 18 1)))");
    assert!(err.contains("4-byte-aligned"), "got '{err}'");
}

#[test]
fn test_diagnostic_layout_allows_legitimate_runtime_access() {
    // Reading the heap cursor, and reading/writing the aligned runtime cells,
    // is exactly what memory.aipl and codegen.aipl do.
    check_ok("(module m (fn f [] -> i32 (mem.load32 0)))");
    check_ok("(module m (fn f [] -> void (mem.store32 4 0)))");
    check_ok("(module m (fn f [] -> i32 (mem.store32 16 (mem.alloc 256)) (mem.load32 16)))");
    // Heap addresses from mem.alloc are the normal case.
    check_ok("(module m (fn f [] -> void (let p:i32 (mem.alloc 4)) (atomic.lock p) (atomic.unlock p)))");
    // Literal heap addresses are allowed (discouraged, but not the checker's call).
    check_ok("(module m (fn f [] -> void (mem.store32 4096 1)))");
}
