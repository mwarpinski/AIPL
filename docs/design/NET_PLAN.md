# Networking: serving and fetching (stdlib batch L7)

**Status: draft for review, 2026-10-19.** Nothing is built. STDLIB_PLAN.md
asked for this document before any code: "an `aipl` host interface
(versioned imports beside WASI preview 1: listening and connecting TCP
sockets, permissions granted on the command line), in the VM, natively as
Linux system calls, and in aipl-run; then `http` serving and fetching
over it". Writing it changed the plan's first half; the reasons are
below, and the decisions for the owner are at the end.

## 1. The problem a server has that a compiler does not

AIPL programs so far run and exit: a compile, a test, a data job. A
server runs for days. Three facts about memory today make that the hard
part, harder than the sockets:

- `new`, `arr.new`, and every run-time `str` are never freed
  (AIPL_SPEC.md 4.B, 4.E), by design: a program frees what it allocates
  through `std/alloc`, and the rest goes when it exits.
- Every `(ptr str.Bytes)` view is made with `new` (std/str.aipl:10-14),
  so `str.slice`, `str.from_str`, and `buf.bytes` leak 16 bytes per call.
  Views are everywhere: `(ptr str.Bytes)` appears 377 times in std and
  1,308 times in the compiler's own source. std/http's reader makes a
  view per header and per part of the start line.
- Memory is at most 2 GiB (wasm32).

A server reading 10,000 requests a second with ten headers each makes
at least 23 views a request (three for the start line, a name and a value
per header): 368 bytes, about 3.7 MB a second, so it fills 2 GiB in about
ten minutes from views alone. Making `str.Bytes` a value struct fixes the
views (it is two numbers) but changes about 1,700 places across std and
the compiler, and the `str` values and `new` objects a handler makes would
still pile up. Resetting the memory `new` uses between requests is not
safe: a `new` object's generation is 0, which no check catches, so a
pointer kept across requests would read whatever came next, silently.

## 2. The proposal: a fresh instance for every request

Do what CGI did and what WebAssembly's serverless hosts do (Fastly
Compute, Fermyon Spin): the host owns the listening socket, and each
request runs the program from the start in a fresh instance. The
program reads one request on standard input and writes one response on
standard output, exactly the shape std/http was built for ("a program
can read a saved request, or one piped through stdin, and answer it on
stdout", STDLIB_PLAN.md section 4).

```
aipl serve shop.aipl --listen 127.0.0.1:8080        (the VM)
aipl-run --listen 127.0.0.1:8080 shop.wasm          (wasmtime)
./shop --listen 127.0.0.1:8080                      (native, compiled with --serve)
```

The host accepts a connection, reads one HTTP/1.1 request (with the
limits std/http applies), starts the program with that request on stdin,
sends what it writes to stdout back as the response, and closes the
connection (no keep-alive in the first version). The handler is an
ordinary AIPL program:

```lisp
(module hello
  (import http)
  (import str)
  (import stream)
  (import buf)
  (import alloc)
  (fn main [] -> i32
    (let a:alloc.Allocator (call alloc.default))
    (let input:(ptr buf.Buf) (call buf.make a 4096))
    (let r:(ptr stream.Reader) (call stream.stdin a))
    (let _n:i32 (call stream.read_all r input))
    (let o:(ptr buf.Buf) (call buf.make a 256))
    (match_result (call http.parse_request a (call buf.bytes input) (call http.default_limits))
      (ok m
        (call http.start_response o 200)
        (call http.add_header o "Content-Type" "text/plain; charset=utf-8")
        (call http.end_head o (get m http.Message.target)))
      (err e
        (call http.start_response o 400)
        (call http.end_head o (call str.from_str (get e http.Error.what)))))
    (call buf.print o)
    0))
```

What this buys:

- **Nothing leaks across requests**, whatever the handler allocates: the
  instance is gone when the response is sent. The memory problem of
  section 1 does not arise, now or later.
- **A crash affects one request.** A failed contract, a bounds check, or
  a use after free ends that request with a 500 and its message in the
  server's log; the next request starts clean. A long-lived server
  cannot promise that.
- **No sockets, threads, or event loop in the program.** Concurrency is
  the host's: it runs requests side by side, each in its own instance,
  so a handler needs no `unsafe` and no locks.
- **Every handler can be tested without a network.** A saved request
  piped into the program is the whole test; `aipl test` and the
  three-backend comparison work on it unchanged.
- **The same in all three backends**, by construction: the host part is
  small and the program is the program.

What it costs, and what needs care:

- **The VM's own lines.** `aipl eval` prints `[AIPL Result]: ...` on
  standard output after a program (checked 2026-10-19 with the handler
  above); under `aipl serve` that has to go to the log, not into the
  response.

- **No state in memory between requests.** A counter, a cache, or a
  session lives in a file (or, later, a database). This is the model of
  CGI, PHP, and serverless functions; it does not suit a chat server or
  a game.
- **Starting an instance per request.** Under wasmtime a module compiled
  once is instantiated in microseconds (wasmtime's `InstancePre`); a
  native program forks a copy of itself made before the first request
  (Linux copies its memory only as it is written). Both are to be
  measured; the target is thousands of requests a second on one core.
- **Streaming bodies.** A request is read whole (up to the limit) before
  the program starts; responses stream as the program writes them. Big
  uploads wait for a later version.

## 3. Fetching: TCP connections from the program

A program that calls another service needs an outgoing connection, and
WASI preview 1 has no way to make one (it has `sock_accept`, `sock_recv`,
and `sock_send` on sockets the host hands it, but no `connect`). So this
part is the plan's host interface: a versioned import module,
`aipl_net_v1`, beside `wasi_snapshot_preview1`, implemented in each
backend:

| Function | Meaning |
|---|---|
| `connect(addr, len, port) -> fd` | a TCP connection to an IPv4 or IPv6 address given as text, or -1; refused unless granted |
| `recv(fd, buf, max) -> n`, `send(fd, buf, len) -> n` | blocking reads and writes; `fs.close` closes it |

Granted on the command line, nothing by default: `--allow-connect
HOST:PORT` (repeatable; `*` for any port). A native executable gets its
grants when it is compiled (`aipl compile --exe --allow-connect ...`),
as `--sandbox` already works (aipl_src/native/native.aipl:319), so a
binary says what it may reach and cannot be talked into more.

Then `std/net` wraps it (a `stream.Reader` and `stream.Writer` over a
connection, so std/http's reader and writer work on it) and `http.fetch`
sends a request and reads the response:

```lisp
(match_result (call http.fetch a "http://127.0.0.1:9000/stock?item=7" (call http.default_limits))
  (ok m ... (get m http.Message.status) (get m http.Message.body) ...)
  (err e ...))
```

Two things it will not do in this batch, and why:

- **No `https://`.** TLS is encryption, and STDLIB_PLAN.md decided
  against encryption written here ("ours would be unaudited"). A program
  fetches from services on the same machine or network over plain HTTP,
  or through a local proxy that speaks TLS. Getting TLS from the host
  instead (Rust's rustls under aipl-run and the VM) would leave native
  executables without it and the backends different, which this project
  does not allow.
- **No names, only addresses, in the first version.** Natively there is
  no libc, so no system resolver: looking up `example.com` needs a DNS
  client written in AIPL (UDP, /etc/resolv.conf, /etc/hosts). The VM and
  aipl-run could ask the system, but then the backends would differ.
  `localhost` and /etc/hosts entries are cheap to support everywhere;
  DNS proper is its own small batch.

## 4. Order of work

1. **`aipl serve` and `--listen`** in the VM, aipl-run, and native
   (`--serve` on `aipl compile --exe`), with std/http as the handler's
   reader and writer. Tests: real requests over loopback to a handler
   in each backend, answers compared byte for byte; refusals (too big,
   malformed, a handler that fails) get the same status everywhere; a
   load test that serves a million requests and shows memory flat.
2. **`aipl_net_v1.connect`** with `--allow-connect`, in the three
   backends; `std/net` streams over it; `http.fetch`. Tests: fetch from
   step 1's server in each backend; a refused grant fails with a clear
   message.
3. **Measure** requests a second per backend on a quiet machine and
   write it in docs/BENCHMARKS.md, as for everything else.

Each step is tested in every backend and both toolchains, documented in
AIPL_SPEC.md and the prompt guide, and merged before the next.

## 5. Decisions for the owner

1. **A fresh instance per request (recommended)**, or long-lived servers
   with sockets in the program. The second needs, first, `str.Bytes` as
   a value struct (about 1,700 places), an end to every per-call leak in
   std, threads or an event loop in programs, and still cannot recover
   from a crash in one request. It could come later for programs that
   need memory between requests; the first serves the common case safely
   now.
2. **Fetching over plain HTTP to addresses only, in this batch
   (recommended)**, with DNS as a later small batch; or DNS now (a
   resolver in AIPL, about a week); or host-provided name lookup in the
   VM and aipl-run only (the backends would differ: not recommended).
3. **No TLS (recommended, as decided in STDLIB_PLAN.md)**, or TLS from
   the host in the VM and aipl-run only (not recommended: native
   executables would lack it).
4. **Native grants at compile time (recommended, as `--sandbox`)**, or at
   run time from the program's command line (simpler to use, but then a
   binary's reach depends on how it is started).

## 6. What stays open after this batch

- `str.Bytes` as a value struct still matters for long batch jobs; it
  stays on the launch checklist (from docs/notes/pitfalls.md), no longer
  blocking networking.
- Keep-alive and request streaming, if measurements show the need.
- WebSockets, servers that hold state, UDP, and DNS: later batches,
  each with its own short plan.
