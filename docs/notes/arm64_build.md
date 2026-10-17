# Building AIPL for ARM64 / macOS

There are two primary approaches to building and running AIPL on ARM64 / macOS **without needing a physical Mac**:

---

## 1. Immediate Approach: The `AIPLEXE1` Wasm-Launcher Bundle + GitHub Actions

As implemented in [`src/main.rs`](file:///home/matt/Projects/Repos/AIPL/src/main.rs#L217-L236), AIPL includes a cross-platform standalone executable format:
- On platforms without a native machine-code backend, `aipl compile --exe --target wasm` takes the `aipl-run` binary (which embeds Wasmtime), appends the compiled `.wasm` bytecode to the end of the file, and tacks on a 16-byte `AIPLEXE1` footer.
- When executed, the binary inspects its own trailing `AIPLEXE1` footer and runs the embedded `.wasm` payload directly at near-native speed via Wasmtime's AArch64 Cranelift JIT.

### Building via CI Without a Mac
GitHub Actions provides free **`macos-14` (Apple Silicon M1 ARM64)** and **`ubuntu-24.04-arm` (Linux ARM64)** runners. A 15-line `.github/workflows/release.yml` workflow can build `aipl-darwin-arm64` and `aipl-linux-arm64` automatically on every release or push.

---

## 2. Native AIPL Approach: Adding an ARM64 (`aarch64`) Backend to `aipl_src/native/`

Because [`aipl_src/native/`](file:///home/matt/Projects/Repos/AIPL/aipl_src/native/native.aipl) translates **WebAssembly to machine code** (rather than lowering the AST directly), adding native ARM64 support is well-isolated:
- [`wasm_reader.aipl`](file:///home/matt/Projects/Repos/AIPL/aipl_src/native/wasm_reader.aipl) (100% of the Wasm parser and validation logic) remains **completely unchanged**.
- **ARM64 instruction encoding (`arm64_emit.aipl`) is simpler than [`x64_emit.aipl`](file:///home/matt/Projects/Repos/AIPL/aipl_src/native/x64_emit.aipl):**
  - Every ARM64 instruction is a fixed 32-bit `i32` word (no variable-length REX/ModRM/SIB prefix bytes like x86-64).
  - ARM64 has 31 general-purpose registers (`x0..x30`) and 32 float registers (`v0..v31`), so the virtual-stack cache in [`lower.aipl`](file:///home/matt/Projects/Repos/AIPL/aipl_src/native/lower.aipl) rarely spills in typical functions.
  - Integer division on ARM64 (`sdiv` / `udiv`) does not trap on `INT_MIN / -1` or `/ 0`, unlike x86-64 `idiv`.

### Local Testing on Linux x86-64
Using `qemu-aarch64` user-mode emulation on Linux x86-64, local machines can directly execute generated Linux ARM64 ELF binaries (`./aipl-linux-arm64`) as transparently as native x86-64 binaries.

