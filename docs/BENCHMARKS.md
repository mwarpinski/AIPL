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
2026-10-05.

| Benchmark | Input | AIPL native | AIPL wasm (wasmtime) | C (gcc -O2) | Python 3 |
|---|---|---|---|---|---|
| spigot | 10000 digits | 2.068 s (2.3x) | 1.552 s (1.7x) | 0.914 s (1.0x) | 47.222 s (51.7x) |
| fannkuch | n = 10 | 0.719 s (4.7x) | 0.203 s (1.3x) | 0.153 s (1.0x) | 3.760 s (24.5x) |
| spectralnorm | n = 1000 | 0.363 s (13.5x) | 0.113 s (4.2x) | 0.027 s (1.0x) | 7.131 s (266.0x) |
| nbody | 1,000,000 steps | 0.517 s (10.0x) | 0.088 s (1.7x) | 0.052 s (1.0x) | 5.780 s (111.5x) |
| mandelbrot | 1000 x 1000 | 0.211 s (4.2x) | 0.066 s (1.3x) | 0.050 s (1.0x) | 3.033 s (60.1x) |
| binarytrees | depth 16 | 0.347 s (1.5x) | 0.184 s (0.8x) | 0.237 s (1.0x) | 1.108 s (4.7x) |
| knucleotide | fasta 150000 | 0.985 s (20.1x) | 0.520 s (10.6x) | 0.049 s (1.0x) | 0.518 s (10.6x) |
| pidigits | 10000 digits | 10.188 s (27.2x) | 2.206 s (5.9x) | 0.374 s (1.0x) | 2.090 s (5.6x) |

Updated 2026-10-05 after the native translator kept the top of the value
stack in a register (see "Native speed" below); the first run (2026-10-04)
had native at spigot 4.360 s, fannkuch 1.358, nbody 0.794, binarytrees
0.568, knucleotide 1.411, pidigits 21.059.

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

**mandelbrot** (a binary PBM bitmap of the set, 50 iterations per point).
Ran with no language changes: bit operations and raw byte output already
work (`buf.push_byte`, `fs.write`). The test size (199) is not a multiple of
8, so the padded last byte of each row is checked too.

**binarytrees** (millions of short-lived tree nodes, one long-lived tree).
AIPL has no general `free`, so at the benchmark's sizes it would run out of
its 64 MiB. Added `std/arena`: a typed region allocator (`(call (arena.alloc
Node) a)` returns a zeroed `(ptr Node)`, no cast; `arena.reset` frees
everything at once and its chunks are reused), as the benchmark allows for
memory pools. Not a like-for-like race: AIPL resets an arena per tree, the C
version calls malloc and free per node (the classic C entry), and Python
uses its garbage collector, which is why AIPL under wasmtime beats C here.
A general per-object free remains a gap. Writing it found a resolver bug,
fixed in both resolvers: a module calling its own generic function with its
own struct (`(alloc Pair)` inside std/arena) left the type argument
unqualified, so the instance's type no longer matched the struct once the
module was imported (`tests/test_generics.rs`).

**knucleotide** (count every 1-, 2-, ... 18-letter substring of a DNA
sequence read from stdin; frequency tables and occurrence counts). Its input
comes from the Benchmarks Game's fasta generator (`fasta.py`, run by
`tools/bench.py` through `stdin_cmd`). Ran with no language changes, using
`std/strmap` (keys are slices of the sequence, no copies) and `vec.sort_by`.
What it showed:
- **The 64 MiB memory cap.** At the benchmark's usual size (fasta 250000,
  1.25 million letters) both AIPL builds run out of memory: seven maps of up
  to a million entries each, none ever freed. Measured at 150000 instead. A
  larger cap (wasm allows up to 4 GiB; AIPL's signed 32-bit addresses make
  2 GiB the natural ceiling) is a design decision across the VM, both
  compilers, the launcher, and the native backend; a general free would
  help too. *Raised to 2 GiB on 2026-10-05:* the native build now runs
  fasta 1000000 (10 MB of input), with output identical to Python's.
- **String-keyed hash maps are slow.** AIPL is no faster than Python here:
  `strmap` hashes and compares keys byte by byte in AIPL, while Python's
  dict is optimised C, and the C version packs each k-mer into a 64-bit
  integer. Faster hashing (word at a time) or an integer-keyed map for short
  keys would help most.

**pidigits** (pi by Gibbons' unbounded spigot, as in the Benchmarks Game:
arbitrary-precision integers). C uses GMP, as the Benchmarks Game's C
entries do, so C here means decades of hand-tuned assembly rather than C
the language; Python uses its built-in integers. What it showed:
- **No big integers.** Added `std/bigint`: signed, 32-bit limbs in `i64`
  (AIPL has no unsigned compare or 128-bit multiply), changed in place
  because nothing is freed. It is checked against Python's integers by a
  4000-operation random walk (`tests/aipl/bigint_ops.aipl`,
  `tools/bigint_vectors.py`). A first version handled only non-negative
  numbers; the algorithm's `accum` goes negative, which is how the signed
  version came about. The walk then found a carry limb dropped in
  multiply-subtract.
- **Library speed matters more than the language here.** Finding each digit
  by repeated subtraction took 7.1 s (wasm); `div_small_quotient`
  (estimate the quotient in floating point from the top limbs, then
  correct) brought it to 4.0 s, and loops without per-limb bounds checks
  (limbs past the length are kept zero) to about Python's time.
- **Native is far behind wasm** (about 9x), the baseline translator's cost
  on tight loops again.
- **Length.** The AIPL file is 90 lines against C's 40 and Python's 29:
  bignums are calls rather than operators, state lives in a struct (no
  globals), and the argument parsing and digit output are repeated from
  spigot. `os.arg_int`, a stdout helper for `buf`, and a shared digit
  printer would remove about a third.

## Native speed (2026-10-05)

The two fixes these notes pointed at, measured one at a time (best of 3,
the compiler compiling itself as a ninth program):

- **The top of the value stack in a register.** The baseline translator
  pushed every value to the machine stack and popped it back for the next
  instruction; now a result stays in rax until something else needs the
  register, spilled only where control flow joins (blocks, branches,
  calls). About 2x on spigot, fannkuch, and pidigits, 1.5x on nbody,
  binarytrees, and the compiler itself; nothing on the float-heavy
  spectralnorm and mandelbrot. Kept.
- **The memory size in a register** (single-threaded code): no measurable
  difference, since the size cell is always in L1. Not kept.

Three further steps on the same line were tried and also measured as no
difference, so they were not kept: holding a constant or local unloaded
until an instruction can use it as an immediate or memory operand; holding
the value beneath it in a register too; and keeping a comparison in the
flags for the branch that uses it. Once the stack round trips are gone,
what sets the pace is that every local variable lives in memory: a loop
counter or accumulator is stored and reloaded on every iteration. The next
real gain needs locals in registers (a register allocator, at least for
each function's hottest locals), a project of its own.
