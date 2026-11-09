# AIPL launch notes

The text for the release page and the announcement; the release notes the workflow generates
(`tools/release_notes.py`: platforms, install, verify, commits) go below it. Written for someone who has never heard of
the project.

## What AIPL is

AIPL (AI Programming Language) is a small, statically typed systems language designed to be written by AI coding agents
and read by people. It is prefix S-expressions with every type written out and nothing converted implicitly; contracts
(`req`, `ens`) run on every call, array indexes are bounds-checked, integer arithmetic traps on overflow, and every
compile error and every crash says where, in a form an agent can act on. It compiles to WebAssembly and to native Linux
executables (x86-64 and AArch64), a hello-world executable is about 85 KB, and its compiler is written in AIPL and
compiles itself. The [README](../README.md) has an example and the first questions are in the [FAQ](FAQ.md).

```sh
curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/mwarpinski/AIPL/main/install.sh | sh
```

## What is finished, and what is not

Said before anyone has to ask:

- **Linux is the product.** Linux x86-64 and AArch64 install the self-hosted `aipl`, built from a checked-in seed with
  no Rust anywhere in the build. Anything you can do with a checkout, you can do from `git clone`, `sh bootstrap/build.sh`.
- **macOS (Apple silicon) and Windows (x86-64) are a preview.** They install the Rust `aipl` and `aipl-run`, and
  `compile --exe` there makes a 15 to 20 MB bundle that carries the launcher, not a small native executable. Mach-O and
  PE backends are the next big piece of work ([ROADMAP](../ROADMAP.md) item 4). The macOS binary is not notarised and the
  Windows one is not code-signed, so both operating systems will warn. CI builds and tests Windows on every push; an
  Apple Silicon Mac is checked by hand.
- **The question the project exists for is not answered.** Whether agents write correct programs more often in AIPL
  than in other languages has not been measured. The harness (`tools/harness/`) exists; the measurement, with tasks,
  prompts, transcripts and the numbers, including where AIPL loses, comes after launch.
- **The language is not stable.** Modules can declare the language version they were written for, but there is no promise yet that a program
  written today builds next year ([FAQ](FAQ.md), [LANGUAGE_VERSIONING](LANGUAGE_VERSIONING.md)). There are no packages and
  no registry, no language server (editor grammars exist, unpublished), and no TLS written in AIPL: HTTPS goes through a
  small Rust helper, `aipl-net`.
- **Known rough edges:** `aipl run` on a `.wasm` that traps prints one extra line on standard error; a program on Windows
  sees only the drive it is started on; Linux AArch64 is built and tested on GitHub's arm64 machines but the installer
  has not been run on a physical one; [LIMITS.md](LIMITS.md) lists the limits of the language and of the compilers.
- **It is not fast like C.** Native code takes 1.2 to 3.3 times as long as C on six of the eight benchmarks and about
  9 times on the one that is all hashing; [BENCHMARKS.md](BENCHMARKS.md) has the table, the machine and the cost of the
  overflow checks.

## How it was built

The language is designed by Matt Warpinski and implemented with AI coding agents (Claude); the commit messages say so.
Because of that, the project leans hard on checking rather than on trust:

- Two independent compilers, a Rust one (the *referee*) and the self-hosted one, must accept and refuse the same
  programs, print the same diagnostics, and behave the same on every program in a conformance suite.
- An interpreter and two backends (WebAssembly and native) must agree on every program; fuzzers generate and mutate
  programs on every push; the optimizer is checked against itself.
- The compiler rebuilds itself from the seed to a fixed point on every CI run, and each seed's hash is recorded and
  checked ([RELEASING](RELEASING.md)).
- The documents are checked against the code: every program in them verifies, every quoted error message is in the
  sources, every command exists (`tools/docs_check.py`).

The Rust referee stays during the alpha and beta and is planned to go ([ROADMAP](../ROADMAP.md), "removing Rust
entirely"). The release itself is built and signed by a workflow in this repository (Sigstore keyless signing), and
[SECURITY.md](../SECURITY.md) says how to report a problem.

## What is next

Native executables on macOS and Windows, the A/B measurement of the thesis, a language server, and then a stability
promise and a package manager, in that order of how soon a newcomer meets them ([ROADMAP](../ROADMAP.md)).
