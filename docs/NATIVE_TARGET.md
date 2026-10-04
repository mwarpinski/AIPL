# Native code: the P13 decision and why it changed

**Superseded (2026-10-02).** The current plan is [NATIVE_BACKEND_PLAN.md](NATIVE_BACKEND_PLAN.md): a Linux x86-64 backend written in AIPL that translates the wasm AIPL already produces into a standalone ELF executable. This file records the earlier decision so its reasoning is not lost.

## What P13 decided (2026-10-01)

AIPL would have one compilation target, WebAssembly with WASI, and no native code generator. The reasons:

- **Wasm is the semantic reference.** The VM, the Rust backend, and the self-hosted compiler are all checked against wasm behaviour (AIPL_SPEC.md 10.4, 10.6). A native backend is another implementation of those semantics to keep identical.
- **Wasm already runs at native speed.** wasmtime compiles wasm to machine code with Cranelift. Measured then (release build): recursive `fib(27)` took about 1 ms as compiled AIPL under wasmtime and about 460 ms in the VM.
- **The old ELF emitter could not grow into a compiler.** It had no labels, jumps, calls, stack frames, or memory access, hardcoded its segment size, and mis-encoded a 32-bit `cmpxchg`. It stays quarantined in `attic/elf_emitter.aipl`.

## What changed (2026-10-02)

The project owner set new goals (PROGRESS.md, "Direction"): AIPL programs ship as single standalone executables, and the toolchain migrates off Rust. Running under wasmtime meets neither by itself: a `.wasm` needs a runtime installed, and the standalone launcher (`aipl compile --exe`, AIPL_SPEC.md 6.5) is about 18 MB of Rust and wasmtime per program.

The new plan keeps everything P13 valued:

- **Wasm stays the one semantic reference.** The native backend translates wasm, not AIPL, so language features still only change AIPL to wasm, and every native build is tested by matching the same program under wasmtime.
- **It is written in AIPL,** after self-hosting (P14), as P13 itself said a native backend should be ("a wasm-to-native translator written in AIPL, after import resolution is self-hosted").
- **It starts from nothing in the attic.** The new reader, encoder, and ELF writer (`aipl_src/native/`) are tested against wasmparser, GNU as, and the Linux kernel itself.

Wasm plus the launcher remains the path on every platform without a native backend, and a plain `.wasm` still runs under any WASI host:

```bash
aipl compile program.aipl -o program.wasm
aipl run program.wasm -- ARGS                      # the aipl-run launcher
wasmtime run --dir=. program.wasm                   # any WASI host
```
