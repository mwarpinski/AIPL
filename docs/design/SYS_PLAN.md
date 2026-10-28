# System host imports: `chmod` and processes (S1 of aipl_standalone.md)

**Status: proposed (2026-10-28).** Step 1 (`fs.chmod`) is started; the
grant model (D2) and the process interface (D3-D7) are the owner's to
confirm before step 2. Written in the style of NET_PLAN.md, whose
`aipl_net_v1` this copies in shape: AIPL's own import module beside
WASI preview 1, provided by the VM and aipl-run in Rust, lowered to Linux
system calls by the native backend (written in AIPL), granted on the
command line and fixed in an executable when it is compiled.

## At a glance

| Step | What | Status |
|---|---|---|
| 1 | `fs.chmod`: the execute bit, in every backend and both toolchains | In progress |
| 2 | The process operations `proc.*` and `--allow-run`, in every backend and both toolchains | Not started |
| 3 | `std/proc`: the safe interface (run a program and capture its output; a `Child` with pipes) | Not started |
| 4 | A seed refresh, so the self-hosted `aipl` can use them (S2 of aipl_standalone.md) | Owner approves |

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
