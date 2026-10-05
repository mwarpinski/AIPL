# Benchmarks

Programs in `benchmarks/<name>/`, each written four ways with the same
algorithm: AIPL (`<name>.aipl`), C (`<name>.c`), and Python (`<name>.py`);
AIPL is built natively (`aipl compile --exe`, Linux x86-64) and as wasm
under the launcher (`--target wasm`, wasmtime's Cranelift). They serve two
purposes: measuring speed, and finding what the language is missing (each
program's gaps are listed below, with what was added for it).

    cargo build --release
    python3 tools/bench.py            # every benchmark, at its benchmark size
    python3 tools/bench.py spigot     # one

`tools/bench.py` builds every version, checks they all print the same
output, and reports the best of three wall-clock runs (process start
included) with each time relative to C. `tests/test_benchmarks.rs` runs each
AIPL program, wasm and native, at its small test size against
`expected.txt` (produced by the C version), so the benchmarks are also
correctness tests. AIPL programs time their own work with `std/time` and
print it on stderr.

## Results

AMD Ryzen 7 H 255, Linux 6.18, gcc 16.2 -O2, Python 3.14, release build,
2026-10-04.

| Benchmark | Input | AIPL native | AIPL wasm (wasmtime) | C (gcc -O2) | Python 3 |
|---|---|---|---|---|---|
| spigot | 10000 digits | 4.360 s (4.7x) | 1.573 s (1.7x) | 0.922 s (1.0x) | 47.345 s (51.3x) |
| fannkuch | n = 10 | 1.358 s (8.8x) | 0.212 s (1.4x) | 0.155 s (1.0x) | 3.745 s (24.2x) |
| spectralnorm | n = 1000 | 0.395 s (14.6x) | 0.121 s (4.5x) | 0.027 s (1.0x) | 7.373 s (272.0x) |
| nbody | 1,000,000 steps | 0.794 s (15.2x) | 0.092 s (1.8x) | 0.052 s (1.0x) | 5.829 s (111.8x) |

## Notes per benchmark

**spigot** (pi by the Rabinowitz-Wagon spigot: integers and one array, no
bignums; output in the Benchmarks Game's pidigits format). Ran with no
language changes. What it showed:
- AIPL needed `std/time` to time its own work, and could not print an
  `i64` at all: added `fmt.int64_to_bytes`/`uint64_to_bytes`,
  `buf.push_i64`, `io.print_i64`/`println_i64`, and `std/time` (`now`,
  `since`, `report`, `push_duration`).
- Native code is 2.8 times slower than wasmtime's here (it was 10% slower
  compiling the compiler): the inner loop is one 64-bit division and two
  array accesses per step, where the baseline translator's design costs
  most (every value passes through the stack in memory; every access
  re-reads the memory size for its bounds check). The first optimisation
  target, if native speed matters: keep the top of the value stack in
  registers, and the memory size in a register for single-threaded code.

**fannkuch** (fannkuch-redux, single-threaded: permutations of 0..n-1, each
flipped until 0 comes first). Ran with no language changes. What it showed:
array-heavy code is where the native translator trails wasmtime most (6.4
times): each `arr.get`/`arr.set` is several wasm instructions, every one
passing its values through the stack in memory, plus a bounds check that
reloads the memory size. wasmtime keeps them in registers. The same
optimisations as for spigot (value stack in registers, memory size in a
register) would matter most here.

**spectralnorm** (the power method on A'A, printed to nine decimals). Needed
two additions, both now in the language and standard library:
- `(f64.sqrt x)`: a new operation, correctly rounded (wasm `f64.sqrt`,
  SSE2 `sqrtsd` natively, Rust's `sqrt` in the VM), so results match C to
  the bit. A library square root (Newton's method) could differ in the last
  bit and break the output comparison.
- Printing floats: `fmt.f64_fixed` (and `buf.push_f64`, `io.print_f64`,
  `io.println_f64`) writes the exact decimal value rounded half to even, as
  C's `printf("%.9f")` and Python's format do, using a small
  arbitrary-precision integer; checked against Rust's exact formatting on
  20,000 random doubles. Shortest round-trip printing (`printf("%g")`
  style) and parsing floats from text remain gaps.
C gains most here: gcc inlines the matrix-entry function and vectorises the
inner loop, while AIPL calls a function per entry.

**nbody** (the Sun and the four gas giants, energy printed to nine decimals
before and after). Ran with the spectralnorm additions (`f64.sqrt`,
`fmt.f64_fixed`). AIPL float literals have no exponent notation, so the
benchmark's constants are written out in decimal (the same doubles). All
four versions print the published values, bit-identical float arithmetic
in the same operation order. Float code is where the native translator
trails most (8.6 times wasmtime): every operation moves values from the
stack in memory into xmm registers and back, where Cranelift keeps them in
registers.
