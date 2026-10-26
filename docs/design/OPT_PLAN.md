# The optimizer: faster programs by default

**Status: proposed 2026-10-25, for the owner.** The owner's aim: faster
programs first, a faster compiler second.

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

- Type-check the optimized program before code generation (docs/
  NOTES_REVIEW.md, final_thoughts: an optimizer rewrite once produced code
  the checker would have refused, and native code crashed). A rewrite
  that breaks typing becomes a compile error naming the optimizer, never
  wrong code.
- Teach it `when` (CANONICAL_PLAN.md step 4). The 2,500 generated programs
  use `when` and passed, but its passes should know the form rather than
  treat it as an unknown call: inlining into a when's body, a `let`
  inside one (block-scoped), and constant propagation across it.
- Read the four passes against their claims, with a test for each claim
  (the owner's rule for another agent's work): the known-bits parity
  reasoning ((/ even 2) as a shift), the inlining's hygiene, what it
  treats as pure.
- CI: tools/opt_diff.py over the repository's programs and 500 generated
  ones, as a test.

### 2. Make it the default for compiled output

`aipl compile` (wasm and `--exe`) builds through the optimizer; `--no-opt`
builds as today. `aipl eval` (the VM), `aipl test`, and `aipl verify` are
unchanged. The Rust command runs the optimizer in wasmtime, as it runs the
native backend (src/native.rs); the self-hosted command calls it directly.

**For the owner:** this makes the default compiled output the self-hosted
toolchain's (the optimizer feeds codegen.aipl), with the Rust code
generator as `--no-opt`. Byte parity between the two toolchains stays the
rule for unoptimized output (every existing parity test passes `--no-opt`);
optimized output has one implementation, as the native backend does. The
alternative is a flag, `--opt`, off by default: no change to what users get
unless they ask, but programs are slower unless a model remembers it.
Recommended: on by default.

### 3. Document and measure

The spec (6.x: what `aipl compile` does), the guide's one line on
compiling, docs/BENCHMARKS.md re-measured both ways, and docs/notes'
claims about codegen_opt given verdicts in docs/NOTES_REVIEW.md.

### 4. The next speed: the biggest gaps to C

From docs/BENCHMARKS.md (before the optimizer): knucleotide 17x C
(hashing byte strings and a check per byte), pidigits 10x (bigint without
a 64x64 to 128-bit multiply), spectralnorm 4x (now about 2.6x), nbody 3x
natively (more values live than registers). Each gets profiled, and a
plan names what to change (std, the optimizer, or the native backend),
measured before and after.
