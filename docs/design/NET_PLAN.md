# Networking: serving and fetching (stdlib batch L7)

**Status: approved 2026-10-19 (decisions in section 6).** Serving is
built (steps 1-4); fetching, TLS, and the measurements are not (steps
5-7). STDLIB_PLAN.md asked for this document before any code: "an `aipl`
host interface (versioned imports beside WASI preview 1: listening and
connecting TCP sockets, permissions granted on the command line), in the
VM, natively as Linux system calls, and in aipl-run; then `http` serving
and fetching over it".

## At a glance

| Step | What | Status |
|---|---|---|
| 1 | `(no_leak)` on function types (MEMORY_PLAN.md decision 9) | **Done** 2026-10-20 |
| 2 | Sockets: `aipl_net_v1` listen/accept/recv/send/close, `--listen`, std/ip, std/net | **Done** 2026-10-20 |
| 3 | std/serve and the long-running server (A); `--max-memory` | **Done** 2026-10-20 |
| 4 | A fresh instance per request (B): `serve.once`, `aipl serve --fresh`, `aipl compile --exe --serve` | **Done** 2026-10-20 |
| 5 | Fetching: `connect`, `--allow-connect`, `http.fetch` | **Next** |
| 6 | TLS from the host (rustls): `tls_connect`, `tls_accept`, `https://` | Not started |
| 7 | Measure requests a second per backend and mode (docs/BENCHMARKS.md) | Not started |

Also outstanding (small, found while building steps 3-4):

- A fresh server's 500 for an answer over 64 MiB writes nothing to the
  server's log; a trap's 500 logs the trap, this one should log a reason.
- `aipl eval` panics printing its own status lines when standard output
  is a full disk (the program's exit status is still right).
- "Out of memory" said as such when a program reaches `--max-memory`
  (docs/LAUNCH_CHECKLIST.md; today it is the out-of-bounds trap).

## 1. Why a long-running server is safe now

The first draft of this plan argued that a long-running server was
unsafe in AIPL, because of memory, and proposed only a fresh instance per
request. MEMORY_PLAN.md (complete) removed the reasons:

| The draft said | Now (MEMORY_PLAN.md) |
|---|---|
| every view is made with `new` and never freed: 3.7 MB a second at 10,000 requests a second | a view is a value, three slots, nothing allocated (M1) |
| `new`, `arr.new`, and run-time `str` are never freed | `new` and `arr.new` name their allocator (F); what is never freed says so, `alloc.forever` |
| nothing shows which calls keep memory | `(no_leak)` proves a function, and everything it calls, gives back what it takes (M3) |
| results and unions make hidden cells | they are values (M4) |
| std leaks per call (io.print_int, float formatting, json numbers) | fixed, measured flat (M2) |
| a pointer kept across requests reads whatever came next | every pointer, array, and view is generation-checked: it stops the program with "Use after free" |

So a server runs each request on an arena, resets it after, marks its
handler `(no_leak)`, and the checker proves memory stays flat; a pointer
kept by mistake past the reset traps on its next use instead of reading
another request's data.

What did not change: **a trap ends the program.** AIPL has no
exceptions and no recovery from a failed contract, bounds check, or use
after free, by design (the program's state may be wrong). In a server
that runs for days, one bad request then takes the server down. That is
the real difference between the two ways to serve.

## 2. Two ways to serve, one handler (built: steps 3 and 4)

A handler is written once, the same for both:

```lisp
(fn handle [a:alloc.Allocator req:(ptr http.Message) out:(ptr buf.Buf)] -> void (no_leak)
  (call http.start_response out 200)
  (call http.add_header out "Content-Type" "text/plain; charset=utf-8")
  (call http.end_head out (get req http.Message.target)))
```

`a` is the request's arena, `req` the parsed request, `out` the
response's buffer; `std/serve` reads and writes the bytes, so the handler
does no I/O.

**A. A long-running server**: `main` calls `serve.run`. The program owns
the listening socket and loops: accept, read a request, run the handler
on an arena, write the response, reset the arena. State lives in memory
between requests (a cache, a counter, a session map) through
`serve.run_with` and a context.

- Fast: no start-up per request.
- Memory flat, proved by `(no_leak)` and checked by the generations.
- **A trap in one request ends the server.** It needs a supervisor that
  restarts it (systemd, a container's restart policy), and loses what it
  held in memory.

**B. A fresh instance per request**: `main` returns `serve.once`, which
answers one request from standard input on standard output. The host
owns the socket and runs the program from the start for each connection
(as CGI does, and WebAssembly's serverless hosts: Fastly Compute,
Fermyon Spin).

- **A trap ends one request** (a 500, and the trap in the server's log);
  the next starts clean.
- Nothing can outlive a request, `(no_leak)` or not.
- No state in memory between requests (a file or a database instead),
  and a start-up per request: under aipl-run a module compiled once and
  instantiated per connection; natively a fork of a copy made once
  memory is ready (Linux copies memory only as it is written).

```
aipl eval --listen 127.0.0.1:8080 shop.aipl                        (A, in the VM)
aipl compile --exe --listen 127.0.0.1:8080 shop.aipl -o shop       (A, an executable)
aipl serve --fresh shop.aipl --listen 127.0.0.1:8080               (B, under aipl-run)
aipl compile --exe --serve 127.0.0.1:8080 shop.aipl -o shop        (B, an executable)
```

A B program is tried without a network by piping a saved request
through it: `aipl eval shop.aipl < request.txt`, the same bytes from
every backend.

## 3. The host interface: `aipl_net_v1`

WASI preview 1 has no way to listen or connect. A versioned import
module beside `wasi_snapshot_preview1`, implemented in each backend (the
VM and aipl-run in src/net.rs, native executables as Linux system calls
written in AIPL, aipl_src/native/sockets.aipl):

| Function | Meaning | Status |
|---|---|---|
| `listen(addr, len, port) -> h` | a listening TCP socket; -1 if the system refuses it, -2 if it was not granted | built (step 2) |
| `accept(h) -> h` | the next connection (blocking), or -1 | built (step 2) |
| `recv(h, buf, max) -> n`, `send(h, buf, len) -> n` | blocking reads and writes: bytes moved, 0 at the end, -1 on an error | built (step 2) |
| `close(h) -> i32` | closes a socket: 0, or -1 | built (step 2) |
| `connect(addr, len, port) -> h` | a connection; -1, or -2 if not granted | **not built** (step 5) |
| `tls_connect`, `tls_accept` | TLS over a connection (section 4) | **not built** (step 6) |

An address is given as its bytes, 4 for IPv4 or 16 for IPv6, in network
order; std/ip parses the text (`127.0.0.1`, `::1`) in AIPL, so no backend
parses addresses. A handle is a socket's, numbered apart from WASI's file
descriptors: aipl-run hosts WASI through wasmtime-wasi, whose descriptor
table a host function cannot add a socket to, so `fs.close` could not
close one there, and `fs.close` given a socket's number would close
whatever file has that number. (Natively a handle is the Linux
descriptor itself; what a raw handle's number means is each backend's,
and only std/net's `Socket` is portable.) So sockets close through
`close` here, and std/net keeps the handle in its own type,
`net.Socket`, whose field is private: a program cannot hand a socket to
`fs` or a file to `net`. In AIPL these are the operations `net.listen`,
`net.accept`, `net.recv`, `net.send`, and `net.close`, usable only inside
`unsafe` (they take raw addresses, as `fs.read` does); std/net is the
safe interface, and turns -2 into an error naming the grant to add ("not
granted: run the program with --listen ADDR:PORT").

Granted on the command line, nothing by default: `--listen ADDR:PORT`
(built) and `--allow-connect HOST:PORT` (repeatable; `*` for any port;
**not built**, step 5). A native executable or launcher bundle gets its
grants when it is compiled, as `--sandbox` does, so a binary says what it
may reach and cannot be talked into more. The VM and aipl-run take them
at start.

`std/net` wraps sockets as `stream.Reader` and `stream.Writer`, so
std/http reads and writes over them unchanged. A stream's callbacks are
calls through function references, whose types carry the `(no_leak)`
promise (step 1), so a `(no_leak)` handler may use streams.

## 4. Fetching (not built: steps 5 and 6)

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
front), as the backend support table has other explicit refusals. A
TLS written in AIPL is not planned (decision 4).

- **Addresses, not names, at first.** Natively there is no libc, so no
  system resolver; a DNS client in AIPL (UDP, /etc/resolv.conf,
  /etc/hosts) is its own small batch. `localhost` and /etc/hosts entries
  are cheap everywhere.

## 5. Order of work

Each step is tested in every backend and both toolchains, documented in
AIPL_SPEC.md and the prompt guide, and merged before the next.

### Done

1. **Decision 9** (MEMORY_PLAN.md): `(no_leak)` on function types, so
   streams can be used in a handler. Done 2026-10-20.
2. **`aipl_net_v1`** listen, accept, recv, send, and close, in the three
   backends, with the grants; std/ip and std/net (`Socket`) and streams
   over it. Done 2026-10-20: src/net.rs for the VM and aipl-run,
   aipl_src/native/sockets.aipl natively; `--listen` on eval, run,
   aipl-run, and compile --exe (fixed in the executable, and in the
   launcher bundle's trailer, AIPLEXE2). Tests (tests/test_net.rs): a
   loopback exchange in each backend, IPv4 and IPv6, the same bytes
   everywhere; a listen without a grant refused; both toolchains' bytes
   alike.
3. **`std/serve`** and the long-running server (A) in every backend, and
   `--max-memory`, so a runaway request fails cleanly. Done 2026-10-20.
   Tests (tests/test_serve.rs, tests/test_memory.rs): real requests over
   loopback, answers byte for byte alike across backends; refusals (too
   big, malformed) the same status everywhere; a million requests with
   memory flat, natively and under aipl-run (100,000 in CI, a million
   with AIPL_MILLION=1; 20,000 in the VM); a view kept past its request
   trapping in every backend; `--max-memory` stopping at the same place
   everywhere.
4. **The fresh instance per request (B).** Done 2026-10-20: `serve.once`;
   `aipl serve --fresh` (aipl-run `--serve-fresh`: the module compiled
   once, an instance per connection, the connection its standard input,
   its output held until it exits); `aipl compile --exe --serve
   ADDR:PORT`, natively a forking host (aipl_src/native/fresh.aipl), as a
   bundle aipl-run's `--serve-fresh`. Tests (tests/test_serve.rs): saved
   requests piped through `serve.once` in the VM, under aipl-run, and
   natively, the same bytes from each; each of the three hosts serving
   five connections with a crash and a 64 MiB answer among them (each a
   500, the next request served); an address in use refused with the same
   message by each; a threaded program refused when compiled. As built,
   `aipl serve` without `--fresh` is refused with the way to a
   long-running server (`aipl eval --listen`) rather than being a second
   way to run one, and `--serve` takes its address when compiled, as
   `--listen` grants are.

### Not started

5. **`connect`, `--allow-connect`, and `http.fetch`**, tested against
   step 3's server in each backend. **Next.**
6. **TLS from the host:** `tls_connect` and `tls_accept`, `https://` in
   `http.fetch`, a server with a certificate; native executables refuse
   it at compile time with the way out. Tests: a fetch from step 3's
   server over TLS with a test certificate, in the VM and under aipl-run;
   the refusal's message.
7. **Measure** requests a second per backend and mode on a quiet machine,
   in docs/BENCHMARKS.md.

## 6. Decisions (the owner's, 2026-10-19)

1. **Both ways to serve, long-running first:** a program uses whichever
   fits; the long-running server (A) is built first, since the fresh
   instance (B) reuses its parts (the handler, std/serve, the sockets)
   and adds only how the program is started. *Built: A in step 3, B in
   step 4.*
2. **Concurrency: one request at a time, then a pool of worker threads**,
   each with its own arena ("walk, then run"). An event loop, if ever,
   later. *One at a time is built; the worker pool is not started.*
3. **Fetching over plain HTTP to addresses first**; DNS as a later small
   batch. *Not started (step 5).*
4. **TLS from the host (rustls) before launch**, in the VM, aipl-run, and
   the launcher-bundled executables; native executables refuse it at
   compile time with the way out (`--target wasm`, or a TLS proxy in
   front, as most servers are deployed anyway). *Not started (step 6).*
   **A TLS written in AIPL is not planned** (revised 2026-10-20): it
   would add only HTTPS inside a native executable with no proxy, and
   hand-written cryptography is not fit for real traffic without an
   outside audit, which the project will not pay for. Revisit only if
   AIPL gains the kind of use that grant-funded audits (OSTIF, the
   Sovereign Tech Fund, OpenSSF Alpha-Omega) go to.
5. **Native grants at compile time**, as `--sandbox`. *Built (steps 2
   and 4: `--listen` and `--serve`).*

## 7. Later, after this batch

- The worker-thread pool (decision 2).
- Keep-alive and streamed request bodies, if measurements show the need.
- WebSockets, UDP, and DNS: later batches, each with its own short plan.
- Not planned: a TLS written in AIPL (decision 4).
- A server that recovers from a trap in one request without a fresh
  instance would need the language to recover from failures, which AIPL
  deliberately does not; the fresh instance (B) is the answer to that.
