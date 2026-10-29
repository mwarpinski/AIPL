# System host imports: `chmod` and processes (S1 of aipl_standalone.md)

**Status: proposed (2026-10-28), D1-D7 approved by the owner the same day.
All four steps are built.** Written in the style of NET_PLAN.md, whose
`aipl_net_v1` this copies in shape: AIPL's own import module beside
WASI preview 1, provided by the VM and aipl-run in Rust, lowered to Linux
system calls by the native backend (written in AIPL), granted on the
command line and fixed in an executable when it is compiled.

## At a glance

| Step | What | Status |
|---|---|---|
| 1 | `fs.chmod`: the execute bit, in every backend and both toolchains | **Done** 2026-10-28 (section 6) |
| 2 | The process operations `proc.*` and `--allow-run`, in every backend and both toolchains | **Done** 2026-10-28 (section 7) |
| 3 | `std/proc`: the safe interface (run a program and capture its output; a `Child` with pipes) | **Done** 2026-10-28 (section 8) |
| 4 | A seed refresh, so the self-hosted `aipl` can use them (S2 of aipl_standalone.md) | **Done** 2026-10-29 (section 9) |

## 1. Why a new import module

WASI preview 1 has no call that sets a file's mode, starts a process,
or replaces one. Everything else the Rust command needs from the system
it already gets from the host or has (`fs.stat`, `os.arg`, sockets).

The imports go in a module of AIPL's own, `aipl_sys_v1`, as sockets went
in `aipl_net_v1`. A program that uses none of them has none in its import
section, so a plain program still instantiates with WASI alone.

## 2. Decisions

**D1. `fs.chmod` needs no grant; it is confined like the other `fs.*`
operations.** Setting a mode on a file the program can already open for
writing, or delete, gives it no power it lacks: `fs.delete` is the larger
one. So `fs.chmod` follows the rules of `fs.open`: relative paths in the
working directory, absolute paths only if the program may use them, and
nothing outside the working directory under `--sandbox`. (The first draft
of aipl_standalone.md put `chmod` under a grant; this is simpler and no
weaker.) The mode is the permission bits only, 0 to 0o777; setuid,
setgid, and sticky are refused as an invalid argument.

**D2. Processes need a grant: `--allow-run PROGRAM`, repeatable, `*` for
any program.** Starting a process ends every promise a sandbox makes, so,
as `--listen` does for a port, a program names the programs it may start,
fixed when it is compiled (`aipl compile --exe --allow-run /usr/bin/cc`)
and given on `eval`, `run`, and aipl-run at start. A grant is the program
path exactly as the program will pass it to `proc.spawn`; no grant, no
imports that work (the operations answer -76, below). The `aipl` command
itself is built with `*`. A granted program runs with the working
directory and environment of its parent and is trusted: `--sandbox`
confines the program's own file access, not what a program it started
does.

**D3. The operations** (usable only inside `unsafe`, as `net.*` is; every
address and length is in linear memory):

| Operation | Meaning |
|---|---|
| `(fs.chmod path len mode)` | sets a file's permission bits: 0, or minus the WASI errno |
| `(proc.spawn path len args args_len stdio)` | starts the program at `path` (an exact path; the host searches nothing). `args` is `args_len` bytes holding the arguments, each followed by a zero byte, the first the program's own name. `stdio` is three 2-bit fields, standard input in bits 0-1, output in 2-3, error in 4-5: 0 shares the parent's, 1 is a pipe to the program, 2 is the null device, 3 (error only) is the same as its output. Gives a process handle, or minus an errno |
| `(proc.stream p which)` | the pipe for a piped stream of process `p` (0 its input, 1 its output, 2 its error), as a handle, or -1 if that stream is not a pipe |
| `(proc.read h buf max)`, `(proc.write h buf len)` | bytes moved, 0 at the end of a read, minus an errno on an error; a write to a program that has gone is an error, not a signal |
| `(proc.close h)` | closes a pipe (closing the input is how a program is told the input has ended) or releases a process handle: 0, or minus an errno |
| `(proc.wait p)` | waits for the program to end: its exit status 0 to 255, or 256 plus the signal number if a signal ended it; minus an errno on an error. A second wait gives the same answer |
| `(proc.exec path len args args_len)` | replaces this process with the program: it does not return, except with minus an errno when it cannot |

**D4. The host takes an exact path.** No `PATH` search in the host, so no
backend has to parse an environment variable the same way; `std/proc`
searches `PATH` in AIPL (with `fs.stat`) and passes the path it found.

**D5. Handles are numbered apart from `fs` descriptors**, as sockets are
(NET_PLAN.md section 3), and a handle a program was not given is refused
(minus the errno for a bad descriptor), never another file's. A process
the program started is waited for only by it. Natively a pipe's handle is
the Linux descriptor and a process's is its process id, each checked
against a table of those this program opened, as sockets.aipl checks
sockets.

**D6. Errors are minus the WASI errno, as `fs.*` gives** (the same number
in every backend); a program that was not granted is minus 76 (WASI's
`notcapable`), which std/proc says as "not granted: run the program
with --allow-run PROGRAM".

**D7. What is not here yet:** a signal to a program (`kill`; a test
runner wants one for a timeout), waiting for any of several programs,
a different working directory or environment for the program, and
reading two pipes without blocking on one. A caller reads output and error
through one pipe (`stdio` 3), or reads one in a thread (AIPL has them).

## 3. Each backend

- **The VM and aipl-run** (src/vm.rs, src/net.rs's neighbor, src/bin/aipl_run.rs):
  `std::process::Command` with piped stdio, `CommandExt::exec`, and
  `set_permissions`, behind one table of handles. The path rules for
  `fs.chmod` are those the file operations already apply in each.
- **Native executables** (aipl_src/native/): Linux system calls from
  new routines beside sockets.aipl: `pipe2`, `fork`, `dup2`, `execve`
  (the arguments' pointer table built on the stack from the block, the
  environment the process started with), `wait4`, `read`, `write`,
  `close`, and, for `fs.chmod`, the file opened as `fs.open` opens it
  (the same confinement) and `fchmod` on that descriptor. fresh.aipl
  already forks, pipes, redirects, and waits, and is the model.
- **Both compilers** emit the same import and the same bytes: the
  operations are new op codes in each parser, checker, and code
  generator, as `net.*` were (a language-level change in src/ and
  aipl_src/, tested by the byte-for-byte comparison).
- **The grant** travels as `--listen` does: a list of strings fixed in a
  native executable or a launcher bundle's trailer (a new flag bit), and
  checked at the call. The self-hosted `compile` takes `--allow-run` too.

## 4. Order, and the seed

Step 1 first: one operation through every layer, which finds each
wiring problem once, before the process operations multiply it. Step 2
adds the seven process operations and the grant. Step 3 is std/proc.

**The compiler cannot use the operations in its own source until a seed
has them**, since the seed that builds it must understand them. So step 4
is a seed refresh (the owner's approval, as always), and S2's commands
in aipl_src/aipl.aipl come after it.

## 5. Tests

tests/test_sys.rs, as tests/test_net.rs is built: one program per
operation, the same bytes from the VM, aipl-run, a native executable, and
the launcher bundle; both toolchains' bytes alike; refusals alike
everywhere (an ungranted spawn, a handle that is not the program's, a mode
with setuid, a path outside `--sandbox`); a child that echoes its input,
one that exits with a status, one killed by a signal, one that does not
exist; the descriptors the program holds are the same before and after
(no leaked pipes); `fs.chmod` on a file the program wrote, then run.

## 6. Step 1, as built (2026-10-28)

`(fs.chmod ptr len mode)`, an operation usable only in `unsafe`, in both
toolchains (src/ and aipl_src/: parser, printer, checker, code generators),
the VM, aipl-run, and native executables.

- **The import is `aipl_sys_v1.chmod(dirfd, path, len, mode)`,** not
  (path, len, mode): the compiled code chooses the directory as `fs.delete`
  does (3, the working directory, or 4, `/`, the path then without its
  first slash), so every host sees what the WASI file operations see and
  `--sandbox` means one thing. D1's rule is therefore: a relative path may
  never leave the working directory (`..`, or a symbolic link out of it:
  `-63`), `--sandbox` or not, as wasmtime's preopens refuse it; an absolute
  path is unconfined unless the program was built with `--sandbox`, which
  gives it no `/` (`-8`). The VM's paths are ordinary, as they are for every
  `fs.*` operation.
- **Results** are 0 or minus the WASI errno; a mode with a bit outside 0o777
  is -28; a path whose length runs past memory is -21, a value and not a
  trap (the host checks it; the compiled prelude has already read the
  path's first byte, so a path that *starts* outside memory traps there, as
  `fs.delete` does).
- **Natively** (aipl_src/native/wasi.aipl, `emit_sys_chmod`): the file is
  opened beneath its directory by the same `openat2` (or plain `openat`
  where that is refused: Rosetta 2, Linux before 5.6), read-only and
  without blocking, then `fchmod` and `close`. A file that cannot be read
  (mode 0) cannot be opened, so then the parent directory is opened and
  `fchmodat` names the file in it, unless a `newfstatat` says it is a
  symbolic link (EACCES). aipl-run and the VM use `chmod(2)` on the path
  (src/sys.rs), confined by a lexical check and `canonicalize` against the
  working directory: a check, then a use, where the native open is the
  kernel's own, so a program racing another process for the same directory
  is confined less tightly under aipl-run than natively.
- **The std wrapper** (added in step 4): `fs.chmod [path:str mode:i32] ->
  (result i32 io.Error)` and `chmod_path` in `std/fs`, with `fs.FILE` (0o644)
  and `fs.PROGRAM` (0o755). It waited for the seed refresh, since the compiler
  imports `std/fs` and the earlier seed could not read the operation.
- A path with a NUL byte in it ends there natively (as every native path
  operation does) and is an error in the VM and aipl-run.
- Tests: tests/test_sys.rs (seven runs of one program, in the VM, aipl-run,
  native, and the bundle, with and without `--sandbox`; native where
  `openat2` is refused; both toolchains' bytes), and the operation in
  tests/test_opcode_conformance.rs.

## 7. Step 2, as built (2026-10-28)

The seven operations `proc.spawn`, `proc.stream`, `proc.read`, `proc.write`,
`proc.close`, `proc.wait`, and `proc.exec` (usable only in `unsafe`), as D3
has them, in both toolchains, the VM, aipl-run, and native executables, with
the grant `--allow-run PROGRAM` (`*` for any) on `aipl eval`, `aipl run`,
`aipl-run`, and `aipl compile --exe` (the Rust command and the self-hosted
one); an executable carries its grants (a native one in its data, a bundle
in its trailer, as `run:PROGRAM` lines beside the `--listen` addresses).

- **Rust hosts** (src/sys.rs, `Procs`): `std::process::Command` with our own
  pipes (`std::io::pipe`), a table of processes and one of pipes, each handle
  counting up (pipes from 3, processes from 1024), and blocking calls outside
  the table's lock. The VM, aipl-run, and its threads share one table.
- **Natively** (aipl_src/native/procs.aipl): `fork` and `execve`, with a pipe
  that closes on exec so the parent learns why a program did not start
  (a missing program is -44 everywhere, as under Rust's `Command`); pipes are
  descriptors checked against a table of the ones this program opened; a
  process is a slot in a table of 64 (a 65th live process is -33); `wait4`
  with the status decoded as D3 says and kept for a second wait; a write
  ignores SIGPIPE for the call.
- **Limits that differ,** all refusals with a value and not a trap: path and
  arguments together over 65,536 bytes is -1 in every backend; a native
  executable holds each granted path to 256 bytes (the compile says so) and
  at most 64 live processes; the Rust hosts keep no such cap on processes.
- **What a program sees is its parent's:** working directory and environment.
  `--sandbox` confines the program's own files, not a program it starts.
- **Not built (D7):** a signal to a program, waiting on several, a different
  directory or environment for the child, reading two pipes without blocking
  on one. `--allow-run` is not on `aipl eval -e`, `aipl serve`, or `aipl
  watch` yet. The tables are not locked natively (as sockets' are not): two
  threads starting programs at once may race for a slot.
- **Tests:** tests/test_sys.rs: one program through `cat` and `sh` (a pipe
  each way, an exit status, a signal, output and error through one pipe, a
  null stream, arguments with and without a final zero byte, the environment,
  every refusal) and one that `exec`s, identical from the VM, aipl-run, a
  native executable, and the bundle; the grants (none, one, two, `*`) in each;
  both toolchains' bytes; and the seven operations in
  tests/test_opcode_conformance.rs.

## 8. Step 3, as built (2026-10-28)

`std/proc` (aipl_src/std/proc.aipl), the safe interface over step 2, with no
`unsafe` for a caller:

- `Command` (`command`, `command_path`, `arg`, `arg_bytes`, `free_command`):
  the path (also the program's own name) and the arguments as the host takes
  them, one block, each followed by a zero byte; its memory is the
  allocator's.
- `run c input out` to the end: its status in `(result i32 proc.Error)`, with
  everything it wrote, its error too, added to `out`; an empty input is the
  null device. `spawn c` for a conversation (a pipe each way, the error
  merged into the output) or `spawn_with` for any of `Stream.inherit`,
  `pipe`, `null`, `merged`; on the `Child`: `write`, `close_input`, `read`,
  `read_error`, `wait`, `close`. `exec c` replaces the program and returns only
  the `Error` it failed with. `exited` and `signal` read a status.
- `find name out` looks a name up in the PATH (a name with a slash is a path
  already); it finds a file, as `fs.info` sees it, and leaves whether it may
  be run to the system.
- `Error` is `not_granted` (-76), `not_found` (-44, -54), `denied` (-2, -63),
  or `other`, with `message`.
- Not here: a timeout or a signal (D7), reading output and error as two
  pipes without blocking on one (read one in a thread), a program's working
  directory or environment.
- **Found on the way, not fixed:** the two toolchains print an enum constant
  inside a `req` differently in the contract's failure message (the Rust one
  `(enum.cast proc.Stream 3)`, the self-hosted `3`), and the self-hosted
  printer drops `enum.ord` there too, so std/proc keeps enum values out of its
  contracts. A local file named like a standard module (a test program called
  `proc.aipl`) shadows it: the tests' programs are `spawn_ops.aipl` and
  `spawn_exec.aipl`.

## 9. Step 4, as built (2026-10-29)

The owner approved the seed refresh the same day. In order:

- **`std/fs` has `chmod` and `chmod_path`** (`(result i32 io.Error)`, with
  `fs.FILE` 0o644 and `fs.PROGRAM` 0o755). The compiler imports `std/fs`, so
  from this commit the seed has to read `fs.chmod`.
- **The toolchain's own wasmtime hosts define `aipl_sys_v1.chmod`**
  (`sys::add_chmod`, in src/selfhost.rs, src/native.rs, src/tools.rs): the Rust
  `aipl` runs the self-hosted compiler (the optimized build) and the native
  backend under wasmtime, and a module that names an import must find it to
  start. Found when the first `aipl compile` after the `std/fs` change stopped at
  "unknown import `aipl_sys_v1::chmod`". Process operations need no such
  stub yet: no module the Rust `aipl` runs this way imports `std/proc`.
- **The self-hosted `compile --exe` makes its output executable**
  (aipl_src/aipl.aipl), as the Rust command does (mode 0755 whatever the
  umask, an existing file included). `bootstrap/build.sh`, `tools/make_kit.sh`,
  and `tools/container_test.sh` lost their `chmod +x`; `test_bootstrap` asserts
  the mode of every executable the seed and its stages write.
- **The seed** was rebuilt by the Rust toolchain from 20888dd (sha256 in
  bootstrap/README.md): a fixed point, byte for byte its own stage 2 and 3.
  It is the first seed that sets the execute bit, and its source can name every
  `aipl_sys_v1` operation, so the S2 commands can start programs.
- **Not done, and found on the way:** the self-hosted command still writes its
  output in place (`io.write_path` truncates and writes), where the Rust one
  writes a new file and renames it (a program replacing the very executable
  running it would stop with "text file busy"). AIPL has no rename operation
  yet; it belongs to S2's `run` and `watch`, which write and start.

## 10. `fs.rename` (2026-10-29)

Asked for by the owner right after step 4, because writing a file and renaming
it into place is how a program replaces another safely (the Rust command does
it; `run` and `watch` will need it).

- **`(fs.rename old_ptr old_len new_ptr new_len)`**, `0` or minus a WASI errno,
  is WASI's own `path_rename` (no `aipl_sys_v1` import, so `aipl-run` and
  any WASI host give it as they give `fs.delete`, with each path's directory
  chosen the same way: fd 3, or 4 for a path from `/`). It needs no grant, and
  is confined like the other file operations.
- **Natively**, each path's parent directory is opened beneath its directory
  (the way `path_unlink_file` does, shared now as `emit_split_parent`) and
  `renameat` moves the last component between the two parents. The VM uses
  `std::fs::rename`.
- **The same answers everywhere**, which the test checks against all seven
  runs (VM, aipl-run, native, bundle, each with `--sandbox`) and the files left:
  a file replaced, a directory moved, a link moved itself, a directory onto a
  non-empty one `-55`, a directory onto a file `-54`, a file onto a directory
  `-31`, a `..` out of the directory `-63`, no `/` under `--sandbox` `-8`.
- **`path_rename` is the last import**, after `exec`, so no existing program's
  import order or bytes change (the test that the seed's own build is
  unchanged is test_bootstrap).
- **Not yet: the std wrapper.** `fs.rename [from:str to:str] -> (result i32
  io.Error)` belongs in `std/fs`, which the compiler imports; the seed cannot
  read the new operation, so it lands with the next seed refresh, together with
  the self-hosted `compile` writing a new file and renaming it into place
  (the Rust command's way, which does not stop when the output is the program
  running).
