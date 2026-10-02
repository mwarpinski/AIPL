# AIPL Language Gap Analysis

What AIPL does **not** do yet, checked against the code on 2026-10-01. [AIPL_SPEC.md](AIPL_SPEC.md) describes what it does; [PROGRESS.md](PROGRESS.md) has task status and how to verify; [AIPL_Structural_Audit.md](AIPL_Structural_Audit.md) has the ordered task list (P-numbers below refer to it). When something here gets built, delete its entry rather than appending an update note.

---

## 1. Toolchain

- **The self-hosted compiler takes one import-free module.** `aipl_src/codegen.aipl` produces bytes identical to the Rust backend for the whole language except VM-only ops, including itself (AIPL_SPEC.md 6.4). It does not resolve `(import ...)` itself: `aipl compile --self` resolves in Rust and prints the flat module back as source for it (P14 moves resolution into AIPL), float literals beyond `m ≤ 2^53`, `k ≤ 22` are compile error 973, and there is no standalone self-hosted command yet (a `driver.aipl` around `codegen.compile_module`).
- **Import resolution is Rust (P14).** `src/resolver.rs` is bootstrap scaffolding. WASI file I/O now exists, so nothing blocks rewriting it in AIPL except P9 finishing first.
- **The VM is slow (audit B13).** `invoke` clones the whole function body on every call. codegen.aipl compiling itself takes about 80 s in a debug build.
- **Contracts are VM-only, and `inv` is never evaluated (audit B5).** The wasm backend emits no contracts. `aipl verify` prints "contracts verified" after type-checking them, not proving them.
- **No language or ABI versioning (deferred from P12).** No `:version` in modules and no version in the wasm output. Deliberately deferred until there are packages from different authors or a second toolchain; see the audit's P12 note.
- **VM runtime errors have no source position.** A contract failure prints the contract as a Rust `Debug` dump, not source text.

## 2. Language

- **Function references but no closures.** `(ref f)` and `call_ref` (P10) give first-class references to named functions; there are no anonymous functions and nothing captures variables, so state goes through an argument (as `thread.spawn`'s `i32` does).
- **Structs live only behind pointers.** `(ptr S)` and `(arr T)` are strictly typed, but there are no by-value or nested structs, no arrays of structs by value (packed records need `ptr.cast` arithmetic, as `compiler.aipl`'s `token_at` does), no unions (a field used two ways, like `compiler.aipl`'s `Node.a`, needs a cast), and no enums or general pattern matching (`match_result` is the only match).
- **No generics, no visibility.** Every function in every module is addressable by its qualified name.
- **No module-level state.** There are no globals; modules keep state in `mem.alloc`'d blocks whose pointers live in runtime cells (codegen.aipl owns cells 4–60).
- **`and`/`or` do not short-circuit,** in either backend. Guard side-effecting or trapping operands with a nested `if`.
- **Numeric gaps:** no `f32` literals, no `f32` conversions (only `i64`↔`f64`: `f64.convert_i64_s`, `i64.trunc_f64_s`, and the two reinterprets; go through `i64.extend_s` for `i32`), `mem.load_f32/f64` and `mem.store_f32/f64` are rejected by both backends (use struct fields or arrays of `f64`), no exponent notation in float literals, and loop bounds and addresses are `i32` only.

## 3. Memory

- **`mem.free` is a no-op** in both backends (the argument is not even evaluated). The allocator is bump-only, so long-running programs leak.
- **Array bounds are checked only in the VM.** Compiled `arr.get`/`arr.set` with a bad index reads or writes neighbouring heap memory (AIPL_SPEC.md 4.E).
- **Reads from the reserved block 0–1023 are not checked.** This is deliberate: the block holds zeros and runtime cells.
- **Memory caps at 100 pages (6.4 MiB)** in both backends.

## 4. Strings and I/O

- **No string concatenation in wasm.** `(+ str str)` is VM-only.
- **`sys.print` takes only `str` in wasm.** Numbers print through the standard library (`io.print_int`, `io.println_int`, `fmt.*`), and strings become byte slices with `str.from_str`. The library is small: no string building or concatenation in wasm, no parsing of numbers from text, no collections.
- **VM `str` values are Rust strings, not pointers.** `str.ptr` and `str` struct fields copy the string into the heap each time, so heap addresses after those operations differ from wasm, where a `str` is the interned literal's address.
- **`sys.exit` in the VM returns an error** (`sys.exit(N) requested`) instead of setting the process exit code. **`sys.time` is unimplemented** in both backends.
- **String literals share a 512-byte area per module** (addresses 512–1023).

## 5. Concurrency

`thread.spawn`/`thread.join` and `atomic.*` are real in the VM (OS threads sharing linear memory) and rejected by the wasm backend, which would need shared memory and wasi-threads. `mem.alloc` is not atomic, so allocate before spawning.

## 6. Stale files still in the tree

- `examples/hello_browser.aipl` uses the removed `dom.*` ops and no longer parses. `tests/test_differential.rs` pins it as stale.
- `src/stdlib/web.rs` is a hard-coded JavaScript bridge string, left over from the removed `dom.*` ops.
- `README.md` describes the pre-P1 tree (since corrected; check it before trusting it).

## 7. Before "many modules from many authors" is safe

Still true from the original analysis, updated for what has landed:

1. **Shared data layouts have a type and a namespace.** Structs (P8) are reached through typed `(ptr S)` pointers and are qualified by module (`compiler.Node`), so two packages can each define `Node`. What is still missing is visibility: every struct and function of an imported module is reachable.
2. **Duplication is still easy.** `wasm_emitter.aipl` (now in `attic/`) was a live example. Imports make reuse possible, not the default; discoverability and lint tooling are missing.
3. **No versioning or dependency resolution.** Imports resolve by filename through the search path (AIPL_SPEC.md 11); there is no language version (deferred) and nothing handles package versions.
4. **No privacy.** Every helper is public.

## 8. On the longer-term ambition (browser / PDF viewer in pure AIPL)

A PDF reader or rasteriser is a realistic first "real systems program" now that the standard library (P8b) and control flow (P11) exist, once a faster interpreter or compiled self-host exist. It needs binary parsing, vector rasterisation into a memory buffer, and file I/O, all of which compile to wasm today. A browser additionally needs a windowing/graphics host, which wasm alone cannot provide, plus HTML/CSS/layout/font/image/network stacks. Neither is near-term; the path runs through sections 1–4 above.
