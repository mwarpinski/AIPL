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

/// What the checker accepts must compile (docs/gemini-audit.md found the
/// first four holes; a sweep of every op over 0-3 operands of each kind
/// found the rest). Each rejected program, with the message it gets.
#[test]
fn ops_the_compilers_cannot_lower_are_rejected_by_the_checker() {
    let cases: &[(&str, &str)] = &[
        // atomics: operand count and types
        ("(atomic.add)", "atomic.add takes 2 operands, (atomic.add p v); got 0"),
        ("(atomic.add (mem.alloc 4) 1 2)", "atomic.add takes 2 operands, (atomic.add p v); got 3"),
        ("(atomic.add (mem.alloc 4) 1i64)", "atomic.add operand 2 must be I32, got I64"),
        ("(if (atomic.cas (mem.alloc 4) 0) 1 0)", "atomic.cas takes 3 operands, (atomic.cas p expected new); got 2"),
        ("(if (atomic.cas (mem.alloc 4) 0 true) 1 0)", "atomic.cas operand 3 must be I32, got Bool"),
        ("(atomic.lock) 0", "atomic.lock takes 1 operands, (atomic.lock p); got 0"),
        ("(atomic.unlock \"m\") 0", "atomic.unlock operand 1 must be I32, got Str"),
        // threads
        ("(thread.join \"x\")", "thread.join needs the i32 handle thread.spawn returned, got Str"),
        // printing
        ("(sys.print 1) 0", "sys.print prints str values, got I32; for numbers use io.print_int"),
        ("(sys.print \"a\" 2.5) 0", "sys.print prints str values, got F64"),
        // arithmetic on things that are not numbers
        ("(str.len (+ \"a\" \"b\"))", "+ does not join strings; build them with std/buf"),
        ("(if (+ true false) 1 0)", "Add needs numbers (i32, i64, f32, f64), got Bool"),
        ("(i32.wrap (i64.trunc_f64_s (% 1.5 1.0)))", "Mod is integer arithmetic (i32 or i64), got F64"),
        ("(if (bitand true true) 1 0)", "BitAnd is integer arithmetic (i32 or i64), got Bool"),
        ("(i32.wrap (i64.trunc_f64_s (shl 1.5 1.5)))", "Shl is integer arithmetic (i32 or i64), got F64"),
        // ordering things that are not numbers
        ("(if (lt \"a\" \"b\") 1 0)", "Lt on Str: only numbers are ordered; bool and str compare only with eq/neq"),
        ("(if (gte true false) 1 0)", "Gte on Bool: only numbers are ordered"),
        // ops that no longer exist
        ("(mem.free (mem.alloc 8)) 0", "there is no mem.free: memory from mem.alloc is never freed. Free single objects with std/heap"),
        ("(i32.wrap (i64.trunc_f64_s (mem.load_f64 (mem.alloc 8))))", "there is no mem.load_f64: keep floats in struct fields or (arr f64)"),
        ("(mem.store_f32 (mem.alloc 8) 1.5) 0", "there is no mem.store_f32"),
    ];
    for (body, want) in cases {
        let src = format!("(module m (fn main [] -> i32 {body}))");
        let err = match Parser::parse(&src) {
            Err(e) => e,
            Ok(m) => TypeChecker::new().check_module(&m).err().unwrap_or_else(|| panic!("accepted: {src}")),
        };
        assert!(err.contains(want), "{src}\n  expected: {want}\n  got: {err}");
    }
    // still accepted: eq/neq on bool and str, + - * / and ordering on floats
    check_ok("(module m (fn main [] -> i32 (if (and (eq \"a\" \"a\") (and (neq true false) (lt 1.5 (* 2.0 (- 3.0 (/ 1.0 2.0)))))) 1 0)))");
}
