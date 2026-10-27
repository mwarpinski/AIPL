# Handoff: where AIPL stands and what to do next

Written 2026-10-21 for an agent (or the owner) picking the work up on another computer. It is a snapshot; the code and git history win if they disagree. Read this, then ROADMAP.md and the plan named under "Next".

## Who the owner is and how they like to work

- An economist and junior Python/SQL developer; explain architecture in plain terms, not jargon.
- AIPL is a language for AIs, written by AIs. Choose what is correct and least error-prone for an AI; human ergonomics do not matter. Strict types, explicit at the point of use (which allocator, which promise), even if verbose, but never restating what is already plain.
- They value disagreement backed by evidence. Every critical claim (a reviewer's notes, another model's audit) gets a tested verdict before it is acted on. Re-verify any other agent's "done".
- They will never pay for a security audit (so no hand-written crypto for real traffic).
- Log each of your own AIPL mistakes in docs/design/AGENT_FRICTION.md (what, what the toolchain said, the feature that would prevent it).
- A training corpus of AIPL code is planned for later: zero incorrect code, no counterexamples. Not started; wait for the language to settle.

## Repository and branches

- GitHub repo, two long-lived branches: `main` (release) and `development` (integration). Work happens on `features/<name>`, merges `--no-ff` into `development`, and `development` merges into `main` at milestones.
- As of 2026-10-20 23:50, `main` = `development` = `bf1cfcb` (pushed). `features/net` is at `e647c60`, contained in both.
- On the owner's old machine two checkouts exist: `~/Projects/Repos/AIPL` (development, shared with the owner and other agents) and `~/Projects/Repos/AIPL-memory` (a git worktree, where the work was done). On a fresh machine one clone is enough: `git clone`, then `git worktree add ../AIPL-work -b features/<name> origin/development` for each piece of work.
- **Safety rules:** never `git stash`, `git checkout <branch>`, `git reset`, `git restore` or `git clean` in a checkout with uncommitted work (undo an edit by editing it back); commit only your own files, by explicit path; do merges in a temporary detached worktree (`git worktree add --detach /tmp/x origin/development`).

## Before every push: the full CI, locally

CI on GitHub runs: build, clippy with `-D warnings`, `cargo test --release`, the AIPL test suite, `fuzz.py` and `run_fuzz.py`. Run **every** step, doc-only changes included (docs are tests; a README edit once broke CI), and run `sh bootstrap/build.sh` first (3 seconds; it fails fast if the compiler's imported std uses a feature the checked-in seed cannot read). Takes about 15 minutes.

The scripts are now `tools/ci.sh` and `tools/ci-status.sh` (they were in the git-ignored `target/` until 2026-10-21); the copies below are the first versions, kept for the record. Usage: `sh tools/ci.sh /path/to/checkout`. Since 2026-10-21 the tests run through `tools/ci_tests.py` (each test binary in turn, as `cargo test` runs them, with cargo's environment from `.cargo/config.toml`; `CI_PLAIN=1` runs plain `cargo test`), which records every test's start, end, and time in `target/ci_tests.jsonl`. `sh tools/ci-status.sh [/path/to/checkout] [--watch [SECONDS]] [--all]` shows the steps and their times, the tests running now and for how long, each binary's result, time, and peak memory, the slowest tests, the failures with the start of their output, and how long the rest should take from the last run's times. Memory is per test binary: a binary's tests share one process.

`ci.sh`:

```sh
#!/bin/sh
# Every step of .github/workflows/ci.yml, in the given checkout; each step
# also logged, timestamped, to target/ci_steps.log (target/ci-status.sh reads it).
cd "$1" || exit 1
L=target/ci_steps.log
date '+%Y-%m-%d %H:%M:%S CI run' > $L
step() { echo "== $1"; echo "$(date +%H:%M:%S) $1" >> $L; }
note() { echo "$1"; echo "           $1" >> $L; }
step "build"; cargo build --release -q 2>&1 | tail -5
step "seed build"; sh bootstrap/build.sh > target/ci_seed.log 2>&1 || { note "seed build FAILED: $(tail -1 target/ci_seed.log)"; exit 1; }
step "clippy"; r=$(cargo clippy --release -q --all-targets -- -D warnings 2>&1 | tail -20); [ -n "$r" ] && note "clippy: $(echo "$r" | head -1)"; echo "$r"
step "tests"; : > target/ci_tests.log; cargo test --release --no-fail-fast > target/ci_tests.log 2>&1
note "$(grep -E "^test result" target/ci_tests.log | awk '{p+=$4; f+=$6} END {print p" passed, "f" failed"}')"
grep -E "^test .* FAILED" target/ci_tests.log | head -20 | while read -r l; do note "$l"; done
step "aipl test suite"; note "$(./target/release/aipl test aipl_src/test_suite.aipl 2>&1 | tail -1)"
step "fuzz.py"; note "$(python3 tools/fuzz.py --cases 3000 --seed 1 2>&1 | tail -1)"
step "run_fuzz.py"; note "$(python3 tools/run_fuzz.py --cases 500 --seed 1 2>&1 | tail -1)"
step "done"
```

`ci-status.sh`:

```sh
#!/bin/sh
# Where the current (or last) local CI run is: the step it is on, the test
# binaries finished so far (of about 62), and any failures. Run it any time:
#   sh ~/Projects/Repos/AIPL-memory/target/ci-status.sh
T="$(cd "$(dirname "$0")" && pwd)"
if [ -f "$T/ci_steps.log" ]; then
  echo "== steps (started $(head -1 "$T/ci_steps.log" | cut -c1-19))"
  tail -n +2 "$T/ci_steps.log"
fi
if [ -f "$T/ci_tests.log" ]; then
  done=$(grep -c "^test result" "$T/ci_tests.log")
  passed=$(grep "^test result" "$T/ci_tests.log" | awk '{p+=$4} END {print p+0}')
  echo "== tests: $done test binaries done, $passed tests passed so far"
  grep -E "^test .* FAILED|panicked at" "$T/ci_tests.log" | head -10
fi
if pgrep -f "cargo test --release --no-fail-fast" > /dev/null || pgrep -f "tools/fuzz.py\|tools/run_fuzz.py" > /dev/null; then
  echo "== still running"
else
  echo "== not running"
fi
```

Do not edit a worktree while a suite runs on it.

## What is done (2026-10-20)

- MEMORY_PLAN.md: complete. `(no_leak)` on function types, checked views, results and unions as values, `alloc.forever`, std keeps no memory on request paths.
- NET_PLAN.md steps 1-4 complete: sockets (`aipl_net_v1`, `--listen` grants), std/ip, std/net, std/serve (long-running server), `serve.once` and `aipl serve --fresh` (a fresh instance per connection), `aipl compile --exe --serve ADDR:PORT` (native forking host in `aipl_src/native/fresh.aipl`; the launcher bundle uses `aipl-run --serve-fresh`).
- `main -> i32` is the exit status everywhere; `main -> (result i32 str)` also allowed. `--max-memory SIZE`.
- CI was cut from 42 to about 15 minutes (the self-hosted toolchain runs as wasm under wasmtime in the tests).
- Fixed after GitHub's fuzz run failed: the AIPL resolver now matches Rust on a mismatched bracket inside a `{...}` placeholder; a function with two parameters of one name is now refused by both checkers (the VM and compiled code used to disagree); `tools/fuzz.py` sorts its source files so every machine runs the same cases.
- The owner decided: no AIPL-written TLS (HTTPS comes from rustls in the host; native executables refuse TLS and point to a proxy). A "secret types" constant-time idea is recorded in ROADMAP.md as a possible future direction.

## Next, in order

1. **Where the work stands (2026-10-26):** the optimizer (docs/design/OPT_PLAN.md) is done and on `development` and `main` (fea17a6): `aipl compile` optimizes by default (`--no-opt` does not); tests that compare a compiled build with the VM's addresses or call chains, or with the Rust code generator's bytes, pass `--no-opt`. On `features/portable`: a binary that works wherever it is (docs/LAUNCH_CHECKLIST.md section 4, done): `aipl` carries its library (src/library.rs) and the self-hosted command finds `share/aipl/std/` beside it. The owner's release order (2026-10-26): this first, then decide the language changes that break programs (module use without import, `(new S v...)`, an address-sized integer, `arr.of`, match binders' scope, Option-returning `io.read_file`/`os.arg`/`os.env`, `str.Bytes` by value), then one seed refresh, then robustness leftovers, the A/B evidence, a showcase, and the final checks. Deferred: nbody's native register allocator, pidigits' limbs (OPT_PLAN.md step 4). The agent on the Apple Silicon Mac works on `features/apple-silicon` (docs/APPLE_SILICON_HANDOFF.md, round 2).
2. **docs/design/CANONICAL_PLAN.md (approved 2026-10-20), steps 4-7.** Step 1 (docs) is done 2026-10-21: the guide's "Start here", the `and`/`or` checklist fix, `aipl --version` with the commit and the same line on the kit's docs, tools/ci.sh and ci-status.sh. The guide's table of ten pitfalls must follow steps 3-5 as they land. Step 2 (messages that name the right form) is built 2026-10-21 (one row left, in CANONICAL_PLAN.md section 2). Step 3 (`loop` retired, `for` only; internal form `counted_loop`) is built 2026-10-21, tools/retire_loop.py kept. Steps 4-7: `when` is the only one-branch `if` (empty `(block)` else becomes an error), an array literal `(arr.of T a x y z)`, a sweep for other duplicate forms, then an A/B harness across model families before any of the syntax proposals (inferred `let` types, `(f x)` without `call`, and so on). The owner's answers are recorded in that plan; the Sonnet review's verdicts are in docs/NOTES_REVIEW.md. After retiring `loop` or changing syntax the seed (`bootstrap/`) must be refreshed (ask the owner first); keep both toolchains (Rust in `src/`, self-hosted in `aipl_src/`) byte-for-byte equal.
3. **NET_PLAN.md step 5:** `connect`, `--allow-connect`, `http.fetch` (plain HTTP to addresses first; DNS later), tested against std/serve in each backend. Then step 6 (TLS from the host through rustls; native refuses) and step 7 (measure requests a second in docs/BENCHMARKS.md).
4. **Small leftovers: fixed 2026-10-26.** "Out of memory" is said as such; a full standard output is an error, not a panic (main.rs `say!`, tests/test_exit.rs); a fresh-mode answer over 64 MiB is logged as the reason for its 500 by each host (net.rs FRESH_TOO_LONG, aipl_run.rs, native/fresh.aipl; tests/test_serve.rs).
5. **Offered, not yet asked for:** a long fuzz job (many seeds, hundreds of thousands of cases) on a schedule or overnight. It found a real bug within minutes on eight extra seeds. Also a possible `CONSTANT_TIME_PLAN.md` (secret types, `select`/`cmov`, tests that compare branches and addresses taken for different secrets) as post-launch work.
6. Later: the worker-thread pool for servers, keep-alive, WebSockets/UDP/DNS, the packages registry (after launch), the training corpus, the aiOS vision (docs: ROADMAP.md).

## Open problems

- **Native executables and kernels before Linux 5.6 (pinned by the owner, 2026-10-26).** Every file a native executable opens goes through `openat2` (aipl_src/native/wasi.aipl `emit_openat2`), which Linux has had since 5.6; on an older kernel (RHEL 8's 4.18, Ubuntu 20.04's 5.4), and under Rosetta 2 (which lacks it: docs/APPLE_SILICON_REPORT.md, section A5), every open fails. Nothing else native code uses needs more than 3.17 (`getrandom`). The owner's call: say so for now (spec 6.6, FAQ, README, ROADMAP), fix later. The fix, about an hour or two: in `emit_openat2`, on `-38` (ENOSYS) retry with `openat` (257, flags and mode as in the open_how struct) when the program was built without `--sandbox` (nothing is lost: `/` is preopened); a `--sandbox` program probes `openat2` at start and, without it, says it needs Linux 5.6 or newer and exits 1 (a component-by-component resolver would keep the sandbox, but is real work and easy to get wrong). Test it here with `strace -f -e inject=openat2:error=ENOSYS` on the native test programs, and under Rosetta on the Mac. The seed keeps the old code until a refresh, which the owner approves.

- **tests/test_serve.rs's intermittent failures: found and fixed (2026-10-25).** They failed about one run in five, locally and on GitHub. Two causes, both in the tests' `free_port` (tests/test_serve.rs and tests/test_net.rs):
  1. It took ports with `bind(0)`, from the range every outgoing connection also uses, so one of the thousands of connections these tests make could take a port before its server bound it ("Address already in use"). Ports now come from 10000 up to the ephemeral range (/proc/sys/net/ipv4/ip_local_port_range), never twice in a run.
  2. The empty answers: it found a port free by listening on it and closing at once. Tests run as threads of one process while other tests start server processes, and a started process holds a copy of every open socket until it runs its program; so a probe, though closed, could stay listening for a moment in a half-started child. A client connecting then was reset ("Connection reset by peer"), and its server, which bound the port a moment later, never saw it. Shown by tools/flake/inherit.py (no process starting: 0 such connects in 2.1 million; four threads starting processes: 471 in 20 seconds), and by the logged failure (the client connected on its first try, 0.5 ms after the server was started, long before a VM server can listen). `free_port` now asks by connecting (refused: free) and never listens.
  After the fix: tests/test_serve.rs 20 runs of 20 passed (before: about 1 in 6 failed); tests/test_net.rs 5 of 5. tools/flake/ keeps the stress scripts and inherit.py. Found on the way, fixed 2026-10-26: killing `aipl serve --fresh` by its process id left its aipl-run child serving; it now becomes that aipl-run (exec), which removes its temporary module once read (tests/test_serve.rs `ending_aipl_serve_ends_the_server`).
- **A `let` inside a contract: fixed 2026-10-26.** `(req (block (let m:i32 (% n 2)) (eq m 0)))` verified and ran in the VM, but neither code generator took a contract's lets as locals ("Unbound local variable 'm'"). Both now collect them after the body's, in the same order (src/compiler/wasm.rs, aipl_src/codegen.aipl); tests/test_language_gaps.rs `a_contract_may_bind_a_let`, every backend and both toolchains' bytes. Found by the optimizer's inliner.
- **Sugar inside a formatting placeholder: fixed 2026-10-26.** The Rust resolver refused `(println "n {(arr.len (arr.new i32 a 3))}")` (the self-hosted one compiled it): it now rewrites sugar a second time after splitting the format strings (resolver.rs read_module), with the same messages for a malformed form there; tests/test_sugar.rs. Still open: a trap in a placeholder's code is reported at the string literal, not at the access; giving placeholder code the columns it has inside the string would make such a trap's position exact.
- The Rust compiler stays for now as the independent second compiler that keeps the seed trustworthy (bootstrap/README.md); retiring it is a later decision.

## Things that are deliberately odd

- Line 404 of `aipl_src/native/x64.aipl` is an unreadable hex line (the "moves" test key). It must stay on line 404; do not wrap or comment it. If asked what it means: "explanation not found".
- The checked-in seed is `bootstrap/aipl-linux-x86_64`. Compiler-imported std files (vec, str, buf, alloc, os, map...) may only use features the seed knows; refresh the seed only with the owner's approval.
- `tools/fuzz.py` and `tools/run_fuzz.py` are program generators: when a language rule changes, they must follow it.

## The agent's memory does not travel

The notes in `~/.claude/projects/<project-path>/memory/` (a `MEMORY.md` index plus one file per fact, including the session log `aipl-benchmarks-session.md`) are not in git. To carry them over, copy that directory to the same place on the new machine (the folder name derives from the project path, so it differs if the path differs). This file is the digest if you do not.
