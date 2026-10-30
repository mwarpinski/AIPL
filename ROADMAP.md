# Roadmap

Where AIPL is going and what it does not do yet, checked against the code
on 2026-10-27. [AIPL_SPEC.md](AIPL_SPEC.md) describes what it does;
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
2. **The standard library:** fetching over HTTP (docs/design/NET_PLAN.md,
   step 5: `connect`, `--allow-connect`, `http.fetch`); starting processes,
   which needs a host import of AIPL's own (WASI preview 1 has no way);
   time zones; a faster small-block path in `std/heap`.
3. **Rust out of the build path:** the stage-0 seed is done (2026-10-15,
   bootstrap/README.md): a checked-in native `aipl` builds the compiler on
   Linux x86-64 with no Rust, checked byte for byte against the Rust
   toolchain, which stays as the referee. Left: a WebAssembly seed for
   other platforms, and an installer that builds from the seed (DEVELOPING.md,
   "Direction"). The whole road, to a language that needs Rust only as the
   referee, is docs/design/aipl_standalone.md.
4. **Packages, after the launch:** a package registry with versioned
   packages, dependency resolution, and lock files (decided 2026-10-07;
   its command and name are open, e.g. folded into the `aipl` CLI as
   `aipl add`, `aipl build`). Language versioning comes with it.

Explicitly not now, each additive later rather than a rewrite: SIMD,
64-bit memory, exceptions (results cover errors). Later: language
versioning (once packages exist).

## What AIPL does not do yet

### Toolchain

- **Rust is needed to bootstrap off Linux x86-64.** The self-hosted compiler (`aipl_src/aipl.aipl`: driver.aipl's resolver, generics, checker, and code generator, plus the native backend) compiles every program to the Rust toolchain's bytes, itself included. On Linux x86-64 it builds from the checked-in seed (`bootstrap/`); elsewhere the first build takes the Rust toolchain ("Next" 3). The `aipl` CLI's `verify`, `eval`, and `test` use the Rust front end.
- **The execute bit** is set by the AIPL `aipl compile --exe` itself, through `fs.chmod` (an `aipl_sys_v1` import that WASI lacks; docs/design/SYS_PLAN.md). The Rust CLI builds the launcher bundle for platforms without a native backend. Native executables exist only for Linux x86-64; elsewhere the bundle is about 18 MB (wasmtime) and built for the host's OS and CPU.
- **Two of everything in the front end.** Resolver, generics, constants, parser, and checker exist in Rust (`src/`, used by the `aipl` CLI and the VM) and in AIPL (`aipl_src/`, used by the self-hosted compiler); tests hold each pair equal, message for message (AIPL_SPEC.md 6.4, 11). The Rust copies go when the CLI runs the AIPL ones.
- **Two copies drift unless held equal.** The fuzzer (`tools/fuzz.py`) checks the two front ends against each other on mutated programs; it finds no differences on 50,000 cases (2026-10-06), and CI runs 3,000 on every push. Only tests and the fuzzer hold the two equal, so a change to one copy must be made to the other.
- **The VM is slow.** It is a tree-walker: every variable lookup is a string-keyed hash lookup and every block clones its scope. Calls no longer copy the function body (audit B13), but the self-hosted compiler still takes seconds in the VM for work the same compiler compiled to wasm does in milliseconds. Use the compiled toolchain for anything large.
- **Contracts are checked at run time, never proven statically (audit B5).** `req`, `ens`, and `inv` (at entry and exit, since 2026-10-16) run in every backend; a compiled contract message shows no float or string values, and gives its position in the call chain after it rather than in the message.
- **No language or ABI versioning (deferred from P12).** No `:version` in modules and no version in the wasm output. Deliberately deferred until there are packages from different authors or a second toolchain; see the audit's P12 note.
- **Diagnostics report one error per function** (a definition's error alone), not every error in a function. Some common mistakes get generic messages: an `if` without an else is `Unexpected token parsing expression: ')'`.

### Language

- **No closures, by design.** `(ref f)` and `call_ref` give references to named functions; nothing captures variables, and a callback's data is a typed context passed as its first argument (`vec.sort_by_with`, `thread.spawn_with`, `alloc.custom`; AIPL_SPEC.md 4.G). Closures would allocate behind the program's back, enlarge every function value, and slow every call through one.
- **Generics are explicit.** Every use names its type arguments (no inference), and every call to an interface's function names the interface and the type. Templates are checked once with sealed type parameters (AIPL_SPEC.md 4.H). Not yet: interfaces with several type parameters, generic implementations (`(impl (Hash (ptr T)) ...)`, which need a rule against overlap first), and default functions in an interface.
- **Structs are behind pointers unless declared `(value)`.** `(ptr S)` and `(arr T)` are strictly typed. A struct declared `(value)` is held inline, copied where it goes, and never changed (at most 8 scalars), and an array of them is inline (AIPL_SPEC.md 4.E). A heap struct cannot be nested inside another by value (hold it by pointer), and packed records beyond that need `ptr.cast` arithmetic, as `compiler.aipl`'s `token_at` does.
- **No module-level variables, by design.** Modules keep state in a struct passed to every function (codegen.aipl's `Cg`), or in a `mem.alloc`'d block whose pointer lives in a runtime cell (codegen.aipl's keyword map, cell 16). Constants may be computed from other constants (`(const PAGES:i32 (* 4 PAGE_SIZE))`, since 2026-10-16; AIPL_SPEC.md 4.I), folded at compile time; there is no `f32` constant. Older code still uses zero-argument functions as constants (`(fn cc_e [] -> i32 4)` in `aipl_src/native/x64.aipl`).
- **8- and 16-bit types are for storage only** (since 2026-10-16): `u8 i8 u16 i16` struct fields and array elements, read as `i32`, and signed and 16-bit loads and stores (AIPL_SPEC.md 4.A). There is no arithmetic at those widths. Unsigned comparisons (`ltu lteu gtu gteu`) and overflow-checked arithmetic (`checked.add/sub/mul`) exist (AIPL_SPEC.md 8.2); there is no checked division or shift, and no form that reports overflow as a value rather than stopping.
- **Unions are values, and immutable.** A union value (AIPL_SPEC.md 4.J) is held by value, in up to 8 slots, inline in struct fields and arrays; a part that contains itself, or would need more slots, is held through a pointer to a struct. `make` is the only way to fill one: no in-place update, and no nested patterns (a `match` arm binds one level of fields; match again on a field). Generic unions and `std/option` exist; `result` stays its own form, beside them, with payloads of any type. Older code still uses a tag plus fields reused by cast; `codegen.aipl`'s keyword and type tables are enums, but its AST is `compiler.aipl`'s generic `Node`.
- **Numeric gaps:** `f32` literals (`1.5f32`) and the conversions between `f32`, `f64`, `i32`, and `i64` exist (since 2026-10-16), but not every pair (no unsigned conversions, no `f64.convert_i32_u`, no `i64.trunc_f64_u`); no float loads or stores on raw memory (keep floats in struct fields or arrays, or move bits with `mem.load64` and `f64.reinterpret_i64`); and loop bounds and addresses are `i32` only.

- **Friction for code generators** (audit D3). The pitfalls table (AIPL_SPEC.md 13) is long because the chosen spelling is often not what generators write first. Several were removed (a one-branch `if` is `when`, `loop` is `for`, `and`/`or` take two or more operands, a missing value is an `Option`, a struct's fields are named), the ten most common are at the top of the guide, and each that is a compile error has a message that names the fix. There is still no implicit widening, deliberately. What is left is measured by the A/B harness (tools/harness/, docs/LAUNCH_CHECKLIST.md 5).
- **A `result` or an `fs.*` status in statement position is discarded silently** (audit D5; `aipl verify` says nothing). The rest of D5 is fixed (2026-10-26, docs/design/LANGUAGE_DECISIONS.md item 2): a failed `fs.*` gives minus its WASI errno, the same number in every backend; std's reads, writes, streams, and CSV give an `io.Error` (not found, denied, is a directory, other); a missing `os.arg` or `os.env` is an `Option`. The searches (`str.find`, `vec.index_of`) still return -1 for not found.

### Memory

- **Only an allocator that frees, frees.** `mem.alloc` and `alloc.forever` come from a bump cursor that only grows, and a `str` made at run time is never freed. `new` and `arr.new` take the allocator to make the object from (AIPL_SPEC.md 4.E): `std/heap` frees single objects (checking double frees, wrong sizes or types, and writes after free), `std/arena` frees a region at once, and `std/alloc` makes either one `Allocator`, which the collections take (docs/design/HEAP_PLAN.md). A heap's memory is reused but never returned to the system.
- **Memory-safe outside `unsafe`, with stated limits** (AIPL_SPEC.md 3; parallel work without `unsafe` through `std/par` since 2026-10-16). Bounds checks, null checks, generation checks (use after free, since 2026-10-16), contracts, and the reserved-block guard run in every backend; the unchecked operations (`mem.*`, `ptr.cast`/`arr.cast`/`ptr.make`/`arr.make`, threads, atomics) are fenced inside `(unsafe ...)`; module visibility (AIPL_SPEC.md 11.1) keeps safe code from forging the standard library's descriptors. The limits: no data-race checking (threads are unsafe); after an arena reuses its memory, a stale pointer is caught unless the new data at its generation word happens to equal its old 32-bit generation; a fast arena catches a stale pointer only once its memory is reused; `ptr.cast` pointers are unchecked.
- **Reads from the reserved block 0–1023 are not checked.** This is deliberate: the block holds zeros and runtime cells.
- **Memory caps at 32768 pages (2 GiB)** in every backend, or at `--max-memory`'s (above 2 GiB addresses would be negative `i32`s). Allocation grows memory up to the cap automatically; a program that needs more stops at the `mem.alloc` with `Out of memory: the limit is N MiB`.

### Strings and I/O

- **No string operators.** There is no `+` on `str` (it once worked only in the VM; removed 2026-10-05), and no `eq` (since 2026-10-16: a string made at run time has its own address, so `eq` would differ between the VM and compiled code). Functions do both: `str.concat`, `str.same`, `(cmp.Ord.compare str)`.
- **`sys.print` takes only `str`.** Numbers print through the format forms (`(println "n = {n}")`), and strings become byte slices with `str.from_str`. The library (AIPL_SPEC.md 12.6) has text I/O, `i32` formatting and parsing (`fmt`, `str.parse_int`), a string builder (`buf`), and generic collections (`vec`, `map`); floats print with a fixed number of decimals (`fmt.f64_fixed`, exact) or, in `std/json`, as the shortest digits that read back, and `float.from_decimal` parses them; still missing are a shortest-digits printer outside JSON, and sets.
- **VM `str` values are Rust strings, not pointers.** The VM lays out the first loaded module's literals at the same addresses as wasm, so `str.ptr` agrees; a string that is not one of those literals (from a module loaded later into the same VM) is copied onto the heap each time it is used.
- **`sys.exit` in the VM returns an error** (`sys.exit(N) requested`) to its caller; the `aipl eval` command turns it into the exit status, as compiled code has.
- **Three kinds of text** (audit D7): `str` (immutable), `str.Bytes` (a slice), and `buf.Buf` (a builder), converted by calling a function each way (`str.from_str`, `str.from_bytes`, `buf.bytes`, `buf.to_str`; AIPL_SPEC.md 4.B says which to use when). A `str` made at run time is never freed.
- **No time zones.** `std/date` has UTC dates and ISO 8601; there is no time-zone database. The runtime surface is preopened files and directories, stdio, args, environment, two clocks, randomness, and TCP sockets (AIPL's own host interface beside WASI, granted on the command line: `std/net`, `std/serve`; docs/design/NET_PLAN.md). WASI preview 1 has no way to start a process; AIPL's host interface has `proc.spawn` and its operations (`unsafe`, granted by `--allow-run`; docs/design/SYS_PLAN.md), and `std/proc` is the safe library over them.

### Concurrency

Threads and atomics work in both backends (AIPL_SPEC.md 4.D). Gaps: compiled threads need a host that provides wasi-threads' `thread-spawn`, which wasmtime removed in v47, so they run under AIPL's own runner rather than the stock `wasmtime` CLI or a browser (the standards successor, shared-everything threads, is not implemented anywhere yet); each thread has its own file-descriptor table; concurrent `sys.print` lines can interleave; there are no condition variables, channels, or thread-local storage, only atomics and locks over shared memory.

### Self-hosted compiler capacities

`codegen.aipl`'s tables (functions, parameters, locals, structs, fields, unions, variants, `call_ref` signatures, nested blocks) grow as the Rust backend's vectors do (since 2026-10-26; before that they were fixed at 8192, 16, 1024, 255, 64, and 31, and a program past one got a compile error, never a miscompile): what is left is the memory the compile has (compile error 90 past 32768 pages) and the 1 MiB limit on string literals that both backends share (AIPL_SPEC.md 6.4). The aipl command (the compiler and the native backend) is 3,366 functions, generic instances included (2026-10-27, unoptimized).

### Before "many modules from many authors" is safe

Still true from the original analysis, updated for what has landed:

1. **Shared data layouts have a type and a namespace.** Structs (P8) are reached through typed `(ptr S)` pointers and are qualified by module (`compiler.Node`), so two packages can each define `Node`, and module visibility (2026-10-14) keeps what a module does not give private to it.
2. **Duplication is still easy.** Imports make reuse possible, not the default; discoverability and lint tooling are missing.
3. **No versioning or dependency resolution.** Imports resolve by filename through the search path (AIPL_SPEC.md 11); there is no language version (deferred) and nothing handles package versions.
4. **Privacy stops at the source.** Visibility (2026-10-14) gives each module an API distinct from its implementation, but every function is still exported from the compiled wasm (audit S6).

### The longer-term ambition (a PDF viewer or browser in AIPL)

A PDF reader or rasteriser is a realistic first "real systems program" now that the standard library, control flow, and the compiled self-hosted toolchain exist. It needs binary parsing, vector rasterisation into a memory buffer, and file I/O, all of which compile to wasm today. A browser additionally needs a windowing/graphics host, which wasm alone cannot provide, plus HTML/CSS/layout/font/image/network stacks. Neither is near-term; the path runs through the sections above.

### After launch: AI-SQL, a query language written in AIPL

An idea, not a plan (2026-10-19): a small SQL in AIPL, as AIPL is written
with Rust as its bootstrap, to show the language building a real tool in
the data domain. It would use most of std (csv for tables, map for GROUP
BY and hash joins, vec.sort_by for ORDER BY, regex for LIKE, stats for
aggregates, `(no_leak)` per query in a server mode) and ship as one native
binary built by AIPL. "AI" because it would drop SQL's traps the way AIPL
drops C's: typed columns and no implicit casts, qualified names in joins,
output in a stated order, a missing value as an explicit option rather
than NULL's three-valued logic, and errors that say what to write. First
a CSV query tool, its answers checked against SQLite's on the same data;
the full language only if that lands. Not before launch: it would pull
focus.

**Timeline, re-estimated 2026-10-20.** An agent wrote the CSV query tool
(about 4,700 lines of AIPL, outside this repository in ../AI-SQL) in an
hour, and a second model's review and the fixes took fifteen minutes.
That first draft also had silent wrong answers (trailing input ignored,
so `LIMTI 2` dropped the limit; float join keys colliding; `none`
handled two ways), found only by reading and running it. So writing is
no longer the schedule; earning trust in the answers is. In calendar
time, with agents writing and a person deciding:

- **The CSV query tool, trustworthy: days.** What is left is evidence:
  differential testing against SQLite on generated queries as well as
  generated data (in the manner of SQLancer), the semantics written
  down and decided (none, typing, ordering), and a cold review.
- **The full language** (INSERT, UPDATE, DELETE, subqueries, window
  functions, typed DDL): **one to two weeks**, each part landing with
  its differential tests.
- **The storage engine: weeks to a couple of months**, set by testing,
  not code: crash and power-loss simulation, fault injection, and
  concurrent access, against a durable format that must stay readable.
  It needs host operations first (fd_sync, file locks, random-access
  I/O), each a few days in three backends and both toolchains.

The order is unchanged: the CSV query tool, AI-SQL, then the engine,
each worth having alone.

### After launch: a 64-bit target

Programs are limited to 2 GiB of memory (AIPL_SPEC.md 4.A: below it,
every address is a non-negative i32). More would come as a second
compilation target, not a second language (decided 2026-10-19): the
same source compiled for wasm's 64-bit memory (memory64), as C and Rust
compile one source for 32- and 64-bit machines. The checker, the
language, and nearly every program stay as they are; what changes is the
two code generators (a mode in each), the native backend's translation,
the layout of a checked pointer (an address and a generation no longer
share 64 bits), and std's unsafe code that handles raw addresses, which
would come with an address-sized integer type (decided 2026-10-26: not before there is a 64-bit target; docs/FAQ.md says why).
Wanted when a program needs more than 2 GiB in memory; a lower limit
(`--max-memory`) comes first.

### Not planned: TLS written in AIPL

HTTPS comes from rustls in the host (docs/design/NET_PLAN.md decision 4):
the VM, aipl-run, and the launcher-bundled executables; a native
executable refuses TLS at compile time, with the ways out (`--target
wasm`, or a TLS proxy in front, as most servers are deployed). A TLS
written in AIPL would add only HTTPS inside a native executable with no
proxy, and hand-written cryptography is not fit for real traffic without
an outside audit, which the project will not pay for (decided
2026-10-20). Revisit if AIPL gains the kind of use that grant-funded
audits go to.

An idea for then, useful on its own before it: **secret types**, a
qualifier the checker enforces (as it proves `(no_leak)`) so that a
secret value never decides a branch, an index, a loop count, or a
division, and leaves only through an explicit declassify; with `select`
in both code generators and `cmov` in the native backend, and tests that
compare the branches and addresses a compiled routine takes for
different secrets. That would make constant time a property the
compiler proves rather than a matter of expert review (as CT-Wasm,
Jasmin, and FaCT do in research).
