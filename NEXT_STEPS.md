# AIPL: Review, Assessment & Next Steps

This document summarizes the current state of AIPL (as of late October 2026), evaluating what is working exceptionally well, what is missing, and concrete prioritized recommendations for the language, compiler, runtime, and documentation.

---

## 1. What Is Working Exceptionally Well

* **Uncompromising LLM-First Language Design (`CANONICAL_PLAN.md`, `LANGUAGE_DECISIONS.md`)**:
  * **One canonical form per concept**: Retiring inclusive `loop` in favor of exclusive `for`, requiring `when` for one-armed conditionals, requiring `[field:val ...]` on `new`/`make`, and requiring explicit `(zero T)` eliminate an entire class of hallucinated positional arguments and off-by-one errors.
  * **Actionable diagnostics & error recovery (`AGENT_FRICTION.md`)**: Rather than generic parse errors, the lexer and typechecker catch C/Rust/Python habits (`=`, `==`, `!=`, `&&`, bare `true`, `{x}` without quotes, `arr.get` vs `get`, `vec.get` vs `vec.at`) and emit exact replacement expressions using Levenshtein/prefix matching (`aipl_src/suggest.aipl`).
* **Memory Safety Without Borrow-Checker Fights (`MEMORY_PLAN.md`)**:
  * Pairing generational references (`ptr`, `arr`, and 3-slot `str.Bytes` views) with Zig-style explicit allocators (`heap`, `arena`, `alloc.forever`) and whole-program static leak verification (`(no_leak)` in `src/leaks.rs` and `aipl_src/leaks.aipl`) hits an optimal balance for agents: deterministic use-after-free traps with exact source locations, flat memory over 1M+ HTTP requests (`std/serve.aipl`), and zero lifetime-annotation churn.
* **"Trust Nothing" Verification Architecture (`bootstrap/README.md`)**:
  * Holding the Rust compiler (`src/`) and self-hosted AIPL compiler (`aipl_src/`) byte-for-byte identical across two independent implementations (Wheeler's Diverse Double-Compiling), backed by two checked-in stage-0 seeds (`bootstrap/aipl-linux-x86_64` and `bootstrap/aipl-linux-aarch64`), 3-way execution parity (AST VM, Wasm, native ELF), `tools/fuzz.py`, `tools/run_fuzz.py`, and `tools/opt_diff.py` provides remarkable stability.
* **Standalone CLI & AArch64 Milestones (Oct 28–30, 2026)**:
  * `aipl_src/aipl.aipl` now implements `verify`, `run` (`.aipl` and `.wasm`), `test`, `watch`, `serve --fresh`, `eval`, and `doc` without requiring Cargo/Rust at runtime.
  * Native Linux AArch64 backend (`arm64.aipl`, `lower_arm64.aipl`, `runtime_arm64.aipl`) is functional and has achieved self-hosting fixed-point bootstrap.

---

## 2. What Is Obviously Missing

### Priority 1: Empirical Proof of the Core Thesis (Zero A/B Harness Runs)
* **Status**: The 26-task A/B evaluation harness (`tools/harness/harness.py`, `tools/harness/README.md`) is built, but `tools/harness/results/` has zero recorded model runs.
* **Impact**: Beyond proving the core thesis ("AI agents write correct code in fewer turns in AIPL than in Python or Rust") for public launch (`LAUNCH_CHECKLIST.md`), `CANONICAL_PLAN.md` Step 7 explicitly defers major syntax proposals (inferred `let` types, dropping `call`, dropping `ret`, merging `get`/`at`) until they can be empirically tested against real models.
* **Action**: Run the harness across multiple model families (Claude 3.5 Sonnet, GPT-4o, Gemini 1.5 Pro) on Python, Rust, and AIPL. Record and publish the results.

### Priority 2: Outbound Networking (`net.connect`, `http.fetch`, DNS, TLS)
* **Status**: AIPL has a production-capable HTTP/1.1 server (`std/serve`, `std/http`, `std/net`), but cannot initiate outbound network requests.
* **Impact**: Steps 5–7 of `NET_PLAN.md` (`net.connect`, `--allow-connect`, DNS resolution, `http.fetch`, and host TLS in `aipl-run`/VM) are unbuilt. For AI agents building real-world automation, calling external HTTP APIs is even more common than serving endpoints.
* **Action**: Implement native/WASI non-blocking client connection logic, DNS resolution (`sys.getaddrinfo`), and `http.fetch`.

### Priority 3: AArch64 Native Parity & Register Allocation
* **Status**: Native AArch64 works and self-hosts, but remains a baseline stack-frame compiler (`ARM64_PLAN.md`).
* **Impact**:
  * Every value and temporary sits in a 16-byte stack frame slot. As a result, `bootstrap/aipl-linux-aarch64` is 7.6 MB (vs 4.0 MB for x86-64) and self-compilation takes ~20s (vs ~3s for x86-64).
  * Missing runtime pieces (NA11): `procs_arm64` (`--allow-run` / `std/proc`), `sockets_arm64`/`fresh_arm64` (`--listen` / `--serve`), native threads (`clone`/`futex`), and `--sandbox` (`seccomp` for AArch64).
* **Action**: Map first 8 locals to `x19`–`x26` / `d8`–`d15` in `lower_arm64.aipl` (NA10), and complete runtime syscall stubs (NA11).

### Priority 4: Standard Library & Subprocess Completeness
* **Transcendental Math (`std/math.aipl`)**:
  * `std/math` lacks `ln`, `exp`, `pow`, `log10`, `sin`, `cos`, `tan`, and `atan2`.
  * `std/random.aipl` already contains a private pure-AIPL IEEE-754 port of fdlibm's natural log (`ln_f64`). These functions should be ported and exposed in `std/math`.
* **Subprocess Management (`SYS_PLAN.md` D7)**:
  * `std/proc.aipl` cannot terminate/kill a hung process, lacks execution timeouts on `proc.run`, and lacks custom `cwd` and `env` parameters. Native process/socket handle tables also need thread-safe locking.

---

## 3. Concrete Language & Compiler Improvements

### 1. Support Escaped String Literals Inside `{...}` Format Placeholders
* **Problem**: The single most frequent syntax error made by LLMs (`AGENT_FRICTION.md` rows 1, 25, 33) is writing `"{(if cond \"yes\" \"no\")}"` or `"{(call str.join parts \", \")}"`. The lexer currently disallows quotes inside format placeholders, forcing awkward intermediate `let` bindings.
* **Fix**: Allow escaped quotes and nested string literals inside format placeholders in `src/lexer.rs`, `src/fmt.rs`, and `aipl_src/fmtlower.aipl`.

### 2. Add Hex (`0x...`) and Binary (`0b...`) Integer Literals
* **Problem**: In systems, hash, and instruction encoder code (`arm64.aipl`, `x64.aipl`, `hash.aipl`), developers and LLMs must write decimal constants like `-306674912` (for `0xEDB88320`) or `524287` (for `0x7FFFF`). LLMs routinely hallucinate decimal conversions of bitmasks.
* **Fix**: Support standard `0x` and `0b` literals in the lexer and parser. Add `rotl`, `rotr`, and `bitnot` primitives.

### 3. Make `aipl verify` Typecheck `(test ...)` Blocks
* **Problem**: `aipl verify` currently strips `(test ...)` forms before typechecking (`AGENT_FRICTION.md` row 68). A file with broken types or unresolved identifiers inside a `(test ...)` block reports `OK` on `aipl verify`, only failing later on `aipl test`.
* **Fix**: Ensure `aipl verify` typechecks `(test ...)` blocks while keeping standard `aipl compile` stripping them.

### 4. Migrate `std`'s `run_*_tests` to `(test ...)` Forms (`FEEDBACK_TOOLS_PLAN.md` F6)
* **Problem**: Every module in `std` defines public functions like `run_json_tests`, `run_vec_tests`, etc. These are parsed, typechecked, and included in binaries unless pruned by `--opt` dead-code elimination.
* **Fix**: Migrate all `run_*_tests` to `(test ...)` forms so they are automatically omitted from normal builds.

### 5. Mid-Body `(assert cond)` & Contract Message Parity
* **Problem**: `req` and `ens` are header-only, while `inv` is loop-header-only. Mid-function invariant checks currently require `if` + `panic`.
* **Fix**: Introduce mid-body `(assert cond [msg])` using the same counterexample value-printing mechanism as `req`. Resolve the minor contract message formatting difference between Rust and self-hosted compilers for enum values (`SYS_PLAN.md` Section 8).

### 6. Eliminate Text-to-Text Passes & Optimize Arena Usage in `aipl_src/`
* **Problem**: `driver.aipl` serializes ASTs back to flat S-expression text across passes (`resolver`, `generics`, `fmtlower`, `codegen_opt`), requiring `codegen.aipl` to re-parse and re-infer types. Additionally, `codegen_opt.aipl` does not free per function, peaking at ~1 GB of heap when compiling itself (`OPT_PLAN.md`).
* **Fix**: Introduce per-function arena clearing in `codegen_opt.aipl` to reduce peak memory usage by >10x. Over time, pass structured typed ASTs directly between compiler stages.

---

## 4. Documentation & Housekeeping Drift

The following documents need updates to reflect progress made between Oct 28 and Oct 30, 2026:

1. **`README.md`, `ROADMAP.md`, and `docs/FAQ.md`**:
   - Update claims stating the self-hosted compiler only implements `compile`. Note that `verify`, `run`, `test`, `watch`, `serve --fresh`, `eval`, and `doc` are now implemented in `aipl_src/aipl.aipl`.
   - Update `ROADMAP.md` to remove claims that `mkdir`, `rmdir`, `stat`, `chmod`, `rename`, and `exec`/`spawn` are missing; all landed in `sys_v1` and `std/proc`.
   - Remove "sets" from the missing list in `ROADMAP.md` (`std/set.aipl` is complete).
2. **Apple Silicon Branch (`features/apple-silicon`)**:
   - Merge `features/apple-silicon` into `development`. The binary overwrite bug (`ETXTBSY`/`SIGKILL`) was fixed on `development` via `write_replacing` in `src/main.rs` and `aipl_src/aipl.aipl`; the macOS test gating script (`tools/mac_check.sh`) is ready to integrate.
3. **Local Workspace Sync**:
   - Rebuild `target/release/aipl` (`cargo build --release`) so that local tools like `python3 tools/docs_check.py` invoke a binary matching the current syntax specification.

