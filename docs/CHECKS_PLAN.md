# Contracts and array bounds checks in compiled code

Today `req`/`ens` and array bounds checks run only in the VM: a compiled
program skips its contracts and reads or writes past the end of an array
silently (AIPL_SPEC.md 4.E, 7.6; audit D1, B5). This plan makes compiled
programs (wasm, and native through it) check them too, so a program behaves
the same however it runs.

## Design

- **Always on.** Compiled code checks every `req`, every `ens`, and every
  `arr.get`/`arr.set`, as the VM does; there is no flag to turn them off
  (Rust checks array bounds the same way). The cost is measured on the
  benchmarks and the compiler itself.
- **What a failure looks like.** The check writes the VM's message
  (`Array index out of bounds: index 7 for array of length 5`, at 128 in
  the reserved block, so a failure needs no allocation;
  `Pre-condition failed in 'f' at 3:5: (req (gt n 0))`, a string literal),
  stores its address and length in runtime cells 92 and 96, and
  executes `unreachable`. `aipl-run` and native executables print
  `<program>: <message>` instead of `<program>: wasm trap: unreachable`,
  and exit with 134 as for any trap. No WASI import is needed, so a module
  still runs on any host; one that does not know the cells shows an
  ordinary trap. The cells are not per thread: the first failure ends the
  program.
- **Helpers, emitted once per module that needs them**, so each check
  site is a comparison and a call: `$aipl_oob(index, array)` formats the
  bounds message with `$aipl_dec` (signed decimal). The call is followed by
  `unreachable`: without it wasmtime keeps every live value across the
  call, and the checks cost nbody 2x instead of 1.3x.
- **`ens` and early `return`.** A function with `ens` compiles its body
  inside a block; `(return v)` stores `v` in the hidden local `res` and
  branches to the block's end, where the `ens` conditions are checked
  before the function returns.
- **Both compilers.** Every lowering is implemented in `src/compiler/wasm.rs`
  and `aipl_src/codegen.aipl` at byte parity, as always.
- **Agreement with the VM.** `tests/test_differential.rs` stops accepting
  "VM failed, wasm succeeded" for contracts and bounds: both must fail.

## Steps

| Step | What | Status |
|---|---|---|
| CC1 | The failure convention: cells 92/96, `aipl-run` and the native trap routine print the message; spec 7.9 | done |
| CC2 | Array bounds checks in both compilers (`$aipl_oob`), at byte parity; the differential test requires agreement; cost measured | done |
| CC3 | `req` checked at function entry, `ens` at every exit (`return` included), messages as the VM's without the argument values; byte parity; differential agreement | |
| CC4 | Argument values in compiled contract messages (`with n = -1`) for integer and bool parameters, if worth its size | |
| CC5 | Docs (AIPL_SPEC 4.E, 7.6, 6.3, 10.4; PROMPT_GUIDE; LANGUAGE_GAPS; audit D1/B5), benchmarks re-run | |

Each step is one or more commits on `features/compiled-checks`.

## Cost of the bounds checks (CC2)

Each `arr.get`/`arr.set` now loads the length and compares (about ten
wasm instructions). Best of three, before and after:

| Benchmark | wasm before | wasm after | native before | native after |
|---|---|---|---|---|
| nbody | 0.088 s | 0.113 s | 0.517 s | 0.752 s |
| pidigits | 2.206 s | 2.641 s | 10.188 s | 15.609 s |
| fannkuch | 0.203 s | 0.248 s | 0.719 s | 1.090 s |

About 20-28% under wasmtime and 45-55% natively on array-bound loops:
the native translator keeps every local in memory, so the check's extra
local reads and writes cost more there; a register allocator (PROGRESS.md)
is the fix.
