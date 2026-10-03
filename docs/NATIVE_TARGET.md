# Native performance: WebAssembly plus ahead-of-time compilation

AIPL has one compilation target: WebAssembly (with WASI for I/O). There is no
native code generator, and there will not be a hand-written one.

## Why

- **Wasm is the semantic reference.** The VM, the Rust backend, and the
  self-hosted compiler are all checked against wasm behaviour
  (AIPL_SPEC.md 10.4, 10.6). A second, native backend would be a fourth
  implementation of the semantics to keep identical, for little gain.
- **Wasm already runs at native speed.** Runtimes such as wasmtime compile wasm
  to machine code (Cranelift) before running it. Measured on this repository
  (release build): recursive `fib(27)` takes about 1 ms as compiled AIPL under
  wasmtime against about 460 ms in the VM, and the self-hosted compiler
  compiles itself in about 20 ms.
- **The old ELF emitter could not have grown into a compiler.** It had no
  labels, jumps, calls, stack frames, or memory access, hardcoded its segment
  size, and mis-encoded a 32-bit `cmpxchg`. It is quarantined in
  `attic/elf_emitter.aipl` with the other modules that returned constants
  instead of doing work.

## Running compiled AIPL natively

```bash
aipl compile program.aipl -o program.wasm
wasmtime run --dir=. program.wasm --invoke main      # JIT-compiled to native code
wasmtime compile program.wasm -o program.cwasm         # optional: precompile ahead of time
wasmtime run --allow-precompiled --dir=. program.cwasm --invoke main
```

`wasmtime compile` produces machine code for the host CPU, but the result
still runs inside wasmtime (it supplies WASI and memory sandboxing); it is not
a standalone executable. A standalone binary is possible with `wasm2c` plus a C
compiler and a WASI runtime library. Neither path needs anything from AIPL
beyond the `.wasm` it already produces, which is why `aipl` has no
`build-native` command.

## If a native backend is ever wanted

Build it as a wasm-to-native translator written in AIPL, after import
resolution is self-hosted (P14), so the self-hosted toolchain stays a single
pipeline with wasm as the one intermediate form and the semantic reference.
