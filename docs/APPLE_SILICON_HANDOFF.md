# AIPL on Apple Silicon: instructions for the agent on the Mac

**Round 2 is at the end of this file (section 6, 2026-10-26): start there.**
Sections 1-5 are round 1, kept for the rules and the background.

Written 2026-10-25 by Claude (Opus 5.5) for Gemini, working on the owner's
new ARM64 Apple Silicon Mac. It says what AIPL does on that machine today,
what to build and test there, in what order, where to stop and report, and
the rules of this repository. Facts marked *checked* were read in the code
on 2026-10-25; everything else is an expectation for you to test. If what
you find disagrees with this file, the code and your measurements win: say
so in your report.

## 1. Who you work for and how

- The owner is an economist and a junior Python and SQL developer. Explain
  architecture in plain terms.
- AIPL is a programming language for AIs, written by AIs. Choose what is
  correct and least error-prone for a model writing it; human ergonomics
  do not matter.
- Every claim needs evidence: a command and its output, a test, or a file
  and line. The owner re-checks other agents' work, and an earlier outside
  review of this repository was checked claim by claim
  (docs/NOTES_REVIEW.md); several of its claims were wrong. Write your
  report so it can be checked the same way.
- Disagree when the evidence says so. Do not paper over a failure: a red
  test is reported with its output, not skipped silently.
- No hand-written cryptography, ever (the owner will not pay for an audit).

## 2. The repository, briefly

Read, in this order: README.md, DEVELOPING.md, docs/HANDOFF.md (the state
of the work and the rules), docs/design/NATIVE_BACKEND_PLAN.md (how the
native backend is built), and docs/notes/arm64_build.md with its verdicts
in docs/NOTES_REVIEW.md, section "arm64_build" (one claim there is wrong:
ARM64's `sdiv` not trapping is a cost, not an advantage, since WebAssembly
requires the trap, so an ARM64 backend must add the checks).

What runs where (*checked*):

- **Two toolchains, kept byte for byte equal.** The Rust toolchain
  (`src/`: parser, checker, VM, wasm compiler, the `aipl` command, and the
  `aipl-run` launcher, which embeds wasmtime) and the self-hosted one
  (`aipl_src/`, written in AIPL). Both turn a program into the same
  WebAssembly bytes. Any change to one is made to the other.
- **The native backend** (`aipl_src/native/`: wasm_reader.aipl, lower.aipl,
  x64.aipl, elf.aipl, runtime.aipl, wasi.aipl, sockets.aipl, fresh.aipl,
  native.aipl) translates WebAssembly into a static Linux x86-64 ELF
  executable that makes its own system calls. It exists only in AIPL; the
  Rust command runs it inside wasmtime (src/native.rs).
- `aipl_core::native::supported()` (src/native.rs) is true only on Linux
  x86-64. Elsewhere `aipl compile --exe` makes a **launcher bundle** instead:
  a copy of `aipl-run` with the program's wasm, its `--listen` grants, and a
  20-byte `AIPLEXE2` trailer appended (src/main.rs `write_executable`), and
  `--target native` is refused. install.sh says the same on Darwin.
- **The seed** `bootstrap/aipl-linux-x86_64` is a static Linux x86-64 binary
  that builds the self-hosted compiler with no Rust (`sh bootstrap/build.sh`).
  It cannot run on macOS directly.
- **CI** (.github/workflows/ci.yml) runs only on `ubuntu-latest` (x86-64):
  build, clippy with `-D warnings`, `cargo test --release`, the AIPL test
  suite, and two program fuzzers. `tools/ci.sh` runs the same steps locally
  (it also runs the seed build, so on the Mac run its steps one by one), and
  `tools/ci-status.sh` shows a run's progress.
- `.cargo/config.toml` sets `RUST_MIN_STACK` for tests (deep recursion);
  anything that runs test binaries without cargo must set it too.
- Tests that need the native backend: tests/test_bootstrap.rs is gated to
  Linux x86-64, and tests/test_runner.rs checks the platform; most of
  tests/test_native.rs and several others are **not** gated, so expect them
  to fail on macOS. That is the first thing to sort out (task 2).

## 3. Rules of the repository

- **Branch:** work on `features/apple-silicon` (this file is its first
  commit). Never commit to `main` or `development`; the owner merges. Do
  not touch `features/when` (Claude's branch, in progress).
- **Stay out of these files** (Claude is changing them on features/when):
  tests/test_serve.rs, tests/test_net.rs, src/net.rs, src/bin/aipl_run.rs,
  src/sugar.rs, src/parser.rs, aipl_src/parser.aipl, aipl_src/resolver.aipl,
  aipl_src/codegen.aipl. If a fix needs one of them, describe it in your
  report instead of making it.
- **Git safety:** never `git stash`, `git reset`, `git restore`, `git clean`,
  or `git checkout <branch>` in a checkout with uncommitted work (undo an
  edit by editing it back). Commit only your own files, by explicit path.
- **Never refresh the seed** (bootstrap/) without the owner's approval.
- **Do not change the language** (syntax, checker rules, std behaviour).
  This work is about platforms.
- **Gate, do not delete.** A test that cannot run on a platform gets a
  `#[cfg(...)]` or a runtime check with a comment saying why, and keeps
  running where it can. Never weaken an assertion to make it pass.
  (Round 2 makes this stricter: a whole test that cannot run is marked
  ignored, never returns early. See section 6.)
- **Run the checks before every commit you hand over**, on the Mac and,
  where it applies, in the Linux containers (task 4), and paste the
  summary into your report.
- Log your own mistakes writing AIPL in docs/design/AGENT_FRICTION.md (a new
  row at the end, with your name), as the other agents do.

## 4. The tasks, in order

Each has a deliverable. Stop after task 5 (a plan, no code) and wait for the
owner.

### Task 1: build and run AIPL on the Mac

1. Install Rust with rustup (stable), Python 3, and git. Record `uname -a`,
   `sw_vers`, `rustc --version`, and the chip (`sysctl -n machdep.cpu.brand_string`).
2. `cargo build --release`. Report any failure in full.
3. Try the toolchain by hand and record each result:
   - `cd examples && ../target/release/aipl eval word_count.aipl -- input.txt`
     (word_count reads a file named on its command line) and two or three
     other examples, `aipl test aipl_src/test_suite.aipl`,
     `aipl verify`, `aipl doc vec`, `aipl eval -e '(+ 40 2)'`;
   - `aipl compile examples/word_count.aipl -o wc.wasm`, then from examples/
     `../target/release/aipl run ../wc.wasm input.txt`;
   - a server: `aipl eval --listen 127.0.0.1:8080 tests/aipl/net/serve_demo.aipl -- 127.0.0.1:8080 3`
     and three `curl http://127.0.0.1:8080/` calls.

Deliverable: a section "Task 1" in docs/APPLE_SILICON_REPORT.md with the
environment and each command's result.

### Task 2: make the test suite honest on macOS

1. `cargo test --release --no-fail-fast 2>&1 | tee test-macos.log` (on an
   M-series Mac expect well under 15 minutes). `python3 tools/ci_tests.py`
   gives per-test times and per-binary memory in `target/ci_tests.jsonl` if
   you want them.
2. Sort every failure into: (a) needs the native backend or the seed, so
   cannot run on macOS (expected); (b) a real difference on macOS (a bug:
   paths, signals, sockets, file modes, line endings, stack size, timing);
   (c) unclear. Each with the test's name and the first lines of its output.
3. For (a): gate the test to Linux x86-64 the way tests/test_bootstrap.rs
   and tests/test_runner.rs already do, with a one-line comment. Where only
   part of a test needs native (often a loop over backends), skip only that
   part on other platforms.
4. For (b): fix it if the fix is small and outside the files in section 3,
   with a test that shows it; otherwise report it.
5. Clippy must stay clean: `cargo clippy --release --all-targets -- -D warnings`.
6. Run the two fuzzers: `python3 tools/fuzz.py --cases 3000 --seed 1` and
   `python3 tools/run_fuzz.py --cases 500 --seed 1`. run_fuzz compares the
   VM with compiled code; see whether it expects native executables.

Deliverable: commits on the branch (gates, small fixes), and the report's
"Task 2" section: the failure list with categories, what you gated and why,
and the final `cargo test` summary on macOS (0 failed is the goal; any test
still failing is listed with its reason).

### Task 3: macOS executables (the launcher bundle)

On Apple Silicon every executable must carry a valid code signature, or the
kernel kills it ("Killed: 9"). The Rust linker signs `aipl-run` ad hoc. A
bundle is `aipl-run` with bytes appended, which may or may not break that
signature. Nobody has tried it on a Mac. Find out:

1. `aipl compile --exe examples/word_count.aipl -o wc`, then from examples/
   `../wc input.txt`. Does it run? Record
   `codesign -dv --verbose=4 wc` and `codesign --verify --strict wc`.
2. If it is killed or refused: find the smallest correct fix and propose
   it (for example re-signing ad hoc after writing, `codesign --force -s -`,
   if that keeps the appended program readable; or carrying the program in
   a section of its own). src/bin/aipl_run.rs is on the list in section 3,
   so write the proposal with the evidence; a change to src/main.rs
   `write_executable` alone is yours to make and test.
3. Also try a bundle with `--listen` and a server program, and
   `aipl compile --exe --target wasm --serve 127.0.0.1:8081 tests/aipl/net/fresh_demo.aipl -o fresh`
   (a fresh instance per connection). Note: killing `aipl serve --fresh`
   by its process id leaves its `aipl-run` child serving (a known issue);
   kill the process group, or use the bundle.
4. Whether `tools/make_kit.sh` can make a macOS kit (today it makes a Linux
   x86-64 one): what would change. Report only.

Deliverable: the report's "Task 3" section, and any change to
`write_executable` with a test that runs on macOS and is skipped elsewhere.

### Task 4: the Linux side, in containers on the Mac

Use Docker Desktop, OrbStack, or Colima.

1. **linux/arm64** (runs at native speed): clone into a container
   (`rust:1` image), build, and run the test suite. Same expectations as
   macOS (no native backend); report what differs from macOS.
2. **linux/amd64** (through Rosetta, or QEMU if Rosetta is off; slower):
   run every step of CI as GitHub does, including `sh bootstrap/build.sh`
   and the native tests (`sh tools/ci.sh .` runs them all). This checks the
   x86-64 native backend and the seed under emulation. Report timings and
   any failure, and say whether failures look like emulation (timing,
   signals) or real.
3. Check that a native executable built in the amd64 container runs there,
   and that the same program's launcher bundle from the arm64 container
   runs there too.

Deliverable: the report's "Task 4" section, with the commands so anyone can
repeat it (a small script under tools/ is welcome).

### Task 5: a plan for native ARM64 (no code)

Write docs/design/ARM64_PLAN.md in the style of the other plans in
docs/design/ (a status line, "at a glance" table, numbered sections, open
questions for the owner). It should cover, with evidence from the code:

- **Linux aarch64 ELF first.** What stays (wasm_reader.aipl, native.aipl's
  driving, the ELF writer's structure), what is new (an AArch64 encoder in
  place of x64.aipl; lower.aipl's register model for 31 registers; the
  runtime's system calls, whose numbers differ on aarch64 Linux), and how
  WebAssembly's traps are kept (integer division by zero and `INT_MIN / -1`
  need explicit checks on ARM64; out-of-bounds memory through guard pages
  and the signal handler, as runtime.aipl does on x86-64).
- **Then macOS (Mach-O), separately.** macOS has no stable system-call
  interface: programs must call libSystem, so a static executable that
  makes its own calls (what the Linux backend makes) is not an option there.
  Every executable must be signed. Say what that means for the design
  (dynamic linking to libSystem, the Mach-O writer, signing), and whether
  the launcher bundle is a good enough answer for macOS for now.
- **Testing:** qemu-aarch64 user mode on Linux x86-64 (GitHub's runners),
  `ubuntu-24.04-arm` and `macos-14` runners for CI, and how the existing
  native tests (tests/test_native*.rs, tools/run_fuzz.py) would run against
  a second target.
- **Size of the work:** the native backend's current size
  (`wc -l aipl_src/native/*.aipl`) as a guide, in steps the owner can
  approve one at a time.
- **Open questions for the owner.**

Deliverable: the plan file, then stop. Nothing in it is built until the
owner approves it.

## 5. Handing back

When you stop (after task 5, or earlier if something blocks you):

- Commit by explicit path on `features/apple-silicon` and push the branch.
- The report, docs/APPLE_SILICON_REPORT.md, starts with a summary: what
  works on the Mac, what you changed (commit by commit), what is broken and
  why, and what you recommend next. Every claim with its evidence.
- Do not merge into `development` or `main`.

## 6. Round 2 (2026-10-26)

Written by Claude (Opus 5.5) after reading your report, your commits, and
docs/design/ARM64_PLAN.md. Good work: the triage was careful, every
failure was traced, and stopping at the protected file was right. What
follows is what changed since, one correction, and the next tasks.

### 6.1 What changed since round 1

- **features/when is merged** (into `development` and `main`), and so is
  the optimizer (into `development`). The protected list in section 3 is
  lifted: **tests/test_serve.rs and the other files there are yours to
  edit** for the tasks below.
- **This branch is up to date.** Claude merged `development` into
  `features/apple-silicon` (commit "Merge development into
  features/apple-silicon"; it merged cleanly). Pull before you start:
  `git pull` on `features/apple-silicon`. Do not rebase it.
- **`aipl compile` now optimizes by default** (docs/design/OPT_PLAN.md;
  AIPL_SPEC.md 6.7), and `--no-opt` builds as before. Some tests pass
  `--no-opt` on purpose (they compare with the VM's addresses or call
  chains); leave those flags as they are.
- **Compiled toolchain modules are cached** in `~/.cache/aipl` (or
  `$AIPL_CACHE`; empty turns it off). The first `aipl compile` after a
  build takes about half a second, the rest about 0.08 s. If something
  looks stale, `rm -rf ~/.cache/aipl` is always safe.
- New tests since your run: tests/test_opt.rs, and more in
  test_language_gaps.rs, test_values.rs, test_memory.rs. They build with
  plain `--exe` (on the Mac, the launcher bundle), so they should run;
  check.

### 6.2 One correction: a skipped test must say so

Your gating made 21 tests in tests/test_native.rs (and
`the_first_native_executable_prints_and_exits_7` in test_native_elf.rs,
by `#[cfg]`) end early or vanish on the Mac. An early `return` makes cargo
print `ok` for a test that checked nothing, so "61 of 62 targets pass" read
as more than it was. Section 3's old wording allowed "a runtime check";
that was Claude's mistake, and this is the rule now:

- **A whole test that cannot run on this platform is ignored, with the
  reason**, so `cargo test` lists it as `ignored` and counts it:

  ```rust
  #[test]
  #[cfg_attr(not(all(target_os = "linux", target_arch = "x86_64")),
             ignore = "runs Linux x86-64 executables (the native backend)")]
  fn control_flow_programs_match_natively() { ... }
  ```

  Remove the early `return` blocks you added; use this attribute instead.
  Do the same for test_native_elf.rs's `#[cfg]` (it should be ignored on
  the Mac, not compiled away).
- **A test that covers several backends keeps running**, with only the
  native part left out (your `targets` lists in test_runner.rs,
  test_benchmarks.rs, test_std.rs, test_testing.rs, test_eval_expr.rs are
  right). Give each file one constant rather than repeating the `cfg!`:
  `const NATIVE: bool = cfg!(all(target_os = "linux", target_arch = "x86_64"));`
  with a comment saying the native backend targets Linux x86-64 only
  (AIPL_SPEC.md 6.6).
- **Report the counts as cargo prints them**: passed, failed, ignored, per
  platform. A gated part of a multi-backend test is listed in the report
  by name.

### 6.3 The tasks, in order

**Task A: tests/test_serve.rs.** Gate the two tests your report names
(`a_fresh_server_that_cannot_listen_says_so`,
`a_fresh_instance_per_request_isolates_a_crash`) as you proposed: the
`native` host only where `NATIVE` is true, the other hosts everywhere.
These tests start servers on loopback ports; if one is flaky on the Mac,
say so with the output (docs/HANDOFF.md, "Open problems", explains the
port fixes made on Linux).

**Task B: the correction above (6.2)**, across every file you gated.

**Task C: run everything and update the report.** On the Mac and in the
`linux/arm64` container: `cargo build --release`,
`cargo clippy --release --all-targets -- -D warnings`,
`cargo test --release --no-fail-fast` (with the passed / failed / ignored
counts per target), `./target/release/aipl test aipl_src/test_suite.aipl`,
`python3 tools/fuzz.py --cases 3000 --seed 1`,
`python3 tools/run_fuzz.py --cases 500 --seed 1`, and the new
`python3 tools/opt_diff.py` (optimized builds against `--no-opt` builds of
every repository program; it should print no DIFFERS line). Put the
results at the top of docs/APPLE_SILICON_REPORT.md under "Round 2",
correcting round 1's "61 of 62" with the honest counts. Commit by explicit
path and push `features/apple-silicon`. **Then stop and hand back**; the
owner merges.

**Task D (after A-C are pushed): Rosetta and the first load segment, an
investigation on its own branch.** Your finding that Rosetta 2 refuses
AIPL's x86-64 executables (first `PT_LOAD` not at file offset 0) is a real
portability bug for Mac users running `linux/amd64` containers. On a new
branch `features/elf-offset0` from `features/apple-silicon`:

- Change aipl_src/native/elf.aipl so the first `PT_LOAD` segment starts at
  file offset 0 (the ELF and program headers inside it), keeping x86-64's
  4 KiB pages. Nothing else.
- Show the result runs: under Rosetta in `linux/amd64` (it should now),
  under QEMU, and in `linux/arm64` with QEMU user mode if you can. Run
  tests/test_native.rs and tests/test_native_elf.rs in the `linux/amd64`
  container (slow under emulation is fine; say how long).
- `sh bootstrap/build.sh` must still reach its fixed point (stage 3
  identical to stage 2). The seed itself keeps the old layout until a seed
  refresh, which only the owner approves: do not refresh it.
- Report on that branch (a section in docs/APPLE_SILICON_REPORT.md) and
  push it. Do not merge it: Claude reruns the full CI on a native Linux
  x86-64 machine first, since emulation can hide what real hardware shows.

**Not now: docs/design/ARM64_PLAN.md.** The plan is useful and stays as a
proposal. The owner's priority is getting the repository ready for
release, and a second native backend is post-launch work; do not start
NA2-NA12 until the owner approves the plan. Mac users have the launcher
bundle, and Linux arm64 users have wasm under aipl-run, in the meantime.

### 6.4 Files to stay out of in round 2

Claude is working on these next (release readiness: finding the standard
library from an installed or copied `aipl`): src/resolver.rs,
src/selfhost.rs, src/main.rs, aipl_src/aipl.aipl, aipl_src/driver.aipl,
aipl_src/doc.aipl, aipl_src/resolver.aipl, install.sh, tools/make_kit.sh.
If a fix needs one of them, describe it in your report instead.


## 7. Round 3 (2026-10-26)

Written by Claude (Opus 5.5) after reading round 2, the audit in the
report's section A, and running Task D's change through the full CI on
real x86-64 hardware. You are on a Mac mini with an M4 now, so everything
you run is bare metal: that is what this round is for.

Round 2 was useful: the gating is honest, test_serve.rs's "address in
use" check reads the OS's own wording (Darwin's 48, Linux's 98), and the
Rosetta finding was real and well traced. One thing to fix in how you
report (7.5): several statements were not measured.

### 7.1 What changed since round 2

- **This branch is up to date with `development`.** Claude merged it in
  (commit "Merge development into features/apple-silicon ... (for round
  3)"). `git pull` on `features/apple-silicon` before you start. Do not
  rebase.
- **The language changed** (docs/design/LANGUAGE_DECISIONS.md items 1-8).
  You will read programs written this way:
  - a struct is made with its fields named, each once, in order:
    `(new S a [x:1 y:2])`, `(make V [x:1 y:2])`; a field that starts empty
    is given `(zero T)`. The zeroed `(new S a)` and a value struct's
    positional `make` are errors now;
  - a name's scope is its block or match arm (siblings may reuse a name);
  - printing is the format forms only, `(println "n = {n}")`: std/io's
    `println_int` and its kind are gone;
  - `(arr.of T a x y ...)` is the array literal; every module a file names,
    it imports; a missing value is an `option.Option`.
- **The seed was refreshed** (bootstrap/README.md) for the named fields.
- **Robustness fixes:** "Out of memory: the limit is N MiB" at the memory
  limit; the self-hosted compiler's tables grow; sugar works inside a
  formatting placeholder; a full standard output is an error, not a panic;
  a fresh-mode answer over 64 MiB says why it was a 500; `aipl serve
  --fresh` becomes the aipl-run it starts (exec).
- **New tests:** tests/test_scopes.rs, and more in test_sugar.rs,
  test_memory.rs, test_selfhost.rs, test_serve.rs
  (`ending_aipl_serve_ends_the_server`), and test_exit.rs
  (`a_full_standard_output_is_an_error_not_a_panic`, ignored on macOS,
  which has no /dev/full).
- **The A/B harness** (tools/harness/README.md): 26 tasks that models
  write in AIPL, Python, and Rust. `harness.py selftest` is part of CI;
  run it. **Do not attempt the tasks themselves**: you have read this
  repository, so your results would not measure what the harness is for.
  The owner runs other models, in fresh contexts.
- **`openat2` (audit A5) is pinned by the owner.** Native executables
  cannot open files on Linux before 5.6 nor under Rosetta 2; that is now
  said in the spec (6.6), the FAQ, and ROADMAP, and the fix is planned in
  docs/HANDOFF.md. Do not implement it.

### 7.2 Task D: Claude has it from here

The change on `features/elf-offset0` (only its elf.aipl and
test_native_elf.rs diff, applied to current `development`) passed the full
CI on a native Linux x86-64 machine: 453 tests passed, 0 failed; the seed
bootstrap reached its fixed point; the built compiler's first `PT_LOAD` is
at file offset 0. What is left needs x86-64 hardware or the owner (a
read-only segment for the headers, which are writable now; the merge; a
seed refresh so the bootstrap runs under Rosetta), so Claude keeps it.
Leave `features/elf-offset0` and aipl_src/native/elf.aipl alone.

### 7.3 The tasks, in order

**Task A: the full verification on the M4.** After pulling:
`cargo build --release`; `cargo clippy --release --all-targets -- -D
warnings`; `cargo test --release --no-fail-fast`;
`./target/release/aipl test aipl_src/test_suite.aipl`;
`python3 tools/fuzz.py --cases 3000 --seed 1`;
`python3 tools/run_fuzz.py --cases 500 --seed 1`; `python3 tools/opt_diff.py`
and `python3 tools/opt_diff.py --fuzz 500 --seed 1`;
`python3 tools/harness/harness.py selftest`. Report `cargo test` as a table
of **every** target with its passed, failed, and ignored counts (not only
the exceptions), and each other command's last line.

**Task B: the audit's open items.**
1. **The `test_opt.rs` flake (A3).** Make `trap()`'s assertion message
   show the whole exit status, including the signal on Unix
   (`std::os::unix::process::ExitStatusExt::signal`). Change the message
   only, not what is asserted. Then run the test 50 times
   (`for i in $(seq 50); do cargo test --release --test test_opt; done`)
   and report how many failed and with which signal. If it reproduces, find
   out which process got the signal (the bundle or aipl-run) and why; a
   macOS kill of a freshly written executable shows as signal 9.
2. **The two silent skips (A4).** In test_native_elf.rs,
   `readelf_accepts_the_executable_when_installed` returns early without
   `readelf`: make it
   `#[cfg_attr(target_os = "macos", ignore = "needs readelf (binutils), which macOS lacks")]`
   and remove the early return, so on Linux it requires readelf (CI and
   your containers have binutils; check). In test_bootstrap.rs, replace the
   file-wide `#![cfg(...)]` with the 6.2 attribute on each test, so they
   are counted as ignored on the Mac.
3. **The stale statements (A7).** Correct the report's lines that A7
   lists, and tools/container_test.sh: its amd64 note, and the
   `cargo test ... || true` that hides failures (the script should end with
   the tests' status).

**Task C: the `linux/arm64` container.** Rerun everything in Task A there
(`sh tools/container_test.sh arm64`) and report per target, as for the
Mac.

**Task D: bare metal on its own.** Run tests/test_serve.rs and
tests/test_net.rs 5 times each on the Mac and report every result; note
any test whose time differs much from the Linux CI times (test_serve
about 40 s, test_selfhost about 4 min, test_stats about 3 min).

Then commit by explicit path, push `features/apple-silicon`, and **stop
and hand back**. Do not merge anything.

### 7.4 Files

Yours this round: tests/*.rs, for the gating and test_opt's message only;
tools/container_test.sh; docs/APPLE_SILICON_REPORT.md. Stay out of:
aipl_src/, src/, bootstrap/, tools/harness/ (run its selftest only),
install.sh, tools/make_kit.sh, AIPL_SPEC.md, PROMPT_GUIDE_FOR_AIS.md. If a
fix needs one of them, describe it in your report instead.

Not now: the ARM64 backend (docs/design/ARM64_PLAN.md stays a proposal
until the owner approves it; post-launch), the `openat2` fallback, a seed
refresh, and anything in elf.aipl.

### 7.5 How to report

- **Only what you measured.** Every finding comes with the command that
  produced it and its output (or the relevant lines). A cause is stated as
  a cause only if you showed it; otherwise write "unexplained" and what you
  tried. Round 2's "100% reliability", the futex explanation, and "stage 1
  has the new layout" were not measured (audit A5).
- **Name things exactly.** On `linux/arm64`, `aipl compile --exe` writes
  the launcher bundle (an aarch64 ELF holding aipl-run and the wasm), not a
  native executable: AIPL's native backend writes x86-64 only.
- **Open with what is broken and what to do next**, as section 5 asks,
  then the counts, then the details. Keep round 2 and the audit as they
  are, below a new "Round 3" section.

## 8. Round 4 (2026-10-27): the Rosetta check

Written by Claude (Sonnet 5.5). One job this round, and it needs your Mac:
find out whether AIPL's Linux x86-64 executables, and the bootstrap that
builds the compiler with one, now work under Rosetta 2, and whether the
overwrite bug you found in round 3 is gone on macOS. Claude has tested the
Linux half with a stand-in for Rosetta (a seccomp filter that makes
`openat2` fail); you have the real thing. That is the whole reason for this
round: **report what you see, and do not fix anything you find.**

### 8.1 What changed since round 3

`git pull` on `features/apple-silicon` first (Claude merged `development`
into it; do not rebase). Since round 3:

- **Your `test_opt.rs` finding (A3) is fixed in `src/main.rs`.** `aipl
  compile -o OUT` now writes a temporary file and renames it over OUT, so it
  never opens an executable that was run for writing. On Linux the same bug
  was reproduced deterministically (`Text file busy`) by compiling over a
  running executable; `tests/test_opt.rs` has
  `compiling_over_a_running_executable_replaces_it`. Whether that also cures
  the macOS `SIGKILL (Code Signature Invalid)` is what you can measure.
- **Task D is done** (the read-only first `PT_LOAD` at file offset 0), and
  the seed was refreshed twice (bootstrap/README.md). Under Rosetta the
  seed `--version` already ran, from the owner's check.
- **The `openat2` fallback is in** (docs/HANDOFF.md, "The openat2
  fallback"): a native executable built without `--sandbox` that finds
  `openat2` failing (ENOSYS, or EPERM from a seccomp profile) opens files
  with plain `openat`; one built with `--sandbox` prints `this program was
  built with --sandbox, which needs the openat2 system call (Linux 5.6 or
  newer, and not Rosetta 2)` and exits 1. The seed (sha256
  `ae30b56410a1f5254422ff280e48b22a50334328d9ff04b434f341d086a6b4be`) has
  it, so the bootstrap should now finish under Rosetta.
- **New tools:** `tools/mac_check.sh` (what to run on a Mac, section 8.2,
  Task B) and `tools/without_openat2.py` (Linux x86-64 only: do not run it
  on the Mac).
- **Also new, nothing for you to do:** `std/math`, the agent workspace
  (`harness.py workspace`), `docs/USING_AIPL.md`.

### 8.2 The tasks, in order

**Task A: the seed is the one Claude built.** `shasum -a 256
bootstrap/aipl-linux-x86_64` must print the hash above. If it does not,
stop and report that.

**Task B: the Mac itself.** On the M4, from a clean checkout of this branch:
`sh tools/mac_check.sh --quick`, then `sh tools/mac_check.sh` (about 10
minutes), then `sh tools/mac_check.sh --install development` (it installs
from GitHub into a scratch directory; it needs the network). Report every
`PASS`/`FAIL` line. Then the flake, as in round 3: `for i in $(seq 50); do
cargo test --release --test test_opt; done`, and report how many runs failed
and with what signal. Round 3 got 1 in 50 (signal 9). If it is 0 in 50, say
that, and that it proves little: one in fifty may need more runs, so also run
it 150 more times and report all 200.

**Task C: Rosetta.** In Docker Desktop turn on "Use Rosetta for x86_64/amd64
emulation" (Settings, General). **Show how you know a container ran under
Rosetta and not QEMU** (Docker's setting file, `docker info`, an entry under
`/proc/sys/fs/binfmt_misc` inside the container: whatever you can show); if
you cannot tell, say so, and say which you believe and why. Then, with the
repository mounted read-only at `/repo`:

1. `docker run --rm --platform linux/amd64 -v "$PWD":/repo:ro
   debian:stable-slim /repo/bootstrap/aipl-linux-x86_64 --version`
   (it printed `aipl 0.1.0` for the owner).
2. Compile and run a program that opens a file:
   ```
   docker run --rm --platform linux/amd64 -v "$PWD":/repo:ro debian:stable-slim sh -c '
     cd /repo/examples &&
     AIPL_STD=/repo/aipl_src/std/ /repo/bootstrap/aipl-linux-x86_64 compile --exe jsonpp.aipl -o /tmp/j &&
     echo "{\"b\":[1,2],\"a\":null}" | /tmp/j --sort &&
     echo "{\"x\":1}" > /tmp/in.json && /tmp/j /tmp/in.json'
   ```
   Before the fallback this stopped at `Error: cannot read entry file:
   jsonpp.aipl`. Claude expects a pretty-printed document, then `{ "x": 1 }`
   laid out on three lines. Report the output exactly.
3. The real finish line, the whole bootstrap under Rosetta, in a copy (the
   script writes `target/`):
   ```
   docker run --rm --platform linux/amd64 -v "$PWD":/repo:ro debian:stable-slim sh -c '
     mkdir -p /build && cd /repo && tar --exclude=./target -cf - . | tar -xf - -C /build &&
     cd /build && time sh bootstrap/build.sh &&
     sha256sum bootstrap/aipl-linux-x86_64 target/bootstrap/aipl'
   ```
   Claude expects `stage 3: identical to stage 2 (a fixed point)` and two
   identical hashes, `ae30b564...`: the compiler built under Rosetta is byte
   for byte the one built on Linux x86-64. Report the time too (it is about
   8 seconds on a Linux laptop).
4. A `--sandbox` program, which needs `openat2`: in the same kind of
   container, build `examples/word_count.aipl` with `compile --exe
   --sandbox` and run it on a small file. Two results are possible: the
   message quoted in 8.1 and exit status 1 (Rosetta lacks `openat2`, as your
   round 2 report says), or normal output (this macOS version's Rosetta has
   it). Either is fine; report which, with the exact output and exit status.
5. `sh tools/container_test.sh amd64`, now that it ends with the tests'
   status. Report `cargo test` per target as in round 3. Claude expects some
   failures (test_openat_fallback and the tests that run a seccomp filter
   may not work inside Rosetta; a few native tests may depend on the Linux
   kernel). For **each** failing test give its name and the first lines of
   its message, and say whether the cause is Rosetta, the container, or
   AIPL. Do not fix any of them.

### 8.3 Files

Yours this round: docs/APPLE_SILICON_REPORT.md only (add a "Round 4"
section above round 3's). Stay out of everything else, as in 7.4: if a
failure needs a change in aipl_src/, src/, bootstrap/, or a test, describe
it in the report. Do not merge, refresh the seed, or touch `main`.

### 8.4 How to report

As in 7.5: **only what you measured**, each finding with its command and its
output; a cause is a cause only if you showed it, otherwise "unexplained"
and what you tried. Open with a table: for Tasks A, B, and C1-C5, the
command, **PASS** or **FAIL**, and one line on what happened. Then what is
broken and what to do next, then the details. Commit by explicit path, push
`features/apple-silicon`, and **stop and hand back**.
