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
