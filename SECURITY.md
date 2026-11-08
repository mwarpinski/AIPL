# Security policy

AIPL is a language that claims memory safety outside `(unsafe ...)` and ships tools that write and run executables. Reports
that show a claim is false are welcome and are treated as the most important kind of bug.

## Reporting a vulnerability

**Do not open a public issue.** Use GitHub's private report: the repository's **Security** tab, **Report a vulnerability**
(<https://github.com/mwarpinski/AIPL/security/advisories/new>). Only the maintainers see it.

Please include the smallest AIPL program that shows the problem, the command that runs it (`aipl run`, `aipl compile
--exe`, ...), what you expected, what happened, and the output of `aipl --version`.

You will get an answer saying the report arrived within **7 days**, and a first assessment (is it a vulnerability, and how
bad) within **14 days**. A fix is released as soon as it is verified; the report is made public, with credit if you want
it, when the fix is out or after **90 days**, whichever comes first.

## What counts

- **A memory-safety hole**: a program with no `unsafe` and no `ptr.cast` that reads or writes memory it should not, in the
  interpreter or in compiled code (`aipl run`, `--exe`, WebAssembly, either backend). The checks the language promises are
  in [the README](README.md#what-works-today) ("Memory safety") and AIPL_SPEC.md.
- **A sandbox escape**: a program gets more than its grants allow (`--sandbox`, `--allow-connect`, `--allow-listen`, file
  and process grants), or a path reaches outside the directory it was confined to.
- **A miscompilation that breaks a safety check**: the compiler drops or weakens a check the language defines.
- **The supply chain**: `install.sh`, `install.ps1`, or a release whose checksum or Sigstore signature does not verify, or
  an installer that runs something it did not verify.

## What does not

- Data races in threaded code: AIPL does not check them, and threads need `unsafe` (see [docs/LIMITS.md](docs/LIMITS.md)).
- Anything inside `(unsafe ...)`, or through a pointer made by `ptr.cast`: those are unchecked by design.
- The fast arena catching a stale pointer only once its memory is reused: documented.
- A program that runs out of memory or time, or stops with a clean trap: a trap is the safety check working.
- macOS and Windows builds are a preview (docs/RELEASING.md); reports are welcome, but they are not held to the Linux bar yet.

## Supported versions

Only the latest release, and `main`.

## Verifying a download

Each release lists its files in `SHA256SUMS`, signed with Sigstore; the commands are in
[docs/RELEASING.md](docs/RELEASING.md#verifying-a-download).
