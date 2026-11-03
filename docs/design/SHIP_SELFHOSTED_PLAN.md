# Shipping the self-hosted `aipl` in the Linux tarballs

**Status: proposed 2026-11-03 (the owner: "ship the self-hosted native aipl in the Linux tarballs ... first close its gaps").
L1 to L4 are built (the first release to use them is the next one); L5 is partly built.** One step at a time, each ending in something a person can run and a test that holds
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
| **Host names and HTTPS** (`net.connect_host`, `tls_connect`, so `http.fetch "https://..."`) | a native executable got -3 from both imports and std/net said to use `--target wasm` or a proxy | **L3, built**, below |
| Executables off Linux | the launcher bundle | unchanged: not this plan |

## L3: names and TLS through a Rust helper, spoken to over pipes (built 2026-11-03)

A native executable is a static program of machine code the compiler wrote: it cannot link rustls or call the system's
resolver. NET_PLAN.md section 7 named the way: "a Rust process started with `proc.spawn` and spoken to over a socket".
Pipes do the same work and need nothing new in the code generators (there is no `socketpair` op; `std/proc` has pipes in
every backend).

- **`aipl-net`** (src/bin/aipl_net.rs, a Rust program in this crate, built on src/net.rs: rustls with the ring provider,
  webpki-roots and `AIPL_CA_FILE`, the system resolver): `aipl-net connect HOST PORT [--tls]`. It connects, does the
  handshake when asked, writes ONE status byte (0 ok, 1 refused, 2 not granted, 4 TLS failed, 5 no such host, 6 bad name;
  anything but 0 is the whole output), then relays: its standard input to the host, the host to its standard output, until
  either side ends. One thread, polling both: a read blocked on a TLS connection holds the lock a write needs.
- **std/net** (AIPL, no compiler change): `connect_host` and `connect_tls` call the import first. A native executable's
  answers -3, and they start the helper with `proc.spawn_with` (pipes in and out, its error inherited), read the status
  byte, and return a `Socket` whose `h` is -1 and whose `helper` is the child. `recv`, `send`, `poll` (so `readable`,
  `recv_within`, `accept_within`'s siblings) and `close` act on the child's pipes for such a socket. The helper is
  `$AIPL_NET`, else the first `aipl-net` on `PATH` (`proc.find`). The Rust hosts never see -3 and are unchanged.
- **Grants, held by the helper (the owner chose "B", per-host grants enforced for native programs; this is how they are).**
  The first idea, a check in the native runtime's machine code that answers -2 or -3, leaves a hole: the program must be
  allowed to start the helper, and a program allowed to start it can start it with any host. So the helper enforces.
  `--allow-connect example.com:443` is accepted for native executables; the backend (aipl_src/native/native.aipl and
  native_arm64.aipl, through nettrailer.aipl) writes all the connect grants after the executable's last byte as a
  trailer, `HOST:PORT` lines, their length as four little-endian bytes, `AIPLNET1`, only when one is a name; the loader
  ignores it, and an executable with no name grant is the bytes it was. The helper reads the trailer of its parent's
  executable (`/proc/PPID/exe`) and refuses any host it does not name; a parent with no trailer (a shell, or an
  executable with address grants only) has granted nothing. A program that starts `aipl-net` itself, not through std/net,
  therefore reaches exactly what it was granted (tests/test_net.rs,
  `aipl_net_started_by_a_program_reaches_only_the_hosts_that_program_was_granted`).
- **Permission to start it.** A program starts only what it was granted (`--allow-run`). When a name grant is present, the
  compiler also grants the helper at the paths std/net will find it: `$AIPL_NET`, and each absolute folder of `$PATH` with
  an `aipl-net` in it (aipl_src/native/nethelper.aipl for the self-hosted command, src/native.rs for the Rust one: the two
  must give the same bytes). With none found the compile fails and says so; a relative `PATH` entry is not granted. An
  executable carried to a machine where the helper is elsewhere gets `Error.no_helper`; `--allow-run` and `AIPL_NET` are the way.
- **Linux only**, as the helper reads /proc and native executables exist only there. Every program that imports std/net now
  imports `std/proc` too, so a native executable of one carries the process routines.
- **Not done:** a relative `PATH` entry; Windows and macOS (the Rust launcher is their host and provides the imports
  itself); `compile --exe` for another machine than the one that will run it, beyond `AIPL_NET`.

Why not the alternatives: a TLS written in AIPL is not planned (NET_PLAN.md decision 4); an import resolved by the native
runtime's own machine code would need `socketpair`, `fork`, and `dup2` emitted by two code generators for one feature; a
loopback proxy daemon adds a lifecycle to manage; and a check in the runtime alone is bypassable, above.

## Steps

- **L1: find the library when started by name. Built 2026-11-03.** `aipl_src/stdpath.aipl`; the test is
  `the_aipl_command_started_by_name_finds_its_library_beside_bin` in tests/test_bootstrap.rs (the installed layout, PATH, a
  working directory with no checkout, and the error when the library is missing).
- **L2: the dropped modes say so. Built 2026-11-03.** Messages in aipl.aipl, testcmd.aipl, evalcmd.aipl say the mode is not
  part of this aipl and no longer send the user to "the Rust aipl"; the test is
  `the_aipl_command_refuses_the_modes_only_the_rust_command_has` in tests/test_bootstrap.rs.
- **L3: `aipl-net` and std/net's second path. Built 2026-11-03.** tests/test_net.rs now runs every fetch, the TLS cases, and the
  refusals in the native backend too (against a local HTTP server and a local TLS server made with rcgen), and adds:
  the helper missing at compile time, the trailer's bytes, a program that starts the helper itself, the helper run by hand.
- **L4: the tarball. Built 2026-11-03.** `tools/package_release.sh` packs, for Linux, `bin/aipl` (the self-hosted one,
  `sh bootstrap/build.sh`, which needs no Rust), `bin/aipl-net`, and `share/aipl/std/` (found beside `bin/` by name or
  by path: L1); `aipl-run` is no longer in a Linux tarball. The tarball is 3.0 MB (the Rust one was 15.8 MB).
  `install.sh` takes whatever `bin/` holds and checks the install with `aipl run`. release.yml's Linux legs run
  `bootstrap/build.sh` (the aarch64 seed on the arm runner) and check that `aipl_src/aipl.aipl`'s `--version` names the tag's
  version (it is a constant there: bump it with Cargo.toml). Checked here by packing, installing from a `file://` release
  into an empty home, and compiling, running, `--exe`, `doc`, and a fetch by host name with the installed result. macOS and
  Windows keep the Rust `aipl` and `aipl-run`. `aipl --version` of the self-hosted command is `aipl 0.1.0`, with no
  commit, where the Rust one adds it.
- **L5: parity in CI. Partly built.** `the_aipl_command_matches_the_rust_command` compares compile output, errors, and
  `aipl doc` for every example, benchmark, and test program, and
  `both_commands_build_the_same_executable_for_a_host_name_grant` the new trailer; CI's `install` job installs the Linux
  tarball (self-hosted `aipl` and the helper) and runs it by name from outside a checkout. Not yet: running tests/test_net.rs
  with the self-hosted `aipl` as the compiler (it uses the Rust one; both call the same native backend).

## Decisions for the owner

1. **Dropping the VM-only modes (L2).** Recommended: drop, as above; they are the referee's tools.
2. **A helper process for names and TLS (L3)**, one more file in the tarball, in place of a native executable carrying TLS.
   Recommended: yes. The alternative is the status quo, a proxy in front.
3. **How name grants are held (B, A, or C above).** Recommended: B.
4. **Linux only, for now (L4).** macOS and Windows keep the Rust `aipl` and `aipl-run` until their native backends exist.
