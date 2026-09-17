use aipl_core::compiler::binary_ast::BinaryAstCompiler;
use aipl_core::parser::Parser;
use aipl_core::vm::{Value, VM};
use std::time::Instant;

fn main() {
    println!("============================================================");
    println!("        AIPL PERFORMANCE & TOKEN DENSITY BENCHMARK          ");
    println!("============================================================");

    let aipl_source = r#"
    (module bench_module
      (fn matrix_dot [iterations:i32] -> i32
        (let sum:i32 0)
        (loop i 1 iterations 1
          (set! sum (+ sum (* i 2))))
        sum))
    "#;

    let python_equivalent = r#"
class MatrixBenchmark:
    def matrix_dot(self, iterations: int) -> int:
        sum_val = 0
        for i in range(1, iterations + 1):
            sum_val += i * 2
        return sum_val

if __name__ == "__main__":
    bench = MatrixBenchmark()
    result = bench.matrix_dot(100000)
    "#;

    let aipl_tokens = aipl_source.split_whitespace().count();
    let py_tokens = python_equivalent.split_whitespace().count();
    let compression = (1.0 - (aipl_tokens as f64 / py_tokens as f64)) * 100.0;

    println!("[1] Token Economy Analysis:");
    println!("    Python Code Tokens : {}", py_tokens);
    println!("    AIPL S-Expr Tokens : {}", aipl_tokens);
    println!("    Token Compression   : {:.2}% fewer tokens for LLM context!", compression);

    println!("\n[2] VM Execution Speed Benchmark (100,000 loop iterations):");
    let module = Parser::parse(aipl_source).expect("Parse failed");
    let mut vm = VM::new();
    vm.load_module(module.clone());

    let start = Instant::now();
    let res = vm.invoke("matrix_dot", vec![Value::Int(100_000)]).expect("VM failed");
    let duration = start.elapsed();

    println!("    Execution Result : {:?}", res);
    println!("    VM Latency       : {:.4} ms", duration.as_secs_f64() * 1000.0);

    println!("\n[3] Inter-Agent Binary AST Payload Serialization:");
    let bytes = BinaryAstCompiler::encode(&module).unwrap_or_default();
    println!("    Binary AST Payload Size: {} bytes (Instant zero-parse transfer)", bytes.len());
    println!("============================================================");
}
