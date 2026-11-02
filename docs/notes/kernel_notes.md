# Bare-Metal OS and Microkernel Exploration in AIPL

Because AIPL's native backend ([`aipl_src/native/`](file:///home/matt/Projects/Repos/AIPL/aipl_src/native/lower.aipl)) emits raw x86-64 machine code and ELF headers with no runtime or C library dependencies, implementing bare-metal code in AIPL is architecturally feasible.

---

## Memory Model Alignment

In [`aipl_src/native/lower.aipl`](file:///home/matt/Projects/Repos/AIPL/aipl_src/native/lower.aipl#L31-L37):
1. All `(mem.load32 addr)` and `(mem.store32 addr val)` operations compile to single x86-64 memory instructions offset by register `r15` (`[r15 + addr]`), which serves as the linear memory base.
2. In user-space execution, `r15` is initialized by `mmap`.
3. In a freestanding or kernel environment, setting `r15 = 0` (or mapping to physical memory) allows operations like `(mem.store16 753664 val)` (`0xB8000`) within `(unsafe ...)` blocks to write directly to VGA buffers or MMIO registers.

---

## Technical Considerations & Prerequisites

Developing a freestanding AIPL kernel involves several concrete architectural steps:

1. **Bootloader Handoff:**
   QEMU's `-kernel` direct ELF loading expects 32-bit entry under Multiboot/PVH. A modern 64-bit bootloader such as **Limine** provides long-mode initialization, page table setup, and a memory map directly to a 64-bit ELF entry point.
2. **Hardware Instructions as Wasm Imports:**
   Because WebAssembly does not define privileged hardware instructions (`in`, `out`, `lidt`, control registers), hardware primitives must be modeled as imports (e.g., an `aipl_cpu` interface) that [`aipl_src/native/lower.aipl`](file:///home/matt/Projects/Repos/AIPL/aipl_src/native/lower.aipl) lowers directly to target CPU instructions:
   - Port I/O: `(cpu.in8 port:i32) -> i32`, `(cpu.out8 port:i32 byte:i32) -> void`
   - Paging & Registers: `(cpu.read_cr3) -> i64`, `(cpu.write_cr3 phys:i64) -> void`
   - Interrupt Management: `(cpu.lidt addr:i64) -> void`, `(cpu.sti) -> void`, `(cpu.cli) -> void`, `(cpu.hlt) -> void`
3. **Interrupt Handling Stubs:**
   Functions designated as interrupt handlers require prologue and epilogue code to save CPU register state and exit via `iretq`.
4. **Trap and Fault Recovery:**
   User-space out-of-bounds checks rely on POSIX signals. A kernel environment must handle page faults directly via an Interrupt Descriptor Table (IDT).

---

## Suggested Incremental Milestones

| Milestone | Target | Requirements |
|---|---|---|
| **M0** | Boot & Serial Output | Limine bootloader integration, freestanding runtime stub, `cpu.out8` serial driver |
| **M1** | Interrupts & Exceptions | IDT installation, interrupt entry/exit stubs, timer/keyboard handling |
| **M2** | Memory Management | Physical frame allocator, paging setup, kernel heap with generational safety checks |
| **M3** | Task Switching | Context switching, basic user-space execution with syscall entry |
| **M4** | Minimal Syscall Surface | Implement the ~20 core Linux syscalls required by the self-hosted AIPL compiler |

---

## Microkernel Architecture Fit

A microkernel model aligns particularly well with AIPL's language features:
- The core kernel remains minimal (scheduling, address space management, IPC).
- Device drivers and filesystems operate as isolated user processes.
- AIPL's `(plain T)` types (guaranteed to contain no heap pointers or references) provide a natural type-safe boundary for inter-process communication without shared-memory concurrency hazards.

