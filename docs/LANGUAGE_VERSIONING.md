# Language versions

AIPL has a language version, a whole number apart from the version of the `aipl` program. This page is the promise
made about it. The version at launch is **1**.

## What a program says

A module may declare the language it is written for, anywhere among its items:

```
(module hello
  (language 1)
  (import io)
  (fn main [] -> i32 (call io.println "hello") 0))
```

- Without `(language N)` a program means the version the `aipl` that reads it implements.
- With it, an `aipl` that implements a version below `N` refuses the program and says why:
  `this program is written for language version 2; this aipl implements version 1: use a newer aipl`.
- `N` is a whole number from 1. Anything else is an error.

Every module `aipl compile` writes carries the version its compiler implements in a custom section named
`aipl.language` (the digits of the number, as text), so a `.wasm` file says which language it was built from.

## What is promised

- **Within a version, a program that compiles keeps compiling, and keeps meaning the same.** Additions (a new
  function in the standard library, a new form) do not change a version. A bug fix that changes what a program
  does is written down in the release notes.
- **A change that would break programs makes a new version.** The old version stays available: an `aipl` implements
  the current version and the one before it, so a program declared `(language 1)` builds under an `aipl` that
  implements 1 and 2.
- **A removal is announced a release ahead.** What version `N+1` will remove or change is marked deprecated in
  version `N`: `aipl verify` and `aipl lint` say so, and name the replacement, for at least one release before the
  new version takes it away.
- **The standard library follows the same rule.** A function that is removed or whose type changes is deprecated
  for a release first. A function kept for old programs may be hidden from `aipl doc` but still works.

## Before 1.0

Until the first release that calls itself 1.0, the language may still change without a new version, and each change
is in the release notes. The checklist for 1.0 (LAUNCH_CHECKLIST.md, section 1) is what settles the language;
the changes made there (integer `+ - *` trapping on overflow, `wrap.*`, `try` with a conversion, `io.Error` carrying
a path, searches returning an `Option`) are the first ones every later version is measured from.
