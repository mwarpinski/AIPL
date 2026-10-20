# Networking: serving and fetching (stdlib batch L7)

**Status: approved 2026-10-19 (decisions in section 6), revised after the
memory work.** Nothing is built. STDLIB_PLAN.md asked for this document before any code:
"an `aipl` host interface (versioned imports beside WASI preview 1:
listening and connecting TCP sockets, permissions granted on the command
line), in the VM, natively as Linux system calls, and in aipl-run; then
`http` serving and fetching over it". The first draft (this morning)
argued that a long-running server was unsafe in AIPL and proposed only a
fresh instance per request. MEMORY_PLAN.md has since removed the reasons;
this version says what changed and what still differs between the two
ways to serve. The decisions for the owner are at the end.

## 1. What the memory work changed

The draft's case against a long-running server was memory:

| It said | Now (MEMORY_PLAN.md) |
|---|---|
| every view is made with `new` and never freed: 3.7 MB a second at 10,000 requests a second | a view is a value, three slots, nothing allocated (M1) |
| `new`, `arr.new`, and run-time `str` are never freed | `new` and `arr.new` name their allocator (F); what is never freed says so, `alloc.forever` |
| nothing shows which calls keep memory | `(no_leak)` proves a function, and everything it calls, gives back what it takes (M3) |
| results and unions make hidden cells | they are values (M4) |
| std leaks per call (io.print_int, float formatting, json numbers) | fixed, measured flat (M2); what remains is listed there |
| a pointer kept across requests reads whatever came next | every pointer, array, and view is generation-checked: it stops the program with "Use after free" |

So a server can now run each request on an arena, reset it after, mark
its handler `(no_leak)`, and the checker proves memory stays flat; a
pointer kept by mistake past the reset traps on its next use instead of
reading another request's data.

What did not change: **a trap ends the program.** AIPL has no
exceptions and no recovery from a failed contract, bounds check, or use
after free, by design (the program's state may be wrong). In a server
that runs for days, one bad request then takes the server down. That is
the real difference between the two ways to serve.

## 2. Two ways to serve, one handler

A handler is written once, the same for both:

```lisp
(fn handle [a:alloc.Allocator req:http.Message out:(ptr buf.Buf)] -> void (no_leak)
  (call http.start_response out 200)
  (call http.add_header out "Content-Type" "text/plain; charset=utf-8")
  (call http.end_head out (get req http.Message.target)))
```

`a` is the request's arena, `req` the parsed request, `out` the
response's buffer; `std/serve` (new) reads and writes the bytes, so the
handler does no I/O. Which way it runs is chosen when it is started:

**A. A long-running server.** The program owns the listening socket and
loops: accept, read a request, run the handler on an arena, write the
response, reset the arena. State lives in memory between requests (a
cache, a counter, a session map), in the program's own allocator.

- Fast: no start-up per request.
- Memory flat, proved by `(no_leak)` and checked by the generations.
- **A trap in one request ends the server.** It needs a supervisor that
  restarts it (systemd, a container's restart policy), and loses what it
  held in memory.

**B. A fresh instance per request** (CGI, and WebAssembly's serverless
hosts: Fastly Compute, Fermyon Spin). The host owns the socket and runs
the program from the start for each request.

- **A trap ends one request** (a 500 and its message in the log); the
  next starts clean.
- Nothing can outlive a request, `(no_leak)` or not.
- No state in memory between requests (a file or a database instead), and
  a start-up per request: microseconds under wasmtime (a module compiled
  once, instantiated per request); natively, a fork of a process made
  before the first request (Linux copies its memory only as it is
  written), which also keeps what was loaded at start-up.

```
aipl serve shop.aipl --listen 127.0.0.1:8080              (A, in the VM)
aipl serve --fresh shop.aipl --listen 127.0.0.1:8080      (B)
aipl compile --exe --serve shop.aipl -o shop && ./shop --listen 127.0.0.1:8080
```

Both are tested without a network the same way: a saved request piped
through the handler, in every backend.

## 3. The host interface: `aipl_net_v1`

WASI preview 1 has no way to listen or connect. A versioned import
module beside `wasi_snapshot_preview1`, implemented in each backend (the
VM in Rust's std::net, aipl-run as host functions, native executables as
Linux system calls written in AIPL):

| Function | Meaning |
|---|---|
| `listen(addr, len, port) -> fd` | a listening TCP socket, or -1; refused unless granted |
| `accept(fd) -> fd` | the next connection (blocking) |
| `connect(addr, len, port) -> fd` | a connection to an IPv4 or IPv6 address given as text, or -1; refused unless granted |
| `recv(fd, buf, max) -> n`, `send(fd, buf, len) -> n` | blocking reads and writes; `fs.close` closes |

Granted on the command line, nothing by default: `--listen ADDR:PORT`,
`--allow-connect HOST:PORT` (repeatable; `*` for any port). A native
executable gets its grants when it is compiled, as `--sandbox` already
works, so a binary says what it may reach and cannot be talked into more.
The VM and aipl-run take them at start.

`std/net` wraps sockets as `stream.Reader` and `stream.Writer`, so
std/http reads and writes over them unchanged. A stream's callbacks are
calls through function references, whose types carry the `(no_leak)`
promise since decision 9 of MEMORY_PLAN.md (built 2026-10-20), so a
`(no_leak)` handler may use streams.

## 4. Fetching

`http.fetch` over `connect`, a request written and the response read
with std/http:

```lisp
(match_result (call http.fetch a "http://127.0.0.1:9000/stock?item=7" (call http.default_limits))
  (ok m ... (get m http.Message.status) (get m http.Message.body) ...)
  (err e ...))
```

**HTTPS, from the host's TLS** (decision 4): the VM, aipl-run, and the
executables that bundle it (`aipl compile --exe --target wasm`) make and
accept TLS connections through rustls, an audited Rust library, with
the usual root certificates; `aipl_net_v1` gains `tls_connect` and
`tls_accept` (a certificate and key granted on the command line), and
the program reads and writes plain bytes either way. Native executables
cannot hold rustls, so a program that uses TLS is a compile-time error
there, naming the two ways out (`--target wasm`, or a TLS proxy in
front), as the backend support table has other explicit refusals. TLS
written in AIPL comes after launch (ROADMAP.md), labelled experimental
and unaudited until audited.

- **Addresses, not names, at first.** Natively there is no libc, so no
  system resolver; a DNS client in AIPL (UDP, /etc/resolv.conf,
  /etc/hosts) is its own small batch. `localhost` and /etc/hosts entries
  are cheap everywhere.

## 5. Order of work

1. **Decision 9** (MEMORY_PLAN.md): `(no_leak)` on function types, so
   streams can be used in a handler. Done 2026-10-20.
2. **`aipl_net_v1`** listen, accept, recv, send, and close, in the three
   backends, with the grants; `std/net` streams over them.
3. **`std/serve`** and the long-running server (A) in every backend; a
   lower memory limit (`--max-memory`, launch checklist section 6), so a
   runaway request fails cleanly. Tests: real requests over loopback,
   answers compared byte for byte across backends; refusals (too big,
   malformed) the same status everywhere; **a million requests with
   memory flat**; a handler that keeps a pointer past its request traps
   on the next use.
4. **The fresh instance per request (B)**, `aipl serve --fresh` and
   native `--serve` forking: a handler that traps answers 500 and the
   next request is served.
5. **`connect`, `--allow-connect`, and `http.fetch`**, tested against
   step 3's server in each backend.
6. **TLS from the host:** `tls_connect` and `tls_accept`, `https://` in
   `http.fetch`, a server with a certificate; native executables refuse
   it at compile time with the way out. Tests: a fetch from step 3's
   server over TLS with a test certificate, in the VM and under aipl-run;
   the refusal's message.
7. **Measure** requests a second per backend and mode on a quiet machine,
   in docs/BENCHMARKS.md.

Each step is tested in every backend and both toolchains, documented in
AIPL_SPEC.md and the prompt guide, and merged before the next.

## 6. Decisions (the owner's, 2026-10-19)

1. **Both ways to serve, long-running first:** a program uses whichever
   fits; the long-running server (A) is built first, since the fresh
   instance (B) reuses its parts (the handler, std/serve, the sockets)
   and adds only how the program is started.
2. **Concurrency: one request at a time, then a pool of worker threads**,
   each with its own arena ("walk, then run"). An event loop, if ever,
   later.
3. **Fetching over plain HTTP to addresses first**; DNS as a later small
   batch.
4. **TLS from the host (rustls) before launch**, in the VM, aipl-run, and
   the launcher-bundled executables; native executables refuse it at
   compile time with the way out. A TLS written in AIPL comes after
   launch: released as experimental and unaudited, its bar Google's
   Wycheproof test vectors, differential tests against rustls, fuzzing
   of the record and handshake parsers, and constant-time checks on the
   critical routines, until an outside audit (firms such as Trail of
   Bits, NCC Group, or Cure53; funds for open-source audits such as
   OSTIF, the Sovereign Tech Fund, and OpenSSF Alpha-Omega).
5. **Native grants at compile time**, as `--sandbox`.

## 7. What stays open after this batch

- Keep-alive and streamed request bodies, if measurements show the need.
- WebSockets, UDP, and DNS: later batches, each with its own short plan.
- A server that recovers from a trap in one request without a fresh
  instance would need the language to recover from failures, which AIPL
  deliberately does not; the fresh instance (B) is the answer to that.
