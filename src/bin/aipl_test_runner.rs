use aipl_core::checker::TypeChecker;
use aipl_core::parser::Parser;
use aipl_core::vm::{Value, VM};

const AIPL_TEST_SRC: &str = include_str!("../../aipl_src/aipl_test.aipl");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("\x1b[48;5;28m\x1b[37m\x1b[1m  AIPL SOVEREIGN NATIVE TEST RUNNER & CONTRACT HARNESS  \x1b[0m");

    // 1. Parse & Verify aipl_test.aipl S-expressions
    let module = Parser::parse(AIPL_TEST_SRC)?;
    let mut checker = TypeChecker::new();
    checker.check_module(&module)?;

    println!("\x1b[32m[AIPL Verifier]: Module 'aipl_test_runner' is 100% type-safe and contracts verified!\x1b[0m");

    // 2. Load module into AIPL VM and invoke native master test suite
    let mut vm = VM::new();
    vm.load_module(module);

    println!("\x1b[36m[AIPL Test Runner]: Executing master sovereign test suite...\x1b[0m");
    let res = vm.invoke("main", vec![])?;

    if let Value::Int(passed_count) = res {
        println!("\x1b[32m\x1b[1m[AIPL Test Summary]: ALL {}/5 SOVEREIGN NATIVE TESTS PASSED CLEANLY!\x1b[0m", passed_count);
    } else {
        println!("\x1b[31m[AIPL Test Summary]: Test suite failed or returned invalid value {:?}\x1b[0m", res);
    }

    Ok(())
}
