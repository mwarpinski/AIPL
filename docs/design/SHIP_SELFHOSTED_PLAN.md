# Shipping the self-hosted `aipl` in the Linux tarballs

**Status: proposed 2026-11-03 (the owner: "ship the self-hosted native aipl in the Linux tarballs ... first close its gaps").
L1 and L2 are built; L3 to L5 are not started.** One step at a time, each ending in something a person can run and a test that holds
it, as aipl_standalone.md asks.

## The goal

On Linux (x86-64 and AArch64) the `aipl` a user installs is the self-hosted native command (aipl_src/aipl.aipl, built from the
checked-in seed with no Rust), not the Rust one. A user's path then holds no Rust, except one small optional helper for the two
things AIPL does not do itself: host names and TLS (NET_PLAN.md decision 4; "Rust components" in its section 7). macOS and
Windows keep the Rust build until they have native backends (aipl_standalone.md, S3). The Rust `aipl` stays what it has been:
the referee in the test suite and CI, and the build for those two platforms.

## Where it stands (checked 2026-11-03)

The self-hosted command is 4.8 MB, builds in about 11 seconds from the seed (`sh bootstrap/build.sh`, a fixed point), and
does `compile` (wasm and `--exe`), `verify`, `run` (a source file or a `.wasm`, natively), `test`, `watch`, `serve --fresh`,
`eval`, `doc`, and `--version`. What it cannot yet do, and what the tarball needs:

| Gap | What happens today | Decision |
|---|---|---|
| **Finding the library** | Started by bare name (`aipl`, found through PATH) it looked for `aipl_src/std/` below the working directory, so it worked only inside a checkout. It read `share/aipl/std/` beside `bin/` only when argv[0] had a `/`. | **L1, built:** with no `/` it uses the PATH entry that holds a file by that name, as the shell did. The tarball lays the library out at `share/aipl/std/`. |
| `eval -f FUNC`, `eval --wasm`; `test -f`, `--vm`, `--all-backends`; `compile --self`; `compile --target wasm` | refused, with a message that sends the user to "the Rust aipl", which will not be in the tarball | **L2: drop.** All are ways to reach the VM or the second compiler, i.e. the referee's tools. They stay in the Rust CLI the repository's tests and CI use. The messages say the mode is not part of this aipl. |
| `test FILE` where FILE has no `(test ...)` (its entry function `run_all` returns a failure count) | refused | **L2: drop.** Only `aipl_src/test_suite.aipl` (the library's own aggregate) uses the convention; CI runs that with the Rust CLI. |
| **Host names and HTTPS** (`net.connect_host`, `tls_connect`, so `http.fetch "https://..."`) | a native executable gets -3 from both imports and std/net says to use `--target wasm` or a proxy | **L3**, below |
| Executables off Linux | the launcher bundle | unchanged: not this plan |

## L3: names and TLS through a Rust helper, spoken to over pipes

A native executable is a static program of machine code the compiler wrote: it cannot link rustls or call the system's
resolver. NET_PLAN.md section 7 already names the way: "a Rust process started with `proc.spawn` and spoken to over a
socket". Pipes do the same work and need nothing new in the code generators (there is no `socketpair` op; `std/proc` has
pipes, in every backend).

- **`aipl-net`**, a small Rust program in this crate (rustls with the ring provider and webpki-roots, the system resolver, as
  src/net.rs does now): `aipl-net connect HOST PORT [--tls]`. It resolves the name, connects, does the handshake when asked,
  and relays the standard input to the connection and the connection to the standard output until either side closes. Its
  exit status says why it could not connect (no such host, refused, handshake failed, bad name) in the codes `Error` already
  has. It ships beside `aipl` in `bin/`; `AIPL_NET` names another.
- **std/net** (AIPL, no compiler change): `connect_host` and `connect_tls` ask the import first, as they do; on -3 they start
  `aipl-net` with `proc.spawn` and hold the child as the connection. A `Socket` becomes a handle or a child, and `send`,
  `recv`, `close`, and the limited waits (`recv_within`, `poll`) act on whichever it is. A program that uses a name needs
  `--allow-run` for `aipl-net`; the connect grant (`--allow-connect HOST:PORT`) is still checked by name before the helper
  starts, so a program reaches no host it was not granted. Without the helper beside `aipl`, or on `PATH`, the error says so
  and how to get it.
- **Not changed:** the Rust hosts (`aipl eval`, aipl-run, bundles) keep providing the imports directly. A native executable
  built with `compile --exe` that uses names or TLS carries the need for `aipl-net` on the machine it runs on, and says so
  when it is missing.

Why not the alternatives: a TLS written in AIPL is not planned (NET_PLAN.md decision 4); an import resolved by the native
runtime's own machine code would need `socketpair`, `fork`, and `dup2` emitted by two code generators (x86-64, AArch64) for one
feature; a loopback proxy daemon adds a lifecycle to manage.

**The grant problem (found 2026-11-03, before any L3 code).** Today a native executable answers `connect_host` with -3 without
asking any grant, and `compile --exe --allow-connect example.com:443` is refused ("a native executable has no name resolver").
A helper the program starts would reach any host the program names unless something trusted holds the grant. Three ways:

- **B (recommended): the native runtime holds name grants.** `--allow-connect HOST:PORT` is accepted for names, stored in the
  executable like the address grants, and checked by the runtime's `connect_host` (a byte compare of the name and port, in
  the x86-64 and AArch64 routines, sockets.aipl and sockets_arm64.aipl). It answers -2 if no grant names the host, and -3 (use
  the helper) if one does. The program cannot reach a host it was not granted, as in every other backend. Cost: machine code
  in two emitters.
- **A: the helper holds them.** `aipl-net` reads the grants from its parent's executable (`/proc/PPID/exe`, the same trailer
  aipl-run reads) and refuses a host they do not name. No code generator change, but it trusts the helper to find the right
  parent, and Linux's /proc.
- **C: `--allow-run aipl-net` is the grant.** A program allowed to start the helper may reach any host. Simplest; there is no
  per-host grant for native programs.

## Steps

- **L1: find the library when started by name. Built 2026-11-03.** `aipl_src/stdpath.aipl`; the test is
  `the_aipl_command_started_by_name_finds_its_library_beside_bin` in tests/test_bootstrap.rs (the installed layout, PATH, a
  working directory with no checkout, and the error when the library is missing).
- **L2: the dropped modes say so. Built 2026-11-03.** Messages in aipl.aipl, testcmd.aipl, evalcmd.aipl say the mode is not
  part of this aipl and no longer send the user to "the Rust aipl"; the test is
  `the_aipl_command_refuses_the_modes_only_the_rust_command_has` in tests/test_bootstrap.rs.
- **L3: `aipl-net` and std/net's second path.** Tests: the same fetches as tests/test_net.rs against a local HTTP server and
  a local TLS server (rcgen, as that file does) through the self-hosted `aipl`, compared with the Rust hosts' output; the
  grant refusals; the helper missing.
- **L4: the tarball.** release.yml's two Linux legs run `sh bootstrap/build.sh` (the seed for the platform) instead of
  `cargo build` for `aipl`, and still `cargo build --release -p`-style builds `aipl-net`; `tools/package_release.sh` packs
  `bin/aipl` (self-hosted), `bin/aipl-net`, `share/aipl/std/`, and the documents; the smoke test installs it with install.sh
  in an empty home and runs a program, `aipl doc`, and an HTTPS fetch. `aipl-run` is no longer in the Linux tarballs.
  `aipl --version` prints the same line.
- **L5: parity in CI.** The existing `the_aipl_command_matches_the_rust_command` already compares compile output, errors, and
  `aipl doc` for every example, benchmark, and test program; it runs against the packaged binary too, so what ships is what
  was compared.

## Decisions for the owner

1. **Dropping the VM-only modes (L2).** Recommended: drop, as above; they are the referee's tools.
2. **A helper process for names and TLS (L3)**, one more file in the tarball, in place of a native executable carrying TLS.
   Recommended: yes. The alternative is the status quo, a proxy in front.
3. **How name grants are held (B, A, or C above).** Recommended: B.
4. **Linux only, for now (L4).** macOS and Windows keep the Rust `aipl` and `aipl-run` until their native backends exist.
