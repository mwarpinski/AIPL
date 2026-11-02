# The optimizer: faster programs by default

**Status: approved 2026-10-25: the optimizer on by default for compiled
output, once step 1 is done. Steps 1-3 done 2026-10-25, step 4's
knucleotide 2026-10-26, its nbody and pidigits deferred: `aipl compile`
optimizes by default.** The owner's aim: faster programs first, a
faster compiler second.

## What exists

aipl_src/codegen_opt.aipl (2,192 lines, written by an outside model,
2026-10-17) is a source-level optimizer in front of the self-hosted code
generator, not a new code generator. Its own pipeline (`codegen_opt compile
FILE -o OUT [--exe]`) resolves, expands generics, checks, lowers formats,
then rewrites the program and hands it to aipl_src/codegen.aipl (and to the
native backend for `--exe`). Four passes: constant folding, algebraic
identities and strength reduction; inlining of small functions (two
rounds); propagation of immutable locals and removal of dead ones; removal
of functions `main` cannot reach. Nothing in the repository used it until
now: no module imports it, no test ran it, and `--opt` does not exist. Its
text is codegen.aipl's input (enums already numbers), not AIPL source.

## What was measured (2026-10-25, branch features/optimizer)

- **Behaviour.** tools/opt_diff.py compiles each program as `aipl compile`
  does and through the optimizer, runs both, and compares output and exit
  status (positions and heap addresses aside: optimizing moves code and
  shrinks the data). Every program in examples/, benchmarks/, and
  tests/aipl/ that runs (72; 5 skipped: no input, or too slow at the
  default size): the same. tools/run_fuzz.py's generated programs (which
  mix every kind of value, branch, loop, trap, and contract): seeds 1, 2,
  and 3, 2,500 programs, all the same.
- **Error messages.** A trap's message, call chain, and `file:line:col`
  positions are the same optimized (checked on a use after free, a failed
  invariant, a failed precondition).
- **Speed** (tools/opt_bench.py, best of five, quiet machine; optimized
  time over plain, below 1.0 is faster):

  | Benchmark | native | wasm |
  |---|---|---|
  | spectralnorm | 0.65 | 0.59 |
  | knucleotide | 0.85 | 0.85 |
  | mandelbrot | 0.88 | 0.91 |
  | nbody | 1.01 | 0.89 |
  | binarytrees | 1.00 | 0.97 |
  | fannkuch | 1.00 | 0.97 |
  | pidigits | 1.00 | 0.99 |
  | spigot | 1.00 | 1.00 |

  Nothing slower; much faster where small functions sit in hot loops
  (spectralnorm's `a(i, j)`, knucleotide's per-byte calls).
- **Compile time.** About 0.3 s and 200 MB for a program with std/serve,
  against 0.05 s and 20 MB (the optimizer runs as AIPL in wasmtime).

## The plan

| Step | What |
|---|---|
| 1 | Make it safe to rely on |
| 2 | Make it the default for compiled output |
| 3 | Document and measure |
| 4 | The next speed: the biggest gaps to C |

### 1. Make it safe to rely on

**Done 2026-10-25** (branch features/optimizer):

- **Its output is type-checked.** consts.expand_constants keeps enums, so
  the optimizer's text is AIPL the checker reads, and `codegen_opt
  compile` checks it before code generation (docs/NOTES_REVIEW.md,
  final_thoughts: a rewrite once made code the checker would have
  refused, and native code crashed). A program that does not type-check is
  a compile error naming the optimizer and `--no-opt`, never wrong code.
  The check found two bugs at once: a `(no_leak)` clause was taken for a
  body statement and inlined into callers; and an `if` whose branch folds
  to an empty block (a `when false`, a `while false`) must become a
  `when`, since an `if` has two real branches.
- **It knows `when`.** The body is optimized as statements; `(when false
  ...)` is nothing and `(when true b...)` is `(block b... (block))`, the
  value still dropped. Pass 3 propagates only a function's top-level
  `let`s of names declared once, so a `let` inside a `when` is never
  carried past it.
- **The passes, read against their claims.** The spec makes `i32`/`i64`
  wrapping and shift counts masked, so reassociation, `(* x 2^k)` as a
  shift, `divu`/`remu` by a power of two, and the parity rule (`(/ e 2)`
  as `(shr e 1)` for a provably even `e`: exact division, so truncation
  and flooring agree) are sound; integer literals are `i32` only (an `f64`
  operand needs `1.0`), so no float meets these rules. `is_pure` leaves
  out everything that can trap or touch memory (`call`, `get`, `arr.get`,
  division by a non-constant). The inliner takes bodies with no binder
  (no `let`, `match`, loop, `set!`, or `return`), so nothing captures an
  argument; an argument that is not an atom must be pure and used at most
  once. Tests in `codegen_opt --test`: 6 (the `when` and empty-branch
  folds) and 7 (what it must not do: drop a call or a read, inline an
  argument twice or into a branch that may skip its trap, fuse shifts past
  31), each shown to fail with its rule removed.
- **An inlined function has no frame.** A trap inside one was reported at
  the callee's line under the caller's name (`at main (t.aipl:3:37)`,
  line 3 being in `at`). An inlined body now takes its call's position,
  so the report is true: `at main (t.aipl:8:5)`, the call. Recovering the
  callee's frame needs the line table to carry inline chains (the
  optimizer's origins, codegen.aipl, aipl-run's and the native runtime's
  trap reporters): not done; worth it if models debugging optimized
  programs miss the inner line. tests/aipl/opt/inline_trap.aipl.
- **CI.** tools/opt_diff.py (the optimizer's own tests, then every
  repository program and 500 generated ones) in .github/workflows/ci.yml
  and tools/ci.sh. It compares output and status, a failure's message
  exactly (heap addresses aside), and its call chain: the plain one less
  whole frames, every kept frame identical, position and all. Each run
  gets a fresh copy of the program's directory and fixed input (a
  benchmark's `stdin`, else none; inheriting the caller's stdin made
  readers block, and skips came and went with how it was started). 77
  repository programs (1 skipped: it is meant not to compile) and 2,000
  generated ones, seeds 1-4: the same, 134 of seed 1's failing the same
  way. About 1.5 minutes on 20 cores; most of it is wasmtime compiling
  the optimizer afresh for each program (below).

### 2. Make it the default for compiled output

**Done 2026-10-25** (branch features/optimizer):

- driver.aipl's `compile_file_opt` and `compile_tests_opt` run the
  optimizer between format lowering and code generation and type-check
  what it makes; codegen_opt.aipl keeps only the optimizer (its pipeline
  copy and its own command are gone), and its tests are a group of
  aipl_src/test_suite.aipl.
- Both commands optimize by default, `--no-opt` as before (and `--self`
  implies it). The Rust command checks with its own front end first, so
  every error reads as it did, then runs `driver.compile_file_opt` under
  wasmtime (src/selfhost.rs). **Parity survives:** both commands run the
  same AIPL, so they write the same bytes optimized as unoptimized
  (tests/test_bootstrap.rs compares them on every repository program), and
  the bootstrap reaches its fixed point with an optimized stage 2 (3.65 MB
  against 3.94 MB).
- **The cost, solved.** Compiled toolchain modules are cached between runs
  (`$AIPL_CACHE`, else `$XDG_CACHE_HOME/aipl` or `~/.cache/aipl`), keyed by
  the Rust sources (build.rs `AIPL_SRC_HASH`) and every file below
  aipl_src/; `Module::serialize`, with wasmtime refusing a stale or damaged
  entry, which is then compiled again. An optimized compile: 0.08 s (0.5 s
  and 1.8 s of CPU the first time after a change). src/native.rs uses the
  same loader, so `--exe` no longer resolves, checks, and compiles the
  backend on every call. tools/opt_diff.py's 500 generated programs went
  from 85 s to 30 s.
- Tests that compare a compiled build with the VM's addresses or call
  chains, or the Rust code generator's bytes, pass `--no-opt`
  (tests/test_formats.rs, test_heap.rs, test_memory.rs, test_runner.rs,
  tools/fuzz.py, tools/run_fuzz.py); every other test now runs optimized
  builds. tests/test_opt.rs: an inlined function's trap at its call under
  aipl-run and natively, `--no-opt` keeping the frame, and the cache
  surviving a damaged entry and turned off.
- The spec (pipeline, 6.1, a new 6.7, section 9) and the guide say what
  `aipl compile` does and what differs (frames, addresses).
- Known: optimizing the whole compiler peaks at 1 GB of the 2 GB a module
  may use; the optimizer allocates without freeing. A program several
  times its size could fail to optimize (an error naming `--no-opt`).
  Freeing per function is the fix when one does.

What was planned:

`aipl compile` (wasm and `--exe`) builds through the optimizer; `--no-opt`
builds as today. `aipl eval` (the VM), `aipl test`, and `aipl verify` are
unchanged. The Rust command runs the optimizer in wasmtime, as it runs the
native backend (src/native.rs); the self-hosted command calls it directly.

**Compile cost first.** Each optimizer run spends about 1.8 s of CPU (0.2 s
on 20 cores) in wasmtime compiling the 1.4 MB optimizer module, every
time; a plain compile is 0.01 s. `aipl compile --exe` already pays the
same for the native backend, and more: it resolves, checks, and compiles
aipl_src/native/native.aipl from source on every call. On by default, that
cost lands on every compile, so step 2 caches the compiled modules (by
the hash of their bytes and wasmtime's version), or builds them once at
install. To decide in step 2: wasmtime's own cache or `Module::serialize`
(its deserialization trusts the file, as a binary is trusted).

The spec (section 9, the call chain) gains: in optimized output a function
inlined into its caller has no frame, and a trap inside it is reported at
the call.

**For the owner:** this makes the default compiled output the self-hosted
toolchain's (the optimizer feeds codegen.aipl), with the Rust code
generator as `--no-opt`. Byte parity between the two toolchains stays the
rule for unoptimized output (every existing parity test passes `--no-opt`);
optimized output has one implementation, as the native backend does. The
alternative is a flag, `--opt`, off by default: no change to what users get
unless they ask, but programs are slower unless a model remembers it.
Recommended: on by default. **Decided (owner, 2026-10-25): on by
default**, with step 1 first, `--no-opt` documented, and any error the
optimizer itself raises naming `--no-opt`. Since a user's own tests run
unoptimized in the VM, the behaviour comparison (optimized against
unoptimized) is a permanent CI test, in place of the two-toolchain byte
parity that optimized output no longer has.

### 3. Document and measure

**Done 2026-10-25:** the spec and guide in step 2; docs/BENCHMARKS.md
re-measured both ways (tools/bench.py builds each AIPL version with and
without `--no-opt`; best of five on a quiet i9-12900H): optimized over
unoptimized, spectralnorm 0.64 natively and 0.57 under wasmtime,
knucleotide 0.85 and 0.86, mandelbrot 0.89 and 0.90, the rest within 5%,
nothing slower. docs/NOTES_REVIEW.md's codegen_opt rows have verdicts: the
speed claim true (spectralnorm more so), the missing output check fixed,
literal truncation wrong (tests/aipl/opt/literals.aipl, in CI through
opt_diff), the compile-error count still unchecked (no history before git).

What was planned: the spec (6.x: what `aipl compile` does), the guide's one line on
compiling, docs/BENCHMARKS.md re-measured both ways, and docs/notes'
claims about codegen_opt given verdicts in docs/NOTES_REVIEW.md.

### 4. The next speed: the biggest gaps to C

From docs/BENCHMARKS.md, optimized, 2026-10-25 (i9-12900H): knucleotide
8.9x C natively and 7.5x under wasmtime (hashing byte strings and a check
per byte; 17x on the earlier machine, before the optimizer), nbody 3.3x
natively against 1.7x under wasmtime (more values live than registers),
binarytrees and fannkuch 2.0x natively, spectralnorm 1.7x, and pidigits
(no C here: 10.4x on the earlier machine; bigint without a 64x64 to
128-bit multiply). Each gets profiled, and a plan names what to change
(std, the optimizer, or the native backend), measured before and after.

**knucleotide, done 2026-10-26.** Profiled (`AIPL_PROFILE=perfmap`, new in
aipl-run; docs/BENCHMARKS.md, "Profiling"): the program itself was under 4%;
std/map was the rest, comparing keys 37%, probing 20%, hashing 14%. Probe
lengths were fine (1.4 to 1.7 keys compared a lookup, simulated on the
real k-mers; a stronger hash changed nothing), so the cost was two lookups
per count (`get_or`, then `set`) and a comparison wrapper the inliner left.
Changes, all general:
- std/map `(map.add K) [m k d] -> i32`: a count in one lookup (`claim`,
  shared with `set`). knucleotide 0.391 s to 0.258 natively (0.66), 0.332
  to 0.225 under wasmtime (0.68): 8.9x C to 5.8x, and now faster than
  Python (0.295).
- The inliner binds an argument that is not an atom (or a pure one used
  once) to a fresh `let`, in the call's order; every argument but a
  literal, once any is bound, so a variable is read where the call read it.
  The fresh names' prefix is found in no symbol of the program. This
  inlined `cmp.Hash<str.Bytes>.same`; no benchmark moved by more than
  noise.
- Two bugs found on the way, both fixed: the optimizer folded contracts,
  so a failed one printed rewritten text (`(shl i 1)` for `(* i 2)`), and
  an inlined call in a contract brought a `let` that code generation
  cannot see (contracts are now never rewritten;
  tests/aipl/opt/contracts.aipl). And one in the language, open: a `let`
  inside a contract verifies but does not compile (docs/HANDOFF.md, Open
  problems).
- Left: storing each slot's hash would skip most comparisons (about 6% of
  knucleotide) but changes the order a map iterates in, which the
  self-hosted compiler's output may depend on; not now.

pidigits now has a C time on this machine: 7.9x natively, 4.7x under
wasmtime.

**Deferred (owner, 2026-10-26: correct, only slower than they could be;
release work comes first):**
- nbody natively (3.3x C, 1.7x under wasmtime): its `advance` has about 37
  float locals, and native code keeps the 8 most used in registers for
  the whole function, where Cranelift allocates by liveness. The fix is a
  liveness-based register allocator in aipl_src/native/lower.aipl, its own
  plan; it would likely help binarytrees and fannkuch natively too (2.0x
  C, 1.6x under wasmtime).
- pidigits (7.9x C natively, 4.7x under wasmtime): 98% in std/bigint's
  inner loops (`mul_small`, `magnitude_sub_mul`, `magnitude_add_mul`,
  `assign`), 32-bit limbs each in an i64. Packing the limbs in 32 bits
  would halve the memory traffic (std only); 64-bit limbs need a 64x64 to
  128-bit multiply, which neither AIPL nor core wasm has (wasm's "wide
  arithmetic" proposal would add it): a language decision.

The optimizer itself is done: on by default, checked, and compared with
unoptimized builds in CI.
