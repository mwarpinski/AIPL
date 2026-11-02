# Standalone Toolchain: Decoupling AIPL from Cargo

While AIPL possesses advanced compiler internals—including a self-hosted compiler ([`aipl_src/aipl.aipl`](file:///home/matt/Projects/Repos/AIPL/aipl_src/aipl.aipl)) compiling AIPL $\to$ WebAssembly $\to$ native Linux x86-64 ELF executables with zero libc dependencies—its CLI and module resolution conventions currently rely on the repository working directory.

---

## Current Repository Coupling

Four specific constraints currently tie user workflows to the repository root:

1. **Standard Library Path Lookup:**
   In [`aipl_src/aipl.aipl:70-71`](file:///home/matt/Projects/Repos/AIPL/aipl_src/aipl.aipl#L70-L71) and [`codegen_opt.aipl:2357-2358`](file:///home/matt/Projects/Repos/AIPL/aipl_src/codegen_opt.aipl#L2357-L2358):
   ```lisp
   (let std_dir:str.Bytes (call os.env "AIPL_STD"))
   (if (lt (get std_dir str.Bytes.len) 0) (set! std_dir (call str.from_str "aipl_src/std/")) (block))
   ```
   If executed outside the repository root without `$AIPL_STD` set, import resolution defaults to `./aipl_src/std/` in the current working directory. Additionally, the Rust CLI ([`src/resolver.rs:880`](file:///home/matt/Projects/Repos/AIPL/src/resolver.rs#L880)) hardcodes `env!("CARGO_MANIFEST_DIR")/aipl_src/std` at compile time.

2. **Self-Hosted CLI Subcommand Surface:**
   [`aipl_src/aipl.aipl:24-27`](file:///home/matt/Projects/Repos/AIPL/aipl_src/aipl.aipl#L24-L27) currently handles `compile` and `--version`. Although the self-hosted type-checker ([`front.aipl`](file:///home/matt/Projects/Repos/AIPL/aipl_src/front.aipl)) and native backend are compiled into the binary, subcommands like `verify` and `test` are not yet exposed in the top-level driver.

3. **Executable File Permissions (`chmod +x`):**
   When `aipl compile --exe` writes an output ELF binary, WASI file creation defaults to mode `0644`. On Unix systems, executing the resulting binary requires manually setting executable permissions via `chmod +x`.

4. **Default Output Paths:**
   Compiling currently mandates an explicit `-o <output>` argument rather than defaulting to the stem of the source file (e.g., `aipl build app.aipl` $\to$ `./app`).

---

## Standalone Distribution Plan

Achieving a self-contained `~/.local/bin/aipl` binary requiring no external dependencies involves three primary steps:

### 1. Embedded or Standardized `std/` Resolution
The entire standard library in `aipl_src/std/*.aipl` comprises approximately **115 KB across 19 files**.
- **Option A:** Embed standard library sources directly into the compiler binary as static string constants for a fully zero-dependency single-file executable.
- **Option B:** Expand search candidates in the resolver to look up `$AIPL_STD` $\to$ `./aipl_src/std/` $\to$ `~/.local/share/aipl/std/`.

### 2. Full Feature Parity in the Self-Hosted Driver
Extend [`aipl_src/aipl.aipl`](file:///home/matt/Projects/Repos/AIPL/aipl_src/aipl.aipl) to expose:
- `aipl verify <file.aipl>`
- `aipl build <file.aipl>` (defaulting to native executable output with the input stem)
- `aipl compile [--opt] [--exe [--sandbox]] <file.aipl> [-o <out>]`
- `aipl test <file.aipl>`

### 3. Minimal Installation Script
A self-contained installation flow using the checked-in stage-0 seed (`bootstrap/aipl-linux-x86_64`):
```sh
#!/bin/sh
set -eu
PREFIX="${PREFIX:-$HOME/.local}"

# 1. Build the compiler from source using the checked-in native seed
sh bootstrap/build.sh

# 2. Copy the standard library
mkdir -p "$PREFIX/share/aipl/std" "$PREFIX/bin"
cp aipl_src/std/*.aipl "$PREFIX/share/aipl/std/"

# 3. Install the compiler binary
cp target/bootstrap/aipl "$PREFIX/bin/aipl"
chmod +x "$PREFIX/bin/aipl"
```

