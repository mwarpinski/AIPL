# AIPL standalone: nothing Rust can do that AIPL cannot

**Status: proposed (2026-10-27); S1 done 2026-10-29.** A roadmap, not a design: each step below
gets its own plan before code, as NET_PLAN.md did for sockets (S1's is
SYS_PLAN.md, whose grant model the owner approved).

## The goal

AIPL is a complete programming language that runs on the machine, not on
a runtime written in another language. The owner's rule: if Rust can do
it, AIPL can do it, and AIPL should. Apart from work that is simply large
(TLS), a thing that only Rust can do is a gap in AIPL to close, not a
limit to document.

**What stays Rust.** The Rust toolchain stays as the independent second
compiler: `tests/test_bootstrap.rs` builds the same source with both and
requires the same bytes, which is what keeps a self-hosted compiler
honest (bootstrap/README.md, "Why the Rust compiler stays"). It lives in
the test suite and CI. It is not in a user's path.

**Done means**, for Linux x86-64 first: a checked-in `aipl` (the seed, or
one built from it) does everything the Rust `aipl` does, with no `cargo`,
no wasmtime, and no `chmod` in a shell script; and `cargo` is needed only
to run the referee tests.

## Where we are

| What | In AIPL today | Only in Rust today |
|---|---|---|
| Compile to `.wasm`, to a native executable (`--sandbox`, `--listen`, `--max-memory`, `--no-opt`), `verify`, `doc`, `--version` | the self-hosted command (aipl_src/aipl.aipl) | |
| The front end (resolve, generics, check) | aipl_src/ | the same, kept equal by tests and the fuzzer |
| `run` (a `.wasm` or a source file) | | `aipl-run` (wasmtime) and the VM |
| `test` | | `src/testrun.rs`: builds a native test executable, starts it once per test, compares VM, wasm, and native |
| `watch` | | `src/watch.rs`: polls modification times, re-runs |
| `serve` | | `src/main.rs`: starts a fresh program per connection with its stdin and stdout on the socket |
| `eval` | | the VM (`src/vm.rs`, a tree-walking interpreter) |
| Executables off Linux x86-64 | | the launcher bundle (aipl-run, wasmtime, about 18 MB) |
| Listening on TCP | std/net over `aipl_net_v1` | `aipl-run` supplies the same imports |
| Connecting out, TLS | | not built anywhere (NET_PLAN.md, steps 5-7) |

## Steps

Each step ends with something a person can run and a test that holds it.
Rust stays the referee throughout: every AIPL command is tested against
the Rust one on the same inputs.

### S1. The system host imports (`aipl_sys_v1`)

**Done 2026-10-29** (SYS_PLAN.md, sections 6-9): `fs.chmod` and the process
operations in every backend and both toolchains, `std/proc`, and a seed that
sets the execute bit itself. `fs.rename` (section 10) and `std/fs`'s `rename`
followed, and the self-hosted `compile` now writes its output beside it and
renames it into place, so a compiler can replace the file it is running from.
`remove`, `mkdir`, and `rmdir` (sections 10 and 11) complete the file operations
a program needs to manage the files it writes.
The text below is the plan as it was.

The one missing piece under most of the table. A host import module in the
style of `aipl_net_v1`: lowered to system calls by the native backend
(aipl_src/native/), supplied by `aipl-run` for the wasm path, wrapped by a
std module.

- `chmod` (the execute bit), so a written executable is runnable and
  `bootstrap/build.sh` loses its `chmod +x`;
- start a process with arguments, wait for it, read its exit status;
- pipes for a child's standard input, output, and error, with the output
  captured;
- replace this process with another (`exec`), for `run`.

The design is SYS_PLAN.md. **Decision for the owner:** starting a process
breaks a sandbox, so, like sockets, it needs a grant fixed at compile time
(`--allow-run PROGRAM`, `*` for any); `chmod` needs none, being confined
like the other file operations (SYS_PLAN.md, D1 and D2). The `aipl` command
itself is built with `*`.

Done when: the seed builds itself with no shell `chmod`; a test program
starts a child, feeds it input, reads its output and status, under the
native backend and `aipl-run`, byte for byte alike; a program built
without the grant cannot name the imports.

### S2. The commands that need only S1 (Linux x86-64)

In this order, smallest first; each lands in aipl_src/aipl.aipl and a test
runs it against the Rust command:

1. `verify` (the AIPL front end exists; no new import) **done 2026-10-29**: `driver.verify_file` stops after the checks, and tests/test_bootstrap.rs compares its output, errors, and status with the Rust command's on every example, benchmark, and test program;
2. `run`: compile to a native executable and run it (`.wasm` input waits
   for S3). **Done 2026-10-29** for source files: `aipl run [FLAGS] FILE.aipl
   [ARGS]` builds into `$TMPDIR/aipl-run-<time>` (else `/tmp`), starts it on
   its own standard streams with ARGS, removes it, and exits with its status
   (128 plus the signal if a signal ended it). It starts the program and waits
   rather than `exec`ing it, so it can clean up. Because the command starts a
   program, it is built with `--allow-run '*'` (build.sh, tests, the kit);
   the Rust command's `run` takes only a `.wasm` file;
3. `test`: build the test executable, start it once per test, compare
   output. It loses the VM column; the fuzzers and the test suite keep the
   VM cross-check. **Done 2026-10-29** (aipl_src/testcmd.aipl, resolver.test_names):
   `aipl test FILE|DIR [-k TEXT] [--native]` prints the Rust command's lines,
   failures, counts, and status, byte for byte on the corpus (tests/test_testing.rs).
   Left in Rust: `--vm`, `--all-backends`, and a file with no `(test ...)`, whose
   entry function the Rust command runs in the VM (that is how CI runs
   aipl_src/test_suite.aipl). A failed test's output and error come as one
   stream, in the order written, where the Rust command prints the output and
   then the error; nothing parses either;
4. `watch`: poll `fs.stat`, run again;
5. `serve`: a fresh program per connection, pipes on the socket (std/net
   exists);
6. `eval`: compile and run; the VM's result line stays, or it is dropped
   by decision (REPL_PLAN.md).

Done when: every Rust command except `eval`'s VM-only output has an AIPL
equivalent in the seed-built `aipl`, and `tools/ci.sh` runs the AIPL ones
as the main path with the Rust ones as the check.

### S3. Running WebAssembly without wasmtime

On Linux x86-64 the native backend already turns wasm into machine code
(wasm_reader.aipl, lower.aipl, x64.aipl), so `aipl run x.wasm` is
translate, write, exec. The rest:

- a native ARM64 backend, Linux first (ARM64_PLAN.md, proposed), which
  also gives Apple Silicon Linux containers and Rosetta-free Macs a real
  executable;
- macOS needs a Mach-O writer and code signing (ARM64_PLAN.md, section on
  Darwin) or, until then, the launcher bundle;
- a WebAssembly interpreter in AIPL, for any platform without a native
  backend. Slower, small to ship, and a way to run `.wasm` anywhere the
  seed runs.

Done when: `aipl run` of a `.wasm` file works on Linux x86-64 with no
`aipl-run`; for each other platform, the chosen path is stated and tested.

### S4. The VM in AIPL

The tree-walking interpreter is the semantic reference for `eval`, a REPL,
and `aipl test --vm`. Rewrite it in AIPL once the compiled path covers
`eval` (S2), checked against the Rust VM on the fuzz corpus
(tools/run_fuzz.py). Done when `aipl eval` needs no Rust.

### S5. Connecting out, and TLS

NET_PLAN.md steps 5-7: `connect`, a grant for it, `http.fetch`. TLS is a
project of its own (record layer, handshake, certificate validation,
the primitives, each checked against test vectors and a reference
implementation) and should have its own plan; until then fetching is
plain HTTP, and the README says so.

### S6. Other platforms

Windows does not build today (`std::os::unix` in the Rust source). A native
Windows target needs a PE writer and its own system interface, so it is a
new backend, not a port of the Rust code. Not planned until S1-S4 are done.

## Order, and what it does not block

S1, then S2, are the work: they are small steps with a clear test, and they
turn the seed into the whole command on Linux x86-64. S3 (ARM64 first) is
next because it is what the other platforms need. S4 and S5 are large and
can follow in either order. None of it blocks showing the project:
README.md and ROADMAP.md say what Rust is still needed for.

## Open questions

- The grant model for starting processes (S1): a flag at compile time, as
  sockets have, is the recommendation. Alternatives: a runtime prompt (no:
  agents cannot answer one) or no grant (no: it ends the sandbox).
- Whether `aipl test` keeps a VM column once the VM is AIPL (S4).
- Whether the WebAssembly interpreter (S3) is worth building before ARM64
  and Mach-O, or after.
