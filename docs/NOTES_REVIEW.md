# docs/notes, checked claim by claim

docs/notes holds an outside model's review of AIPL (argon, 2026-10-16
and 17: assessment, final_thoughts, freedom, kernel_notes, painpoints,
pitfalls, speed-ups, arm64_build; the `.txt` files are the originals,
fuller than the `.md` versions). Readers will point their own models at
this repository, so every claim in the notes gets a verdict here, with
the evidence: a command, a test, or file:line. Checked 2026-10-18 and 19
against commit 5a0b934 and the L6 work after it.

Verdicts: **true** (and fixed, or open with a launch checklist item),
**partly**, **wrong** (with what shows it), **by design** (with the
reason), **external** (a claim about another project or about hardware,
not about this repository), **unchecked** (with why).

## assessment

| Claim | Verdict | Evidence and action |
|---|---|---|
| 15,745 lines of Rust and 26,133 of AIPL | true then, larger now | 17,419 and 37,956 on 2026-10-18 (`cat src/*.rs src/compiler/*.rs src/bin/*.rs \| wc -l`; the same for aipl_src, native, std). The standard library grew most. |
| lower.aipl 2,328 lines, checker.aipl 2,251, codegen.aipl 5,861, generics.aipl 1,562 | true then | lower 2,328 and generics 1,562 still; checker 2,530 and codegen 5,963 now. |
| The code was AI-generated and largely zero-shotted | external | The owner's account of how it was built; nothing in the repository can show it either way. |
| The self-hosted compiler parses every program twice: front.aipl's typed AST is thrown away and codegen.aipl re-tokenizes and re-parses the text into compiler.Node and re-infers types | true, and understated | driver.aipl:60-89: `front.check` (for archetypes), `front.check_formats` on the flattened text, `fmtlower.lower` text to text, then `codegen.compile_mapped`/`compile_lowered` run `compiler.tokenize` and `parse_ast` (codegen.aipl:5373-5375) and infer types again (`node_type`, codegen.aipl:2743). Open: having codegen consume the checker's typed AST would remove a parse and the second inference; a large refactor of both toolchains, which must stay byte for byte equal. |
| `Cg` has 33 fields, grown one feature at a time | true (32) | `struct Cg` in codegen.aipl: 32 fields. A size, not a bug; it shrinks with the item above. |
| Both frontends print the program back to text and re-parse it between passes | partly | The self-hosted toolchain does (resolver, generics, and fmtlower pass text; origins.aipl maps positions back). The Rust frontend passes `Sx` trees and tokens (src/resolver.rs:249, `parse_item`), not text. |
| The early audit found fabricated stubs and tests that only checked "returned a number" | true | docs/history/AUDIT_2026-10.md B7 (line 278) and U6 (line 315); fixed since (attic/ removed, launch checklist section 1). |

## pitfalls

| Claim | Verdict | Evidence and action |
|---|---|---|
| Four-way differential testing; 50,000 mutation fuzz cases, 25,000 execution cases | true | Launch checklist section 1 (fuzz.py and run_fuzz.py entries); tests/test_differential.rs. |
| The native backend runs within 0.8x to 2.0x of Cranelift | true | docs/FAQ.md line 50, from docs/BENCHMARKS.md. |
| Generation checks cost 1.5x to 2.5x until loop versioning hoisted them | partly | Checking every access cost 1.38 (fannkuch) to 2.48 (spectralnorm) natively; with versioned loops most benchmarks pay 0.95 to 1.07, binarytrees 1.94 (memory: a node grew from 8 to 32 bytes) and knucleotide 1.29 (docs/BENCHMARKS.md, "Generation checks"). Our own docs disagree: AIPL_SPEC.md 4.E ("Generation checks run everywhere") says "2-8% on most benchmarks, and 22% under wasmtime on nbody", which did not match BENCHMARKS.md's 2026-10-16 table (it predated value structs). Fixed 2026-10-19: the spec now quotes that table. |
| `(new S)` and `(arr.new T n)` are never freed | true, by design for now | AIPL_SPEC.md 4.E ("new and arr.new objects are never freed"). Data meant to be freed comes from an allocator (`std/alloc`), as the prompt guide says. |
| Run-time `str` values are never freed | true, by design | AIPL_SPEC.md 4.B (the `str` row: "immutable UTF-8, never freed"). |
| Every `str.from_str` and `str.slice` leaks a 16-byte `str.Bytes` | true | std/str.aipl:10-14 (`bytes` makes one with `new`): 8 bytes of struct and 8 of header. `buf.bytes` does the same, and std/regex and std/deflate called it in loops until 2026-10-18. Fixed 2026-10-19 (MEMORY_PLAN.md M1): `str.Bytes` is a value struct holding a checked array, so a view allocates nothing; it also closed a hole the note missed (a view of freed memory read whatever reused it). docs/design/AGENT_FRICTION.md item 7. |
| `io.print_int` allocates 11 bytes on every call, never freed | true | std/io.aipl:22-24 (and 21 bytes for `print_i64`, whose comment says so). Open since 2026-10-16: the fix needs an allocator import or a per-thread scratch buffer. |
| The standard library cannot stream files past 2 GiB | wrong now | std/stream (2026-10-18, after the note): a native program read a 3 GiB file through `stream.read` in 64 KiB pieces, 3,221,225,472 bytes, no error, 10.8 s. Memory itself stays limited to 2 GiB (wasm32). |
| Full generic type arguments on every call and match arm, though only one could type-check | true, by design | The explicit-redundancy rule (the owner's choice, 2026-10-18: not to be inferred). final_thoughts makes the opposite case: the restated types are local anchors for a model. |
| `io.read_file`, `os.arg`, and `os.env` return `len -1` where the prompt guide says to use `Option` | true | std/io.aipl:55-56, std/os.aipl:42-48: each returns a `str.Bytes` with `len -1`. **Open: return `(option.Option ...)` or a result**; std/stream already returns a result for `open`. These are the oldest std functions, used by the compiler, so the change waits for the seed refresh (launch checklist section 6). |
| A name keeps one type per function, so sibling match arms cannot both bind `v` | true | AIPL_SPEC.md 7.1 and section 13; hit 7 times on 2026-10-18 (docs/design/AGENT_FRICTION.md item 2). **Open: scope match binders to their arm.** "~30 lines to remove" is unchecked. |
| `(loop i 0 (- n 1) 1 ...)` has an inclusive bound, against every other language | fixed | `(for i 0 n ...)`, exclusive, 2026-10-18 (commit 026f4aa, AIPL_SPEC.md 4.M). `loop` stays as it was. |
| `(ok:E v)` and `(err:T e)` restate a type the return position already gives | true, by design | AIPL_SPEC.md 4.F: "every result is made with its whole type written". No such mistake in about 9,000 lines written on 2026-10-18. |
| Two frontends and two codegens double the cost of each feature | true; keeping them is by design | Each 2026-10-18 feature (`when`, `for`, variadic `and`/`or`, `dbg`, tests) was written twice. The Rust side is the independent compiler that makes the seed trustworthy (bootstrap/README.md, "Why the Rust compiler stays": diverse double compiling); retiring it loses that. Open for the owner: how much new surface the Rust side must mirror, given the plan to move the toolchain off Rust. |

## final_thoughts

| Claim | Verdict | Evidence and action |
|---|---|---|
| Writing codegen_opt.aipl (about 2,350 lines) hit only 2 compile errors | unchecked | The author's own report; codegen_opt.aipl is not in git yet, so its history cannot be checked. |
| The errors were "'=' is not an operator or form; use 'eq' for equality ..." and "local 'it' is already declared in this function", which say exactly what to do | wrong | Neither text exists in either toolchain or in git history (`git log -S` finds them only in docs/notes). Today `(= x 1)` gives "Unknown op/keyword: =" and a repeated `let` gives "Cannot shadow existing variable 'it'". **Open: make "Unknown op/keyword: =" suggest `eq`** (and `==`, `!=`, `<`, `&&` their AIPL names): the note praised a message that would help, and does not exist. |
| Restated types are local anchors that save a model from tracing inference | by design (agrees) | The reason the explicit style was chosen. |
| `aipl verify` passed but native code crashed: a `(block (call set_compile_error ...) (block))` simplified by codegen_opt unbalanced the stack | wrong as a gap in verify; true as a codegen_opt bug | The construct is real (codegen.aipl:1531, 2536, 2967). Removing the trailing `(block)` turns a void branch into an i32 one, which the checker rejects ("If branch type mismatch: then is i32, else is void", tested 2026-10-18); codegen_opt rewrote the program after checking and never checked its output. **Open, for codegen_opt's integration behind `--opt`: type-check the optimized program before code generation** (milliseconds), so a rewrite cannot produce code the checker would refuse. |
| Reject any non-void expression in statement position without an explicit `(drop e)` | by design (keep) | A statement's value is dropped, correctly in every backend (a bare `(call f i)` tested in the VM, aipl-run, and natively, 2026-10-18; tests/test_opcode_conformance.rs runs every op in statement position). The real friction is `when` around a value, launch checklist section 2. |
| Function-wide names make a model track every temporary in a function | true | As pitfalls above. |
| Long runs of `)))` from nested binary `and`/`or` | fixed for programs | Variadic `and`/`or`, 2026-10-18. The compiler's own source waits for the seed refresh (launch checklist section 6). |
| A heap struct made with `new` and `put`s can miss a field silently | true | **Open: launch checklist section 6**, "Making a heap struct in one form". (The note writes `make` as `(make S [f1 v1 ...])`; it is positional, `(make S v1 v2)`.) |

## freedom

| Claim | Verdict | Evidence and action |
|---|---|---|
| The Rust CLI hardcodes `CARGO_MANIFEST_DIR/aipl_src/std` | true | src/resolver.rs, `find_module_file` (line 994 now; the note says 880). It also ignores `AIPL_STD`. **Open: launch checklist section 4**, "A binary that works wherever it is". install.sh works only because it builds in place. |
| The self-hosted CLI falls back to `./aipl_src/std/` | true | aipl.aipl:76, driver.aipl:155, doc.aipl:618. Same item. |
| The self-hosted CLI has only `compile` and `--version` | partly | It has `doc` too since 2026-10-18; `verify` and `test` are missing. Same item. |
| `--exe` output needs `chmod +x` | partly | Only from the self-hosted CLI (WASI cannot set file modes; aipl.aipl:16 says so); the Rust CLI sets 0755 (src/main.rs:294). |
| `-o` is required | true | A convenience, not a defect. |
| The standard library is 115 KB in 19 files | true then | 460 KB in 37 files on 2026-10-18. |

## painpoints

| Claim | Verdict | Evidence and action |
|---|---|---|
| S-expressions make source-to-source passes easy; explicit conversions allow rewriting without inference; no hidden control flow | true (agrees) | codegen_opt.aipl's printer, equality, and substitution are short recursive functions. |
| Matching tree shapes needs chains of onode_count, onode_nth, is_form | true | codegen_opt.aipl:365 on. **Open: launch checklist section 6**, a pattern matcher over ONode, as a library. |
| Binary `and`/`or` | fixed for programs | As above. |
| Repeating generic type arguments | by design | As above (the owner's call, 2026-10-18). |
| `new` + `put` cannot be checked for a missing field | true | As above. |
| Statement-position values cause stack mismatches natively | wrong now | As above: the checker refuses the mismatched `if`, and a dropped value is correct in every backend. |
| Large literals truncate to 32 bits when re-printed | wrong for the language; unchecked for codegen_opt | An unsuffixed literal past 32 bits is an error suggesting the `i64` form (AIPL_SPEC.md section 2); any truncation is in codegen_opt's printer. **Open: launch checklist section 6.** |

## speed-ups

| Claim | Verdict | Evidence and action |
|---|---|---|
| The benchmark table (1.2x binarytrees and mandelbrot, 3.0x nbody, 4.0x spectralnorm, 17.4x knucleotide, 10.4x pidigits) | true | docs/BENCHMARKS.md lines 32-38, as quoted. |
| knucleotide's gap is per-byte `str.byte_at` calls whose check cannot leave the loop | true | docs/BENCHMARKS.md line 58 (1.29). |
| codegen_opt's inlining made spectralnorm 1.23x and knucleotide 1.19x faster | unchecked | Measured by the note's author; the 2026-10-17 run here gave 0.59 to 1.04 on a loaded machine, so it needs measuring again on a quiet one before any claim. |
| Every build is whole-program (like `-flto`) | true | The resolver merges the program into one module. |
| Type-based alias analysis would be sound, since casts need `unsafe` | true, with a condition | `ptr.cast`, `arr.cast`, `ptr.make`, `arr.make`, and raw `mem.*` need an unsafe context (AIPL_SPEC.md 3, "Unchecked operations"); sound for code without them, which the optimizer can tell. |
| A checked `req (lt i (arr.len a))` lets every `a[i]` in the body skip its bounds check | partly | Only while `i` keeps its value: parameters can be reassigned (`(set! i (+ i 100))` after the `req` compiles, and the access then fails its check, tested 2026-10-18). Array lengths cannot change. The optimizer must also see that `i` is not assigned. |
| `-ffast-math` must never be offered | by design (agrees) | It would break the VM-against-native comparison. |
| binarytrees is close to C because arenas beat malloc | partly | docs/BENCHMARKS.md (line 42) itself says this pairing is not like for like. |
| pidigits needs a 64x64 to 128-bit multiply | unchecked | Plausible (the C entry uses GMP); belongs to NATIVE_SPEED_PLAN.md. |
| An SSA tier with linear-scan allocation would reach about 1.2x to 1.5x of gcc -O2 | unchecked | An estimate for a plan; NATIVE_SPEED_PLAN.md is where it would be tested. |

## kernel_notes

| Claim | Verdict | Evidence and action |
|---|---|---|
| Native executables use about 20 Linux system calls | true | aipl_src/native/runtime.aipl, wasi.aipl, native.aipl: write, read, mmap, mprotect, openat, openat2, close, lseek, getdents64, newfstatat, unlinkat, clock_gettime, getrandom, clone, futex, rt_sigaction, exit_group, and a few more. |
| Out-of-bounds memory traps rely on Linux's SIGSEGV | true | runtime.aipl:294-350 (the handler, installed with rt_sigaction). |
| Addresses are 32-bit offsets from one base register (r15) | true | lower.aipl:31-37; wasm32. |
| QEMU's Multiboot loader refuses 64-bit ELF; Limine loads one; Biscuit (Go) ran nginx at about 90% of Linux | external | About QEMU, Limine, and Biscuit (OSDI 2018), not this repository; the note corrects its own first answer here. The aiOS idea is post-launch (memory: aios-vision). |
| Hardware instructions would be Wasm imports the native backend replaces, as WASI imports become system calls | true as a design | wasi.aipl does that for WASI. No kernel work is planned before launch. |

## arm64_build

| Claim | Verdict | Evidence and action |
|---|---|---|
| `--exe --target wasm` appends the module to aipl-run with a 16-byte AIPLEXE1 trailer | true then; since 2026-10-20 a 20-byte AIPLEXE2 trailer after the module and its --listen grants | src/main.rs write_executable, src/bin/aipl_run.rs MAGIC. |
| GitHub's macos-14 and ubuntu-24.04-arm runners can build ARM64 releases | external | GitHub's offering; relevant to prebuilt binaries (launch checklist section 4). |
| An ARM64 backend would reuse wasm_reader.aipl unchanged; fixed 32-bit instructions are simpler | true as a design | The native backend reads WebAssembly, so a second target shares the reader. |
| ARM64's `sdiv` does not trap on division by zero or INT_MIN / -1, unlike x86 | true about the hardware, wrong as an advantage | WebAssembly requires those traps, so an ARM64 backend has to add the checks that x86's `idiv` gives for free. |
| 31 registers mean normal functions never spill | partly | Fewer spills; "never" depends on the function. |

## sonnet (2026-10-19)

A second outside review: Sonnet, after reading AI-SQL (4,100 lines of
AIPL Gemini wrote), wrote 11 programs from the prompt guide alone and
reported on the language's duplicate forms, its docs, and syntax
proposals, against the kit of 2026-10-19 (173cf89 or a46fa09). Checked
2026-10-20 against features/net (c661bdd and after). The owner's
decisions on it: docs/design/CANONICAL_PLAN.md.

| Claim | Verdict | Evidence and action |
|---|---|---|
| 9 of 11 first-try programs type-checked; both failures guessed an array literal (`arr.of`) | true | No array literal exists: `(arr.of i32 1 2 3)` gives `Unknown op/keyword: arr.of`. CANONICAL_PLAN.md step 5 adds `(arr.of T a x...)`. |
| Gemini used `loop` 96 times and `for` never; `loop`'s inclusive end is a known trap | true | The repository itself has 736 `loop`s and 278 `for`s. Owner's decision: retire `loop` (CANONICAL_PLAN.md step 3). |
| `(new S a)` and `(call (alloc.create S) a)` are two ways to make a struct | partly | `new` is the documented form, and the resolver writes `alloc.create` for it (AIPL_SPEC.md 4.E); the repository calls `alloc.create` directly twice. The review itself traces Gemini's use to its prompt. A template taking `S` as a type argument needs `alloc.create`; no change. |
| Interpolation vs `buf.push_str` chains, `defer`, `try` vs `match_result`, nested vs variadic `and`/`or` are duplicate forms | partly | Not duplicates: `try` passes an error up and `match_result` handles it; `defer` has no other form; nesting two-operand `and`s is composition. Building text has two forms for two jobs (`format_to` for a template, `buf.push_str` for one piece). The guide should say which to use when (CANONICAL_PLAN.md step 6). |
| `when` and `(if c x (block))` are the same | true | 426 `when`s and 2,382 `(block))` in the repository. Owner's decision: `when` only (CANONICAL_PLAN.md step 4). |
| The guide says `and`/`or` take two or more operands (rule 15) and exactly two (checklist) | true | PROMPT_GUIDE_FOR_AIS.md lines 30 and 279; the checklist is wrong (CANONICAL_PLAN.md step 1). |
| Two copies of the spec and the guide, one stale | external | Both copies were in the AI-SQL project folder; the kit (tools/make_kit.sh) ships one of each. |
| No version stamp in the spec or guide | true | CANONICAL_PLAN.md step 1. |
| The pitfall table documents most of the mistakes models make, and is too long to absorb | true | 62 rows (AIPL_SPEC.md section 13). The most-hit ones move to the top of the guide (step 1), and each that is a compile error gets a message naming the fix (step 2): today `(f 1)` gives `Unknown op/keyword: f`, `(let x 5)` `Expected ':', got '5'`, `(if c x)` `Unexpected token parsing expression: ')'`, and a union variant without a field name `Expected ':', got ')'`. |
| `(no_leak)` code cannot sort: `vec.sort_by`, `sort_by_with`, and `stats.median` call through function references | fixed since | MEMORY_PLAN.md decision 9 and `vec.sort` over `cmp.Ord` (2026-10-20): a `(no_leak)` function calling `vec.sort_by` with a `(no_leak)` comparator, `vec.sort`, and `stats.median` type-checks (tried 2026-10-20). |
| AI-SQL hand-wrote a float parser std/text already has | true | The guide's pointer to `aipl doc --find` is near its end (line 289); it moves to the top (step 1). |
| `+`, `-`, `*` wrap silently | true, by design for now | WebAssembly's semantics (AIPL_SPEC.md's first line); `checked.*` stops on overflow. Whether checked should be the default is its own question (speed, and a departure from wasm), not part of this batch. |
| Syntax proposals: inferred `let` types, inferred generic arguments, `(f x)` without `call`, `p.f`, a bytes literal; together 19-22% fewer tokens | unchecked (estimates), decided by measurement | The review's own estimates, on lexical tokens. Inferred generic arguments were declined on 2026-10-18 (explicit redundancy, above). Owner's decision: build the review's A/B harness and test each one at a time, across model families (CANONICAL_PLAN.md step 7). |
| Match arms repeat the full type, `((option.Option T) some [v] ...)` | true, by design | The explicit-redundancy rule, as for generic arguments. |

AI-SQL's own mistake log (AI-SQL/mistake_log.md, Gemini's) is a source
for step 2's messages. One entry is wrong: **Mistake 1** quotes `Union
'Expr' exceeds maximum allowed variant count (8 variants max per
union)`, an error neither compiler has (no such text in src/ or
aipl_src/); a union of 11 variants type-checks and runs in the VM and
natively (tried 2026-10-20). Its other entries are checked as step 2
uses them.
