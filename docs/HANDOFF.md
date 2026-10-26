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

1. **Where the work stands (2026-10-25):** CANONICAL_PLAN.md steps 1-4 are on `development` and `main` (6b54c16). The optimizer, docs/design/OPT_PLAN.md (the owner's priority: faster programs), steps 1-3 are done on branch `features/optimizer`; steps 1-2 are merged into `development` (491034e). `aipl compile` optimizes by default (`--no-opt` does not), both commands give the same bytes, compiled toolchain modules are cached in ~/.cache/aipl, tools/opt_diff.py compares optimized and unoptimized builds in CI, and docs/BENCHMARKS.md is re-measured both ways. Next: step 4 (the biggest gaps to C: knucleotide, nbody natively, pidigits), then merge to `main` when the owner says the optimizer is done. Tests that compare a compiled build with the VM's addresses or call chains, or with the Rust code generator's bytes, must pass `--no-opt`.
2. **docs/design/CANONICAL_PLAN.md (approved 2026-10-20), steps 4-7.** Step 1 (docs) is done 2026-10-21: the guide's "Start here", the `and`/`or` checklist fix, `aipl --version` with the commit and the same line on the kit's docs, tools/ci.sh and ci-status.sh. The guide's table of ten pitfalls must follow steps 3-5 as they land. Step 2 (messages that name the right form) is built 2026-10-21 (one row left, in CANONICAL_PLAN.md section 2). Step 3 (`loop` retired, `for` only; internal form `counted_loop`) is built 2026-10-21, tools/retire_loop.py kept. Steps 4-7: `when` is the only one-branch `if` (empty `(block)` else becomes an error), an array literal `(arr.of T a x y z)`, a sweep for other duplicate forms, then an A/B harness across model families before any of the syntax proposals (inferred `let` types, `(f x)` without `call`, and so on). The owner's answers are recorded in that plan; the Sonnet review's verdicts are in docs/NOTES_REVIEW.md. After retiring `loop` or changing syntax the seed (`bootstrap/`) must be refreshed (ask the owner first); keep both toolchains (Rust in `src/`, self-hosted in `aipl_src/`) byte-for-byte equal.
3. **NET_PLAN.md step 5:** `connect`, `--allow-connect`, `http.fetch` (plain HTTP to addresses first; DNS later), tested against std/serve in each backend. Then step 6 (TLS from the host through rustls; native refuses) and step 7 (measure requests a second in docs/BENCHMARKS.md).
4. **Small leftovers:** a too-long (over 64 MiB) answer in fresh mode gives a 500 but logs no reason; `aipl eval` panics printing its status lines when stdout is a full disk; an "Out of memory" message at `--max-memory` (docs/LAUNCH_CHECKLIST.md).
5. **Offered, not yet asked for:** a long fuzz job (many seeds, hundreds of thousands of cases) on a schedule or overnight. It found a real bug within minutes on eight extra seeds. Also a possible `CONSTANT_TIME_PLAN.md` (secret types, `select`/`cmov`, tests that compare branches and addresses taken for different secrets) as post-launch work.
6. Later: the worker-thread pool for servers, keep-alive, WebSockets/UDP/DNS, the packages registry (after launch), the training corpus, the aiOS vision (docs: ROADMAP.md).

## Open problems

- **tests/test_serve.rs's intermittent failures: found and fixed (2026-10-25).** They failed about one run in five, locally and on GitHub. Two causes, both in the tests' `free_port` (tests/test_serve.rs and tests/test_net.rs):
  1. It took ports with `bind(0)`, from the range every outgoing connection also uses, so one of the thousands of connections these tests make could take a port before its server bound it ("Address already in use"). Ports now come from 10000 up to the ephemeral range (/proc/sys/net/ipv4/ip_local_port_range), never twice in a run.
  2. The empty answers: it found a port free by listening on it and closing at once. Tests run as threads of one process while other tests start server processes, and a started process holds a copy of every open socket until it runs its program; so a probe, though closed, could stay listening for a moment in a half-started child. A client connecting then was reset ("Connection reset by peer"), and its server, which bound the port a moment later, never saw it. Shown by tools/flake/inherit.py (no process starting: 0 such connects in 2.1 million; four threads starting processes: 471 in 20 seconds), and by the logged failure (the client connected on its first try, 0.5 ms after the server was started, long before a VM server can listen). `free_port` now asks by connecting (refused: free) and never listens.
  After the fix: tests/test_serve.rs 20 runs of 20 passed (before: about 1 in 6 failed); tests/test_net.rs 5 of 5. tools/flake/ keeps the stress scripts and inherit.py. Still open, found on the way: killing `aipl serve --fresh` by its process id leaves its aipl-run child serving (CANONICAL_PLAN.md section 4 notes it).
- **A `let` inside a contract** (`(req (block (let m:i32 (+ n 1)) (gt m 0)))`) passes `aipl verify` and runs in the VM, but both code generators refuse it ("Unbound local variable 'm'"; the self-hosted one, compile error 971): they take a function's locals from its body only. Found 2026-10-26 by the optimizer's inliner (which now leaves contracts alone). Fix: collect locals from `req`/`ens`/`inv` too, in src/compiler/wasm.rs and aipl_src/codegen.aipl alike, with a test in each backend.
- The Rust compiler stays for now as the independent second compiler that keeps the seed trustworthy (bootstrap/README.md); retiring it is a later decision.

## Things that are deliberately odd

- Line 404 of `aipl_src/native/x64.aipl` is an unreadable hex line (the "moves" test key). It must stay on line 404; do not wrap or comment it. If asked what it means: "explanation not found".
- The checked-in seed is `bootstrap/aipl-linux-x86_64`. Compiler-imported std files (vec, str, buf, alloc, os, map...) may only use features the seed knows; refresh the seed only with the owner's approval.
- `tools/fuzz.py` and `tools/run_fuzz.py` are program generators: when a language rule changes, they must follow it.

## The agent's memory does not travel

The notes in `~/.claude/projects/<project-path>/memory/` (a `MEMORY.md` index plus one file per fact, including the session log `aipl-benchmarks-session.md`) are not in git. To carry them over, copy that directory to the same place on the new machine (the folder name derives from the project path, so it differs if the path differs). This file is the digest if you do not.
