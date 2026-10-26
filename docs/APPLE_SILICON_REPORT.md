# AIPL on Apple Silicon: Porting and Verification Report

**Date:** 2026-10-25  
**Platform:** macOS 27.0.1 (Darwin 27.0.0 arm64, Apple M4)  
**Branch:** `features/apple-silicon`  
**Agent:** Gemini 3.8 Flash (High)

---

## Executive Summary

This report documents the verification, gating, testing, container validation, and architectural plan for running and supporting AIPL on Apple Silicon (ARM64 macOS) and ARM64/x86-64 Linux containers.

---

## Task 1: Build and Run AIPL on the Mac

### 1.1 Environment Details
- **`uname -a`**: `Darwin pennylane 27.0.0 Darwin Kernel Version 27.0.0: Tue Aug 11 21:02:57 PDT 2026; root:xnu-13432.1.9~1/RELEASE_ARM64_T8132 arm64`
- **`sw_vers`**:
  ```text
  ProductName:		macOS
  ProductVersion:		27.0.1
  BuildVersion:		26A434
  ```
- **`rustc --version`**: `rustc 1.99.1 (58cf8f9c3 2026-10-13)`
- **`cargo --version`**: `cargo 1.99.1 (5f94df478 2026-08-27)`
- **`python3 --version`**: `Python 3.15.0`
- **`git --version`**: `git version 2.56.0`
- **CPU (`sysctl -n machdep.cpu.brand_string`)**: `Apple M4`

### 1.2 Build (`cargo build --release`)
Command executed:
```bash
cargo build --release
```
Result: Succeeded with exit code 0 in 14.02 seconds.
```text
   Compiling aipl v0.1.0 (/Users/matt/Projects/Repos/AIPL)
    Finished `release` profile [optimized] target(s) in 14.02s
```

### 1.3 Manual Toolchain Verification

#### Evaluation of Examples
1. **`examples/word_count.aipl`**:
   - Command: `../target/release/aipl eval word_count.aipl -- input.txt` (executed from `examples/`)
   - Exit code: 0
   - Output:
     ```text
     [AIPL VM] Executing function 'main' from 'word_count.aipl'...
     lines: 4
     words: 15
     bytes: 81
     [AIPL Result]: Ok(Int(0))
     ```
2. **`examples/quicksort.aipl`**:
   - Command: `./target/release/aipl eval examples/quicksort.aipl`
   - Exit code: 0
   - Output:
     ```text
     [AIPL VM] Executing function 'main' from 'examples/quicksort.aipl'...
     median of 5000 random values: 502
     [AIPL Result]: Ok(Int(0))
     ```
3. **`examples/matrix_mult.aipl`**:
   - Command: `./target/release/aipl eval examples/matrix_mult.aipl`
   - Exit code: 0
   - Output:
     ```text
     [AIPL VM] Executing function 'main' from 'examples/matrix_mult.aipl'...
     trace of the product: 69.0
     trace for 9 x 9: 14040
     [AIPL Result]: Int(0)
     ```
4. **`examples/math_core.aipl`**:
   - Command: `./target/release/aipl eval examples/math_core.aipl`
   - Exit code: 0
   - Output:
     ```text
     [AIPL VM] Executing function 'main' from 'examples/math_core.aipl'...
     gcd 48 18 = 6
     isqrt 625 = 25
     primes below 1000: 168
     throttle at 90, 80, 50: yes
     [AIPL Result]: Int(0)
     ```

#### Verification, Docs, and One-liner Eval
1. **`aipl verify`**:
   - Command: `./target/release/aipl verify examples/word_count.aipl`
   - Exit code: 0
   - Output:
     ```text
     [AIPL Verifier] OK: module 'word_count' type-checks. Contracts are type-checked, not proven; req/ens run on every call, in the VM and in compiled code.
     ```
   - Note: Bare `aipl verify` without an argument exits with code 2 explaining `<FILE>` is required (`Usage: aipl verify <FILE>`).
2. **`aipl doc vec`**:
   - Command: `./target/release/aipl doc vec`
   - Exit code: 0
   - Output: Formatted documentation for module `vec` (signatures, descriptions, and contracts).
3. **`aipl eval -e '(+ 40 2)'`**:
   - Command: `./target/release/aipl eval -e '(+ 40 2)'`
   - Exit code: 0
   - Output:
     ```text
     42 : i32
     ```

#### Native Test Suite (`aipl_src/test_suite.aipl`)
- Command: `./target/release/aipl test aipl_src/test_suite.aipl`
- Exit code: 0
- Output: All 36 groups passed (`[AIPL Test] All groups passed.`).

#### Compilation to Wasm & Running via `aipl run`
1. **Compilation**:
   - Command: `./target/release/aipl compile examples/word_count.aipl -o wc.wasm`
   - Exit code: 0
   - Output:
     ```text
     [AIPL Compiler] Successfully compiled 'examples/word_count.aipl' -> 'wc.wasm' (59168 bytes)
     ```
2. **Execution under `aipl run`**:
   - Command: `../target/release/aipl run ../wc.wasm -- input.txt` (executed from `examples/`)
   - Exit code: 0
   - Output:
     ```text
     lines: 4
     words: 15
     bytes: 81
     ```
   - Note: Invoking `aipl run ../wc.wasm input.txt` without `--` was refused by the CLI (`unexpected argument 'input.txt' found; Usage: aipl run [OPTIONS] <FILE> [-- <ARGS>...]`). Supplying `-- input.txt` passed positional arguments to the guest program correctly.

#### Server Evaluation (`serve_demo.aipl`)
- Command:
  ```bash
  ./target/release/aipl eval --listen 127.0.0.1:8080 tests/aipl/net/serve_demo.aipl -- 127.0.0.1:8080 3
  ```
- Three `curl -s -i http://127.0.0.1:8080/` requests were issued:
  ```text
  HTTP/1.1 200 OK
  Content-Type: text/plain; charset=utf-8
  Content-Length: 6

  hello
  ```
- Server process termination:
  ```text
  [AIPL VM] Executing function 'main' from 'tests/aipl/net/serve_demo.aipl'...
  served 3, arena 65536 bytes
  [AIPL Result]: Ok(Int(0))
  ```

---

## Task 2: Make the Test Suite Honest on macOS

### 2.1 Initial Test Suite Run on macOS
Ran:
```bash
cargo test --release --no-fail-fast 2>&1 | tee test-macos.log
```
Results of initial un-gated run:
- Total test targets: 62
- Targets passing cleanly: 53
- Targets with failures: 9 (`test_benchmarks`, `test_eval_expr`, `test_native`, `test_native_elf`, `test_runner`, `test_serve`, `test_std`, `test_testing`, `test_visibility`)
- Total failing tests: 32

### 2.2 Categorization of Initial Failures

#### Category (a): Needs Native Linux x86-64 Backend or Seed (Expected)
The native backend currently lowers WebAssembly exclusively to Linux x86-64 static ELF binaries making direct Linux system calls (`syscall`). On macOS (Darwin ARM64), attempts to execute these binaries fail with exit code 126 (`cannot execute binary file`), or CLI invocations with `--target native` are properly rejected by `src/native.rs` (`Error: --target native: the native backend targets Linux x86-64 only (use --target wasm)`).

1. **`tests/test_benchmarks.rs`**:
   - `every_benchmark_prints_its_expected_output`: loops over `("wasm", ...)` and `("native", ...)` running `aipl_core::native::executable`. Native execution fails with exit code 126.
2. **`tests/test_eval_expr.rs`**:
   - `values_print_with_their_types_in_every_backend`: loops over `["", "--wasm", "--native"]`.
   - `errors_are_at_the_expressions_own_lines_and_columns`: tests traps across backends including `"--native"`.
3. **`tests/test_native.rs`** (21 tests):
   - `allocating_programs_match_natively`
   - `calling_convention_matches_natively`
   - `checked_arithmetic_matches_natively`
   - `control_flow_programs_match_natively`
   - `directories_and_file_information_match_natively`
   - `every_i32_operator_matches_natively`
   - `every_program_matches_natively`
   - `file_programs_match_natively`
   - `float_locals_survive_calls_natively`
   - `float_programs_match_natively`
   - `function_reference_programs_match_natively`
   - `i64_programs_match_natively`
   - `register_locals_survive_atomics_grow_and_imports_natively`
   - `running_out_of_stack_traps`
   - `the_compiler_runs_natively`
   - `the_harness_tells_programs_apart`
   - `thread_programs_match_natively`
   - `traps_name_their_source_positions`
   - `traps_print_their_line_and_call_chain`
   - `value_struct_programs_match_natively`
   - `wasi_programs_match_natively`
   *(Note: 2 tests in `test_native.rs` passed without execution: `unsupported_instructions_and_imports_are_named` and `the_vm_builds_the_same_executable`)*
4. **`tests/test_native_elf.rs`**:
   - `the_first_native_executable_prints_and_exits_7`: executes `hello_from_vm()` directly via `Command::new(&path)`.
5. **`tests/test_runner.rs`**:
   - `standalone_executables`: unconditionally tests `target in ["native", "wasm"]`.
6. **`tests/test_serve.rs`** (Protected File - See Section 2.5):
   - `a_fresh_server_that_cannot_listen_says_so`: passes `--target native` to `compile()`.
   - `a_fresh_instance_per_request_isolates_a_crash`: loops over `host in ["aipl serve --fresh", "native", "bundle"]` with `--target native`.
7. **`tests/test_std.rs`**:
   - `bigint_matches_python_integers`: loops over `("wasm", ...)` and `("native", ...)`.
8. **`tests/test_testing.rs`**:
   - `each_test_runs_alone_and_reports_alike_in_every_backend`: loops over `[None, Some("--vm"), Some("--native"), Some("--all-backends")]`.

#### Category (b): Real Differences on macOS (Platform Bugs / Quirks)
1. **`tests/test_visibility.rs`** (`what_a_module_keeps_is_an_error_in_both_resolvers`):
   - **Failure:**
     ```text
     left:  "/var/folders/.../lib.aipl: 1:31: (pub)..."
     right: "/private/var/folders/.../lib.aipl: 1:31: (pub)..."
     ```
   - **Root Cause:** On macOS, `/var` is a filesystem symlink pointing to `/private/var`. In Rust's resolver (`src/resolver.rs:1031`), imported module paths are resolved via `canonicalize()`, which resolves symlinks and prefixes `/private`. In the self-hosted AIPL resolver (`aipl_src/resolver.aipl`), WASI path resolution does not resolve symlinks outside its sandbox root, leaving `/var`.
   - **Fix:** In `tests/test_visibility.rs:41`, canonicalize the scratch directory (`let dir = dir.canonicalize().unwrap_or(dir);`) so both resolvers operate on the canonical `/private/var/...` path.
2. **`tests/test_runner.rs`** (`assert_kind`):
   - **Failure:** `value_structs_build_for_both_targets` failed with `/var/folders/.../flow_wasm: not an executable`.
   - **Root Cause:** `assert_kind` hardcoded `assert!(bytes.starts_with(b"\x7fELF"))`. On macOS, compiled standalone launcher bundles are Mach-O executables starting with Mach-O magic bytes (`0xcf 0xfa 0xed 0xfe` / `MH_MAGIC_64`), not Linux ELF headers.
   - **Fix:** In `assert_kind`, allow Mach-O magic bytes on `cfg!(target_os = "macos")`.

#### Category (c): Unclear Failures
None. All failures were traced to known architectural boundaries or filesystem path differences.

### 2.3 Gating and Platform Adjustments Committed
In commit `9a27dbe`:
- `tests/test_benchmarks.rs`: Gated native execution to `cfg!(all(target_os = "linux", target_arch = "x86_64"))`; wasm execution runs on all platforms.
- `tests/test_eval_expr.rs`: Gated `--native` options to Linux x86-64.
- `tests/test_native_elf.rs`: Added `#[cfg(all(target_os = "linux", target_arch = "x86_64"))]` to `the_first_native_executable_prints_and_exits_7`; ELF header checks and VM/Wasm byte-parity tests continue running everywhere.
- `tests/test_native.rs`: Added runtime platform checks `if !cfg!(all(target_os = "linux", target_arch = "x86_64")) { return; }` to the 21 tests requiring ELF execution. Preserved execution for `the_vm_builds_the_same_executable` and `unsupported_instructions_and_imports_are_named`.
- `tests/test_runner.rs`: Gated `--target native` in `standalone_executables` and permitted Mach-O binary headers in `assert_kind`.
- `tests/test_std.rs`: Gated native run in `bigint_matches_python_integers`.
- `tests/test_testing.rs`: Gated `--native` and `--all-backends` in `each_test_runs_alone_and_reports_alike_in_every_backend`.
- `tests/test_visibility.rs`: Canonicalized temp dir for symlink path parity.

### 2.4 Final `cargo test` Summary on macOS
Ran:
```bash
cargo test --release --no-fail-fast
```
- **61 out of 62 test targets passed cleanly.**
- **Only 1 target failed:** `tests/test_serve.rs` (2 tests failed: `a_fresh_server_that_cannot_listen_says_so` and `a_fresh_instance_per_request_isolates_a_crash`).

### 2.5 Protected File Report: `tests/test_serve.rs`
Per repository rules in `docs/APPLE_SILICON_HANDOFF.md` Section 3, `tests/test_serve.rs` is being actively modified on `features/when` and must not be edited on this branch.
- **Why it failed:** `tests/test_serve.rs` lines 365 and 407 unconditionally request `compile(&src, &exe, &["--exe", "--target", "native", ...])`. On non-Linux x86-64 platforms, the AIPL compiler intentionally refuses `--target native`.
- **Recommended fix for `features/when` / owner:**
  In `tests/test_serve.rs`:
  ```rust
  let hosts: &[&str] = if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
      &["aipl serve --fresh", "native", "bundle"]
  } else {
      &["aipl serve --fresh", "bundle"]
  };
  ```
  And in `a_fresh_server_that_cannot_listen_says_so`:
  Only compile the `native` target when `cfg!(all(target_os = "linux", target_arch = "x86_64"))`.

### 2.6 Clippy and Fuzzer Verification
1. **Clippy (`cargo clippy --release --all-targets -- -D warnings`)**:
   Passed with 0 warnings or errors.
2. **Grammar & Mutation Fuzzer (`tools/fuzz.py`)**:
   Command:
   ```bash
   python3 tools/fuzz.py --cases 3000 --seed 1
   ```
   Result: `3000 cases, seed 1: 0 failing (kept under target/fuzz/)`
3. **Execution Differential Fuzzer (`tools/run_fuzz.py`)**:
   Command:
   ```bash
   python3 tools/run_fuzz.py --cases 500 --seed 1
   ```
   Result: `500 cases, seed 1: 0 failing (kept under target/run_fuzz/)`  
   *Note on `run_fuzz.py`:* On macOS, `aipl compile --exe` creates the standalone Mach-O launcher bundle, which was executed directly as `./prog` and differentially verified byte-for-byte against the VM (`aipl eval`) and Wasmtime (`aipl run prog.wasm`).

---

## Task 3: macOS Executables (The Launcher Bundle)

### 3.1 Executable Execution & Code Signature Verification
1. **Compilation of Standalone Executable**:
   Command:
   ```bash
   aipl compile --exe examples/word_count.aipl -o wc
   ```
   Result: Output binary `wc` created (14,274,788 bytes).
2. **Execution**:
   Command:
   ```bash
   cd examples && ../wc input.txt
   ```
   Output:
   ```text
   lines: 4
   words: 15
   bytes: 81
   ```
   **Verdict: The executable runs completely and cleanly without being killed by the kernel.**

3. **Code Signature Inspection**:
   - `codesign -dv --verbose=4 wc`:
     ```text
     Executable=/Users/matt/Projects/Repos/AIPL/wc
     Identifier=aipl_run-c748b64ecd4f672c
     Format=Mach-O thin (arm64)
     CodeDirectory v=20400 size=110328 flags=0x20002(adhoc,linker-signed) hashes=3444+0 location=embedded
     VersionPlatform=1
     VersionMin=720896
     VersionSDK=1769472
     Hash type=sha256 size=32
     CandidateCDHash sha256=ea114bcb947c6741236ae9d0d16383641334bd35
     CandidateCDHashFull sha256=ea114bcb947c6741236ae9d0d16383641334bd35bc7aedfd9fbea9c1b9f20115
     Hash choices=sha256
     CMSDigest=ea114bcb947c6741236ae9d0d16383641334bd35bc7aedfd9fbea9c1b9f20115
     CMSDigestType=2
     Executable Segment base=0
     Executable Segment limit=13287424
     Executable Segment flags=0x1
     Page size=4096
     CDHash=ea114bcb947c6741236ae9d0d16383641334bd35
     Signature=adhoc
     Info.plist=not bound
     TeamIdentifier=not set
     Sealed Resources=none
     Internal requirements=none
     ```
   - `codesign --verify --strict wc`:
     ```text
     wc: main executable failed strict validation
     ```
     Exit code: 1.

### 3.2 Analysis of Kernel Signature Enforcement
- **Why it executes despite failing strict validation:**  
  On Apple Silicon macOS, the kernel's AMFI (Apple Mobile File Integrity) subsystem validates the integrity of executable memory pages against the embedded `CodeDirectory` hashes at page-fault / mmap time.  
  Because `aipl compile --exe` appends payload data (WebAssembly bytes, listen grants, and trailer) strictly *after* the end of the Mach-O binary segments, the executable code pages and the linker-generated ad-hoc code directory itself remain 100% unaltered.  
  When launched, `aipl-run` maps only its standard Mach-O segments for execution, and reads the appended payload bytes as ordinary file data via standard POSIX file I/O (`open`, `seek`, `read`). Because no appended bytes are mapped into executable pages, the kernel does not trigger a signature violation or kill the process ("Killed: 9").
- **Re-signing experiment (`codesign --force -s -`):**  
  Re-signing the file also fails strict validation because `codesign` recognizes that file length exceeds the end of the Mach-O load command definitions. However, because the un-resigned binary executes without error, no change to `src/main.rs:write_executable` or re-signing step is required for basic standalone bundle execution on macOS.
- **Dedicated Test:** Added `macos_launcher_bundle_executes_and_preserves_behavior` in `tests/test_runner.rs` (commit `78a7fcc`), gated with `#[cfg(target_os = "macos")]`, verifying compilation, argument passing, and clean exit.

### 3.3 Server Bundles with `--listen` and `--serve`
1. **Server with `--listen`**:
   - Command:
     ```bash
     aipl compile --exe --listen 127.0.0.1:8080 tests/aipl/net/serve_demo.aipl -o target/serve_bundle
     ./target/serve_bundle 127.0.0.1:8080 3
     ```
   - Execution & Curl: Three requests to `http://127.0.0.1:8080/` received `HTTP/1.1 200 OK` (`hello`), and the process exited with code 0 (`served 3, arena 65536 bytes`).
2. **Fresh Instance Per Connection (`--serve`)**:
   - Command:
     ```bash
     aipl compile --exe --target wasm --serve 127.0.0.1:8081 tests/aipl/net/fresh_demo.aipl -o target/fresh
     ./target/fresh
     ```
   - Execution & Curl: Issued requests `GET /` (`200 OK hello`) and `GET /echo?text=hello` (`200 OK /echo?text=hello`). The launcher bundle handled fresh worker isolation on macOS properly.

### 3.4 Feasibility of macOS Kit Generation in `tools/make_kit.sh`
Review of `tools/make_kit.sh` shows that while currently designed for Linux x86-64, adapting it for macOS Darwin ARM64 requires the following concrete changes:
1. **Remove Seed Dependency on Darwin**:  
   Line 42 (`(cd "$WORK" && sh bootstrap/build.sh)`) invokes `./bootstrap/aipl-linux-x86_64`. On macOS, this binary cannot execute natively. A macOS kit script must skip `bootstrap/build.sh` or build `aipl-native` inside a Linux container.
2. **Wrapper Delegation (`bin/aipl`)**:  
   Lines 73-82 of the kit wrapper script intercept `compile --exe` and redirect it to `$KIT/bin/aipl-native`. On macOS, `aipl-native` does not exist. The wrapper must instead allow `libexec/aipl compile --exe` to create launcher bundles embedding `libexec/aipl-run`.
3. **Packaging & Naming**:  
   The resulting tarball should be labeled `aipl-kit-darwin-arm64-$SHORT.tar.gz`. The verification block in `make_kit.sh` (lines 148-168) will then pass, because `eval`, `test`, `compile`, and `./p` (the launcher bundle) all execute successfully on Apple Silicon.


