# AIPL Language Gap Analysis

What AIPL does **not** do yet, checked against the code on 2026-10-01. [AIPL_SPEC.md](AIPL_SPEC.md) describes what it does; [PROGRESS.md](PROGRESS.md) has task status and how to verify; [AIPL_Structural_Audit.md](AIPL_Structural_Audit.md) has the ordered task list (P-numbers below refer to it). When something here gets built, delete its entry rather than appending an update note.

---

## 1. Toolchain

- **The self-hosted toolchain has no type checker.** `aipl_src/driver.aipl` (resolver + codegen, a WASI command when compiled) compiles multi-file programs to the same bytes as the Rust toolchain, itself included, but it trusts its input: only the Rust checker type-checks. Float literals beyond `m ≤ 2^53`, `k ≤ 22` are compile error 973 (AIPL_SPEC.md 6.4).
- **Two import resolvers.** `src/resolver.rs` serves `verify`, `eval`, `compile`, and `test`; `aipl_src/resolver.aipl` serves `compile --self` and the wasm toolchain. Tests hold them equal (AIPL_SPEC.md 11). The Rust one can go once the checker is also in AIPL, since the Rust checker consumes the Rust resolver's parsed module.
- **The VM is slow.** It is a tree-walker: every variable lookup is a string-keyed hash lookup and every block clones its scope. Calls no longer copy the function body (audit B13), but the self-hosted compiler still takes seconds in the VM for work the same compiler compiled to wasm does in milliseconds. Use the compiled toolchain for anything large.
- **Contracts are VM-only, and `inv` is never evaluated (audit B5).** The wasm backend emits no contracts, and nothing is proven statically (`aipl verify` says so).
- **No language or ABI versioning (deferred from P12).** No `:version` in modules and no version in the wasm output. Deliberately deferred until there are packages from different authors or a second toolchain; see the audit's P12 note.
- **Most VM runtime errors have no source position.** Contract failures do (`Pre-condition failed in 'f' at 1:37: (req (gt n 0)) with n = -1`); an out-of-bounds index, a bad memory access, or a store into the reserved block names the op and address but not the line.

## 2. Language

- **Function references but no closures.** `(ref f)` and `call_ref` (P10) give first-class references to named functions; there are no anonymous functions and nothing captures variables, so state goes through an argument (as `thread.spawn`'s `i32` does).
- **No generics.** Containers (`std/vec`, `std/map`, `std/strmap`) hold `i32` words; a list of structs stores addresses with `ptr.addr` and reads them back with `ptr.cast`, an unchecked cast at every use. Generic types (`(vec T)`, `(map K V)`) would make these checked, and are the language feature the collections most need.
- **Structs live only behind pointers.** `(ptr S)` and `(arr T)` are strictly typed, but there are no by-value or nested structs, no arrays of structs by value (packed records need `ptr.cast` arithmetic, as `compiler.aipl`'s `token_at` does), no unions (a field used two ways, like `compiler.aipl`'s `Node.a`, needs a cast), and no enums or general pattern matching (`match_result` is the only match).
- **No visibility.** Every function and struct in every module is addressable by its qualified name.
- **No module-level state.** There are no globals; modules keep state in `mem.alloc`'d blocks whose pointers live in runtime cells (codegen.aipl owns cells 4–60).
- **Numeric gaps:** no `f32` literals, no `f32` conversions (only `i64`↔`f64`: `f64.convert_i64_s`, `i64.trunc_f64_s`, and the two reinterprets; go through `i64.extend_s` for `i32`), `mem.load_f32/f64` and `mem.store_f32/f64` are rejected by both backends (use struct fields or arrays of `f64`), no exponent notation in float literals, and loop bounds and addresses are `i32` only.

## 3. Memory

- **`mem.free` is a no-op** in both backends (the argument is not even evaluated). The allocator is bump-only, so long-running programs leak.
- **Array bounds are checked only in the VM.** Compiled `arr.get`/`arr.set` with a bad index reads or writes neighbouring heap memory (AIPL_SPEC.md 4.E).
- **Reads from the reserved block 0–1023 are not checked.** This is deliberate: the block holds zeros and runtime cells.
- **Memory caps at 1024 pages (64 MiB)** in both backends. Allocation grows memory up to the cap automatically; a program that needs more fails at the first access past the end.

## 4. Strings and I/O

- **No string concatenation in wasm.** `(+ str str)` is VM-only.
- **`sys.print` takes only `str` in wasm.** Numbers print through the standard library (`io.print_int`, `io.println_int`, `fmt.*`), and strings become byte slices with `str.from_str`. The library (AIPL_SPEC.md 12.6) has text I/O, number formatting and parsing (`str.parse_int`), a string builder (`buf`), and `i32` collections (`vec`, `map`, `strmap`); still missing are floats in text, sets, and anything generic.
- **VM `str` values are Rust strings, not pointers.** The VM lays out the first loaded module's literals at the same addresses as wasm, so `str.ptr` agrees; a string that is not one of those literals (from a module loaded later into the same VM) is copied onto the heap each time it is used.
- **`sys.exit` in the VM returns an error** (`sys.exit(N) requested`) instead of setting the process exit code. **`sys.time` is unimplemented** in both backends.

## 5. Concurrency

`thread.spawn`/`thread.join` and `atomic.*` are real in the VM (OS threads sharing linear memory) and rejected by the wasm backend, which would need shared memory and wasi-threads. `mem.alloc` is not atomic, so allocate before spawning.

## 6. Self-hosted compiler capacities

`codegen.aipl` uses fixed-size tables and reports a compile error (never a miscompile) past them: 2048 functions, 16 parameters, 1024 locals per function, 255 structs of up to 15 fields, 32 distinct `call_ref` signatures, 64 KiB / 1364 string literals (AIPL_SPEC.md 6.4). The toolchain itself is about 250 functions. The Rust backend has none of these limits.

## 7. Before "many modules from many authors" is safe

Still true from the original analysis, updated for what has landed:

1. **Shared data layouts have a type and a namespace.** Structs (P8) are reached through typed `(ptr S)` pointers and are qualified by module (`compiler.Node`), so two packages can each define `Node`. What is still missing is visibility: every struct and function of an imported module is reachable.
2. **Duplication is still easy.** Imports make reuse possible, not the default; discoverability and lint tooling are missing.
3. **No versioning or dependency resolution.** Imports resolve by filename through the search path (AIPL_SPEC.md 11); there is no language version (deferred) and nothing handles package versions.
4. **No privacy.** Every helper is public.

## 8. On the longer-term ambition (browser / PDF viewer in pure AIPL)

A PDF reader or rasteriser is a realistic first "real systems program" now that the standard library (P8b) and control flow (P11) exist, once a faster interpreter or compiled self-host exist. It needs binary parsing, vector rasterisation into a memory buffer, and file I/O, all of which compile to wasm today. A browser additionally needs a windowing/graphics host, which wasm alone cannot provide, plus HTML/CSS/layout/font/image/network stacks. Neither is near-term; the path runs through sections 1–4 above.
