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
2026-10-15, after value structs, unions, and results by value
(docs/design/VALUES_PLAN.md); best of three. Absolute times on this
machine vary by about 5% from one run to the next, C's and Python's
included, so the ratios to C are the steadier comparison.

| Benchmark | Input | AIPL native | AIPL wasm (wasmtime) | C (gcc -O2) | Python 3 |
|---|---|---|---|---|---|
| binarytrees | 16 | 0.146 s (0.6x) | 0.155 s (0.6x) | 0.241 s (1.0x) | 1.143 s (4.7x) |
| fannkuch | 10 | 0.298 s (1.9x) | 0.249 s (1.6x) | 0.155 s (1.0x) | 3.802 s (24.6x) |
| knucleotide | stdin: fasta.py 150000 | 0.679 s (13.5x) | 0.615 s (12.2x) | 0.050 s (1.0x) | 0.517 s (10.3x) |
| mandelbrot | 1000 | 0.062 s (1.2x) | 0.070 s (1.4x) | 0.051 s (1.0x) | 3.075 s (60.7x) |
| nbody | 1000000 | 0.151 s (2.9x) | 0.080 s (1.5x) | 0.051 s (1.0x) | 5.553 s (108.0x) |
| pidigits | 10000 | 3.810 s (10.0x) | 2.397 s (6.3x) | 0.382 s (1.0x) | 2.124 s (5.6x) |
| spectralnorm | 1000 | 0.107 s (4.0x) | 0.127 s (4.8x) | 0.027 s (1.0x) | 7.289 s (273.7x) |
| spigot | 10000 | 1.426 s (1.5x) | 1.583 s (1.7x) | 0.931 s (1.0x) | 46.950 s (50.4x) |

Native code takes 0.8 to 1.9 times wasmtime's time: 1.9 on nbody, whose
float-heavy loop has more values live than native code has registers for
("Value structs" below), 1.6 on pidigits, and less than wasmtime's own
time on binarytrees, mandelbrot, spectralnorm, and spigot. Before value
structs (2026-10-14), nbody took 0.179 s natively and 0.139 s under
wasmtime.

Earlier (2026-10-05, before the null checks and the native rewrite):
native spigot 2.725 s, fannkuch 1.080, spectralnorm 0.392, nbody 0.751,
mandelbrot 0.212, binarytrees 0.393, knucleotide 1.395, pidigits 15.652;
wasm spigot 1.589, fannkuch 0.251, spectralnorm 0.123, nbody 0.120,
mandelbrot 0.068, binarytrees 0.197, knucleotide 0.734, pidigits 2.650.

**Null checks** (2026-10-08): every `get`, `put`, and array operation now
checks for a null pointer. Measured before and after (best of 3 to 5 runs):
binarytrees native +5%, wasm +7%; nbody native +6%, wasm +22% (its inner
loop is struct-field reads and writes); spectralnorm +2% and +3%; fannkuch
+8% and +6%. Every table since includes them.

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
  *Since the native rewrite (2026-10-14) native code is faster than
  wasmtime's here ("The value-tracking translator").*

**fannkuch** (fannkuch-redux, single-threaded: permutations of 0..n-1, each
flipped until 0 comes first). Ran with no language changes. What it showed:
array-heavy code is where the native translator trails wasmtime most (6.4
times): each `arr.get`/`arr.set` is several wasm instructions, every one
passing its values through the stack in memory, plus a bounds check that
reloads the memory size. wasmtime keeps them in registers. The same
optimisations as for spigot (value stack in registers, memory size in a
register) would matter most here. *Since the native rewrite, 1.2 times
wasmtime.*

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
  style) and parsing floats from text were gaps. *Since then:*
  `float.from_decimal` parses decimal text (2026-10-06; unsafe to call, as
  it takes a raw address), and `json.write` prints a number in its
  shortest round-trip digits (2026-10-14); a general shortest-digits
  writer in `fmt` is still missing.
C gains most here: gcc inlines the matrix-entry function and vectorises the
inner loop, while AIPL calls a function per entry.

**nbody** (the Sun and the four gas giants, energy printed to nine decimals
before and after). Since 2026-10-15 a body is a value struct, held inline in
an `(arr Body)` ("Value structs" below); it was seven arrays, one per
coordinate. Ran with the spectralnorm additions (`f64.sqrt`,
`fmt.f64_fixed`). The self-hosted compiler did not read exponent notation when this was
written, so the benchmark's constants are written out in decimal (the same doubles). All
four versions print the published values, bit-identical float arithmetic
in the same operation order. Float code is where the native translator
trailed most (8.6 times wasmtime): every operation moved values from the
stack in memory into xmm registers and back, where Cranelift keeps them in
registers. *Since the native rewrite float values live in xmm registers:
1.3 times wasmtime with the seven arrays, 1.9 with value structs, whose
inner loop holds more floats at once than there are registers.*

**mandelbrot** (a binary PBM bitmap of the set, 50 iterations per point).
Ran with no language changes: bit operations and raw byte output already
work (`buf.push_byte`, `fs.write`). The test size (199) is not a multiple of
8, so the padded last byte of each row is checked too.

**binarytrees** (millions of short-lived tree nodes, one long-lived tree).
AIPL had no general `free` (it has since 2026-10-06, `std/heap`), so at the
benchmark's sizes it would have run out of its then 64 MiB. Added `std/arena`: a typed region allocator (`(call (arena.alloc
Node) a)` returns a zeroed `(ptr Node)`, no cast; `arena.reset` frees
everything at once and its chunks are reused), as the benchmark allows for
memory pools. Not a like-for-like race: AIPL resets an arena per tree, the C
version calls malloc and free per node (the classic C entry), and Python
uses its garbage collector, which is why AIPL under wasmtime beats C here.
Freeing each node through `std/heap` instead took 1.12 s natively against
the arena's 0.26 s (docs/design/HEAP_PLAN.md H5, measured 2026-10-06, before
the native rewrite). Writing it found a resolver bug,
fixed in both resolvers: a module calling its own generic function with its
own struct (`(alloc Pair)` inside std/arena) left the type argument
unqualified, so the instance's type no longer matched the struct once the
module was imported (`tests/test_generics.rs`).

**knucleotide** (count every 1-, 2-, ... 18-letter substring of a DNA
sequence read from stdin; frequency tables and occurrence counts). Its input
comes from the Benchmarks Game's fasta generator (`fasta.py`, run by
`tools/bench.py` through `stdin_cmd`). Ran with no language changes, using
`std/strmap` (keys are slices of the sequence, no copies) and `vec.sort_by`;
since 2026-10-14 `(map.Map (ptr str.Bytes) i32)`, the one generic map
(`strmap` is gone).
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
  keys would help most. *2026-10-14:* `std/map` replaced `strmap`, and its
  byte-string hash reads the bytes directly rather than through the
  bounds-checked `str.byte_at`: native 1.306 s to 1.119 s, wasm 0.684 s to
  0.579 s (fasta 150000, same machine, back to back).

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
  on tight loops again. *1.6 times since the native rewrite.*
- **Length.** The AIPL file is 90 lines against C's 40 and Python's 29:
  bignums are calls rather than operators, state lives in a struct (no
  globals), and the argument parsing and digit output are repeated from
  spigot. `os.arg_int`, a stdout helper for `buf`, and a shared digit
  printer would remove about a third. *2026-10-05:* `os.arg_int` and
  `buf.print` are in the standard library and every benchmark uses them
  (55 lines fewer across the eight); the digit printer stays in spigot and
  pidigits, since it prints one contest's output format.

## Native speed (2026-10-05)

The sections from here on are a log, oldest first: each describes the
translator and the numbers of its day, and the Results table above is the
current state.

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

## Locals in registers (2026-10-06)

Each function's four most used locals (a use inside a loop counting 8
times per level) now live in rbx, r12, r13, and r14, saved on entry and
restored on return; around a call to an import, whose routine uses those
registers, they go back to their slots. Native times, before and after
(with the compiled checks of docs/design/CHECKS_PLAN.md in both):

| Benchmark | before | after |
|---|---|---|
| spigot | 2.725 s | 2.479 s |
| fannkuch | 1.080 s | 1.019 s |
| nbody | 0.751 s | 0.697 s |
| knucleotide | 1.395 s | 1.141 s |
| pidigits | 15.652 s | 14.213 s |

6-18%, less than expected. Two measurements say where the rest goes:

- **Not the memory bounds checks.** Removing the check on every load and
  store (unsafely, as a measurement only) gained another 6-14%. Checking
  through guard pages instead (a reserved region past the memory that
  faults, as wasmtime does) would get about that, at the cost of a fault
  handler.
- **The translation of one instruction at a time.** In `s = s + a[i]` the
  loop body is about 60 instructions where an optimising compiler emits
  under 10: each operand passes through rax and the machine stack
  (`n - 1` is `push rax; mov eax,1; mov rcx,rax; pop rax; sub eax,ecx`).
  Closing most of the gap to wasmtime needs a translator that tracks
  where each value on the wasm stack is (a register, a constant, a local)
  and emits code only when a value is used, as single-pass compilers such
  as V8's Liftoff do: a rewrite of the instruction translation in
  lower.aipl, not a tweak. Earlier attempts at pieces of this (constants
  as immediates, two values in registers) measured no gain on their own
  while locals were in memory.

## The value-tracking translator (2026-10-14)

docs/design/NATIVE_SPEED_PLAN.md, stage by stage, in the order done (F4
before F3). Native times, best of five, with wasmtime's alongside
(`python3 tools/bench.py --runs 5 --no-python`; the last two columns from
the results table of 2026-10-14, before value structs); F0 is the baseline,
null checks included.

| Benchmark | F0 | F1 | F1, floats | F2 | F4 | F3 | wasm (wasmtime) | native / wasm, F0 → now |
|---|---|---|---|---|---|---|---|---|
| binarytrees | 0.299 s | 0.237 s | 0.240 s | 0.212 s | 0.184 s | 0.148 s | 0.150 s | 1.9x → 0.99x |
| fannkuch | 1.112 s | 0.762 s | 0.765 s | 0.691 s | 0.492 s | 0.297 s | 0.244 s | 4.2x → 1.22x |
| knucleotide | 1.207 s | 1.068 s | 1.046 s | 1.007 s | 0.816 s | 0.681 s | 0.617 s | 1.9x → 1.10x |
| mandelbrot | 0.208 s | 0.209 s | 0.147 s | 0.143 s | 0.145 s | 0.065 s | 0.069 s | 3.0x → 0.94x |
| nbody | 0.761 s | 0.563 s | 0.496 s | 0.418 s | 0.324 s | 0.179 s | 0.139 s | 5.2x → 1.29x |
| pidigits | 15.559 s | 9.904 s | 9.819 s | 8.734 s | 5.757 s | 3.775 s | 2.379 s | 5.7x → 1.59x |
| spectralnorm | 0.391 s | 0.385 s | 0.260 s | 0.260 s | 0.259 s | 0.101 s | 0.125 s | 3.1x → 0.81x |
| spigot | 2.632 s | 1.713 s | 1.674 s | 1.459 s | 1.471 s | 1.416 s | 1.576 s | 1.6x → 0.90x |

**F1, the value stack.** The upper part of the wasm value stack is
virtual: constants, locals, and values in seven registers (five since F3
gave two to locals), used where they
are; only control flow, calls, and the rarer instructions put values in
memory. Integer code gains 12-36%. The float benchmarks (mandelbrot,
spectralnorm) did not move until float values got registers of their own
too (xmm2-xmm15, "F1, floats"): 12-32% on the three float benchmarks.

**F2, instruction selection.** A comparison an `if`, `br_if`, or `eqz`
takes next stays in the flags (`cmp; jl` instead of `cmp; setl; movzx;
test; je`); `local = local op x` works on the local's register in place;
`local ± constant` is one `lea`; shifts by a constant use the immediate
form; a division by a constant other than 0 and -1 skips its checks.
5-17%; spigot is now faster natively than under wasmtime.

**F4, guard pages** (ahead of F3, being independent of it). Linear memory
is an 8 GiB reservation with no access past its current size, so a load
or store needs no check: one outside the memory faults, and a SIGSEGV
handler makes the faulting code call the trap routine, which prints the
same message and call chain as before. With it, multiplication by a
power of two became a shift. 13-34% on the memory-bound benchmarks, more
than the 6-14% measured for removing the check on 2026-10-06: the rest of
each access has since become cheaper, so the check was a larger share.

**F3, registers and checks** (after F4). Float locals live in xmm8-xmm15
and integer locals in six registers (rbx, r12-r14, r8, r9); a local in its
frame slot is used from memory in place; a defined function with up to six
parameters takes them in registers. The compiled checks (null, bounds,
store guard, contracts) cost far less: a failed check's `if` keeps the
value stack in registers (its end is reached only by not taking it), and
its code moves after the function's, so the hot path is one conditional
jump it does not take. Division by a power of two is shifts; a `br_if`
with nothing to drop is one jump; loop starts are aligned to 32 bytes. The
store guard became two checks, each its own `if`, in both toolchains'
wasm, which also made wasmtime faster on pidigits (2.6 s to 2.3 s).
Mandelbrot 2.4x and spectralnorm 2.4x faster than after F4, fannkuch 1.7x,
pidigits 1.5x.

## Value structs (2026-10-15)

docs/design/VALUES_PLAN.md V4: nbody rewritten with value structs, a
`Vec3` and a `Body` of position, velocity, and mass (seven `f64`s) held
inline in an `(arr Body)`, where it kept seven `(arr f64)`, one per
coordinate. Every operation is in the same order, so the output is the
same to the last bit. Best of five, before and after:

| | native | wasm (wasmtime) | C |
|---|---|---|---|
| seven arrays | 0.184 s | 0.141 s | 0.051 s |
| `(arr Body)` | 0.143 s | 0.076 s | 0.049 s |

Under wasmtime it is 1.9 times as fast, and 1.5 times C. Where the arrays
version made each body's seven numbers seven separate loads from seven
arrays (seven bounds checks, seven null checks), the value version reads a
body with one check and seven loads from one place, and Cranelift keeps
the numbers in registers.

Native code gains 22%, and now trails wasmtime by 1.9x here, where it is
within 1.6x on the other benchmarks: a body's seven numbers, the other
body's, and the intermediate values are far more floats than the eight
registers native code gives float locals, so the rest live in memory, where
Cranelift allocates registers for the whole loop. Several results per call
(how a function returns a value struct natively: through an area in
memory, not registers) do not matter here, since the inner loop calls no
function. Closing it means a better register choice for float-heavy loops,
not anything specific to values.

std gained the same way: `fs.info` returns an `fs.Info` value (it made a
heap cell, never freed, on every call), and `json.write` finds a number's
shortest digits without one either.


## Unions by value (2026-10-15)

docs/design/VALUES_PLAN.md V5: unions that fit are held by value, among
them `std/alloc`'s `Allocator` (2 numbers, a tag and a shared pointer
slot, where it was one address), which every container holds and every
allocation is passed. Best of three, against the run made just before it
with value structs only (both runs on the same machine within the hour):

| Benchmark | native | wasm (wasmtime) |
|---|---|---|
| binarytrees | 0.137 → 0.137 s | 0.143 → 0.147 s |
| knucleotide | 0.624 → 0.647 s | 0.566 → 0.581 s |
| pidigits | 3.602 → 3.689 s | 2.292 → 2.336 s |
| spigot | 1.355 → 1.381 s | 1.511 → 1.532 s |

The others moved by under 1%. The changes are 0-4%, near the run-to-run
noise (C's own times moved 1-2% between the two runs), and lean slower
on the benchmarks that allocate through the library, which now pass an
allocator as two numbers. What unions by value buy is allocations: an
`(option.Option T)` returned for a small `T`, or a `Shape` made in a loop,
takes no memory (`map.find` a million times takes none).

Results by value (the same day, the last of V5) changed no benchmark's
time: none of them makes results in a loop.
