# Roadmap

Where AIPL is going and what it does not do yet, checked against the code
on 2026-10-06. [AIPL_SPEC.md](AIPL_SPEC.md) describes what it does;
[DEVELOPING.md](DEVELOPING.md) has how to build and test it and the
decisions behind it; [docs/history/](docs/history/) has what was done when.
When something here gets built, delete its entry rather than appending a
note. References such as "audit D3" and "P12" are to
[docs/history/AUDIT_2026-10.md](docs/history/AUDIT_2026-10.md).

## Done

Wasm semantics as the specification, with the VM held to them by
differential testing; i32/i64 wrapping arithmetic, unsigned comparisons,
and checked arithmetic; structs, arrays, strict pointer types, function
references, generics checked once with interfaces and `where` clauses
(`std/cmp`, and one `std/map` for any hashable key), constants, enums, and
unions with `match` (generic ones too, and `std/option`); results with
payloads of any type; threads
and atomics; contracts and array bounds checks in every backend; WASI I/O
and a standard library with generic collections, Zig-style allocators,
JSON, and directory listing and file information;
a self-hosted toolchain (resolver, generics, type checker, code generator)
that matches the Rust one byte for byte and rebuilds itself; native Linux
x86-64 executables from a backend written in AIPL, whose single-pass
translator keeps values in registers and checks memory by guard pages
(0.8-2.0x wasmtime's time on the benchmarks); module visibility; eight
benchmarks against C and Python.

## Next, in order

1. **[docs/LAUNCH_CHECKLIST.md](docs/LAUNCH_CHECKLIST.md)**, before showing
   the project publicly: checking every claim, the first five minutes of a new user, and an
   experiment measuring how well AI agents write AIPL.
2. **The standard library:** networking, once WASI has listening sockets;
   starting processes, once WASI can (preview 1 cannot); a faster
   small-block path in `std/heap`.
3. **Rust out of the build path:** the stage-0 seed is done (2026-10-15,
   bootstrap/README.md): a checked-in native `aipl` builds the compiler on
   Linux x86-64 with no Rust, checked byte for byte against the Rust
   toolchain, which stays as the referee. Left: a WebAssembly seed for
   other platforms, the execute bit (a host import), `verify` in the AIPL
   command, and an installer that builds from the seed (DEVELOPING.md,
   "Direction").
4. **Packages, after the launch:** a package registry with versioned
   packages, dependency resolution, and lock files (decided 2026-10-07;
   its command and name are open, e.g. folded into the `aipl` CLI as
   `aipl add`, `aipl build`). Language versioning comes with it.

Explicitly not now, each additive later rather than a rewrite: SIMD,
64-bit memory, exceptions (results cover errors). Later: language
versioning (once packages exist), `inv` contracts.

## What AIPL does not do yet

### Toolchain

- **Rust is needed to bootstrap off Linux x86-64.** The self-hosted compiler (`aipl_src/aipl.aipl`: driver.aipl's resolver, generics, checker, and code generator, plus the native backend) compiles every program to the Rust toolchain's bytes, itself included. On Linux x86-64 it builds from the checked-in seed (`bootstrap/`); elsewhere the first build takes the Rust toolchain ("Next" 4). The `aipl` CLI's `verify`, `eval`, and `test` use the Rust front end.
- **WASI cannot set the execute bit.** The AIPL `aipl compile --exe` writes the executable, but `chmod +x` is left to the caller (`bootstrap/build.sh` does it); the Rust CLI sets it, and builds the launcher bundle for platforms without a native backend. The planned fix is a host-provided import. Native executables exist only for Linux x86-64; elsewhere the bundle is about 18 MB (wasmtime) and built for the host's OS and CPU.
- **Two of everything in the front end.** Resolver, generics, constants, parser, and checker exist in Rust (`src/`, used by the `aipl` CLI and the VM) and in AIPL (`aipl_src/`, used by the self-hosted compiler); tests hold each pair equal, message for message (AIPL_SPEC.md 6.4, 11). The Rust copies go when the CLI runs the AIPL ones.
- **Two copies drift unless held equal.** The fuzzer (`tools/fuzz.py`) checks the two front ends against each other on mutated programs; it finds no differences on 50,000 cases (2026-10-06), and CI runs 3,000 on every push. Only tests and the fuzzer hold the two equal, so a change to one copy must be made to the other.
- **The VM is slow.** It is a tree-walker: every variable lookup is a string-keyed hash lookup and every block clones its scope. Calls no longer copy the function body (audit B13), but the self-hosted compiler still takes seconds in the VM for work the same compiler compiled to wasm does in milliseconds. Use the compiled toolchain for anything large.
- **`inv` is never evaluated, and nothing is proven statically (audit B5).** `req` and `ens` run in every backend (since 2026-10-05); a compiled contract message shows no float or string values, and gives its position in the call chain after it rather than in the message.
- **No language or ABI versioning (deferred from P12).** No `:version` in modules and no version in the wasm output. Deliberately deferred until there are packages from different authors or a second toolchain; see the audit's P12 note.
- **Diagnostics report one error per function** (a definition's error alone), not every error in a function. Some common mistakes get generic messages: an `if` without an else is `Unexpected token parsing expression: ')'`.

### Language

- **No closures, by design.** `(ref f)` and `call_ref` give references to named functions; nothing captures variables, and a callback's data is a typed context passed as its first argument (`vec.sort_by_with`, `thread.spawn_with`, `alloc.custom`; AIPL_SPEC.md 4.G). Closures would allocate behind the program's back, enlarge every function value, and slow every call through one.
- **Generics are explicit.** Every use names its type arguments (no inference), and every call to an interface's function names the interface and the type. Templates are checked once with sealed type parameters (AIPL_SPEC.md 4.H). Not yet: interfaces with several type parameters, generic implementations (`(impl (Hash (ptr T)) ...)`, which need a rule against overlap first), and default functions in an interface.
- **Structs live only behind pointers.** `(ptr S)` and `(arr T)` are strictly typed, but there are no by-value or nested structs, and no arrays of structs by value (packed records need `ptr.cast` arithmetic, as `compiler.aipl`'s `token_at` does).
- **No module-level state.** There are no globals; modules keep state in a struct passed to every function (codegen.aipl's `Cg`), or in a `mem.alloc`'d block whose pointer lives in a runtime cell (codegen.aipl's keyword map, cell 16).
- **Constants are literals only.** `(const NAME:T literal)` (AIPL_SPEC.md 4.I) takes no expressions, not even another constant, and has no `f32` form. Older code still uses zero-argument functions as constants (`(fn cc_e [] -> i32 4)` in `aipl_src/native/x64.aipl`).
- **No 8- or 16-bit types** (`mem.load8` reads unsigned; there is no signed byte load). Unsigned comparisons (`ltu lteu gtu gteu`) and overflow-checked arithmetic (`checked.add/sub/mul`) exist (AIPL_SPEC.md 8.2); there is no checked division or shift, and no form that reports overflow as a value rather than stopping.
- **Unions are boxed and immutable.** A union value (AIPL_SPEC.md 4.J) is always a heap cell, even a variant without fields, and `make` is the only way to fill one: no in-place update, no union fields stored inline in a struct, and no nested patterns (a `match` arm binds one level of fields; match again on a field). Generic unions and `std/option` exist (AIPL_SPEC.md 4.J); `result` stays its own form, beside them, with payloads of any type. Older code still uses a tag plus fields reused by cast; `codegen.aipl`'s keyword and type tables are enums, but its AST is `compiler.aipl`'s generic `Node`.
- **Numeric gaps:** no `f32` literals, no `f32` conversions (only `i64`↔`f64`: `f64.convert_i64_s`, `i64.trunc_f64_s`, and the two reinterprets; go through `i64.extend_s` for `i32`), no float loads or stores on raw memory (keep floats in struct fields or `(arr f64)`, or move bits with `mem.load64` and `f64.reinterpret_i64`), no exponent notation in float literals, and loop bounds and addresses are `i32` only.

- **Friction for code generators** (audit D3). The pitfalls table (AIPL_SPEC.md 13) is long because the chosen spelling is often not what generators write first: `if` always needs an else (the repository has hundreds of empty `(block)` branches), `loop` includes its end bound, `and`/`or` take exactly two operands, and there is no implicit widening. Each is deliberate; each should be measured against what agents actually produce (docs/LAUNCH_CHECKLIST.md 5).
- **Errors are easy to ignore and lossy** (audit D5). A `result` or an `fs.*` status in statement position is discarded silently; every WASI errno becomes `-1`, so "not found" and "permission denied" look the same; library failures use sentinels (`len` -1).

### Memory

- **The built-ins never free.** `mem.alloc`, `new`, `arr.new`, `make`, and result cells come from a bump cursor that only grows. Freeable memory comes from the standard library (docs/design/HEAP_PLAN.md): `std/heap` frees single objects (checking double frees, wrong sizes or types, and writes after free), `std/arena` frees a region at once, and `std/alloc` makes either one `Allocator`, which the collections take. A heap's memory is reused but never returned to the system. Freeing object by object costs about 4x C's malloc/free on binarytrees.
- **Memory-safe outside `unsafe`, with stated limits** (AIPL_SPEC.md 3; parallel work without `unsafe` through `std/par` since 2026-10-16). Bounds checks, null checks, generation checks (use after free, since 2026-10-16), contracts, and the reserved-block guard run in every backend; the unchecked operations (`mem.*`, `ptr.cast`/`arr.cast`/`ptr.make`/`arr.make`, threads, atomics) are fenced inside `(unsafe ...)`; module visibility (AIPL_SPEC.md 11.1) keeps safe code from forging the standard library's descriptors. The limits: no data-race checking (threads are unsafe); after an arena reuses its memory, a stale pointer is caught unless the new data at its generation word happens to equal its old 32-bit generation; a fast arena catches a stale pointer only once its memory is reused; `ptr.cast` pointers are unchecked.
- **Reads from the reserved block 0–1023 are not checked.** This is deliberate: the block holds zeros and runtime cells.
- **Memory caps at 32768 pages (2 GiB)** in every backend (raised from 64 MiB on 2026-10-05; above 2 GiB addresses would be negative `i32`s). Allocation grows memory up to the cap automatically; a program that needs more fails at the first access past the end.

### Strings and I/O

- **No string operators.** There is no `+` on `str` (it once worked only in the VM; removed 2026-10-05): text is built with `std/buf`.
- **`sys.print` takes only `str`.** Numbers print through the standard library (`io.print_int`, `io.println_int`, `fmt.*`), and strings become byte slices with `str.from_str`. The library (AIPL_SPEC.md 12.6) has text I/O, `i32` formatting and parsing (`fmt`, `str.parse_int`), a string builder (`buf`), and generic collections (`vec`, `map`); floats print with a fixed number of decimals (`fmt.f64_fixed`, exact) or, in `std/json`, as the shortest digits that read back, and `float.from_decimal` parses them; still missing are a shortest-digits printer outside JSON, and sets.
- **VM `str` values are Rust strings, not pointers.** The VM lays out the first loaded module's literals at the same addresses as wasm, so `str.ptr` agrees; a string that is not one of those literals (from a module loaded later into the same VM) is copied onto the heap each time it is used.
- **`sys.exit` in the VM returns an error** (`sys.exit(N) requested`) instead of setting the process exit code.
- **Three kinds of text** (audit D7): `str` (an immutable literal), `(ptr str.Bytes)` (a slice), and `buf.Buf` (a builder), converted by hand (`str.from_str`, `buf.bytes`).
- **No dates or time zones, no sockets, no starting processes.** The runtime surface is preopened files and directories, stdio, args, environment, two clocks, and randomness: WASI preview 1 has no listening sockets and no way to start a process.

### Concurrency

Threads and atomics work in both backends (AIPL_SPEC.md 4.D). Gaps: compiled threads need a host that provides wasi-threads' `thread-spawn`, which wasmtime removed in v47, so they run under AIPL's own runner rather than the stock `wasmtime` CLI or a browser (the standards successor, shared-everything threads, is not implemented anywhere yet); each thread has its own file-descriptor table; concurrent `sys.print` lines can interleave; there are no condition variables, channels, or thread-local storage, only atomics and locks over shared memory.

### Self-hosted compiler capacities

`codegen.aipl` uses fixed-size tables (typed arrays in its `Cg`) and reports a compile error (never a miscompile) past them: 8192 functions, 16 parameters, 1024 locals per function, 255 structs of up to 64 fields, 31 distinct `call_ref` signatures, 255 nested blocks per function (AIPL_SPEC.md 6.4). String literals have the 1 MiB limit both backends share. The aipl command (the compiler and the native backend) is about 2,100 functions, generic instances included (2026-10-16). The Rust backend has none of these limits.

### Before "many modules from many authors" is safe

Still true from the original analysis, updated for what has landed:

1. **Shared data layouts have a type and a namespace.** Structs (P8) are reached through typed `(ptr S)` pointers and are qualified by module (`compiler.Node`), so two packages can each define `Node`, and module visibility (2026-10-14) keeps what a module does not give private to it.
2. **Duplication is still easy.** Imports make reuse possible, not the default; discoverability and lint tooling are missing.
3. **No versioning or dependency resolution.** Imports resolve by filename through the search path (AIPL_SPEC.md 11); there is no language version (deferred) and nothing handles package versions.
4. **Privacy stops at the source.** Visibility (2026-10-14) gives each module an API distinct from its implementation, but every function is still exported from the compiled wasm (audit S6).

### The longer-term ambition (a PDF viewer or browser in AIPL)

A PDF reader or rasteriser is a realistic first "real systems program" now that the standard library, control flow, and the compiled self-hosted toolchain exist. It needs binary parsing, vector rasterisation into a memory buffer, and file I/O, all of which compile to wasm today. A browser additionally needs a windowing/graphics host, which wasm alone cannot provide, plus HTML/CSS/layout/font/image/network stacks. Neither is near-term; the path runs through the sections above.
