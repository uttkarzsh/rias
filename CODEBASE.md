# CODEBASE.md — RiasOS (`rias`)

A reference for how this codebase works. It describes the **working tree** of branch `mother` as of 2026-11-05: commit `a6ddc23 feat: add async executor` plus uncommitted changes (see §2, *Working tree vs. HEAD*). Everything comes from reading the source unless it is marked **(inferred)** or **(verified)**. **(verified)** means it was checked by running `cargo build` / `cargo test` on 2026-11-05 (macOS host, Homebrew QEMU, nightly toolchain), after the `src/main.rs` test fix described in §2 (*Working tree vs. HEAD*).

---

## 1. Repository overview

**What it is:** RiasOS is a hobby x86_64 kernel written in `no_std` Rust. It follows Philipp Oppermann's *Writing an OS in Rust* (blog_os) series, which `README.md` cites, up to and including the final *Async/Await* post. The kernel boots through the `bootloader` crate (v0.9) and then:

- writes text to the VGA text buffer and to a serial port,
- sets up a GDT/TSS (with a dedicated double-fault stack) and an IDT,
- handles CPU exceptions (breakpoint, page fault, double fault) and hardware interrupts (timer, keyboard),
- walks and modifies page tables, and allocates physical frames from the bootloader's memory map,
- maps a 100 KiB kernel heap and serves `alloc` from a fixed-size-block allocator,
- runs a **cooperative async executor** (`Future`-based tasks with wakers). The one task it runs decodes keyboard scancodes, which the interrupt handler pushes into a lock-free queue, and echoes the keys to the screen.

`executor.run()` never returns. When no task is ready, the executor halts the CPU (`hlt`) until the next interrupt. There is no preemptive scheduler, shell, filesystem or user space yet. The untracked `PLAN.md` in the repo describes the roadmap for those.

**Crate layout:** one Cargo package with two targets:
- **library** `rias` (`src/lib.rs`), which holds all kernel subsystems and the test framework;
- **binary** `rias` (`src/main.rs`), the actual kernel entry point, which uses the library.

Every module is a directory with a `mod.rs` (`src/<module>/mod.rs`), and `gdt` now lives under `interrupts` (`rias::interrupts::gdt`). Integration tests in `tests/` are each a separate bootable kernel that links the library.

### Architecture diagram

```
               bootloader 0.9 (BIOS boot, maps all phys mem at an offset)
                              │  &'static BootInfo
                              ▼
  src/main.rs  kernel_main
     │ println! banner
     │ rias::init() ─────────► src/lib.rs init()
     │                           ├─ interrupts::gdt::init()  (GDT + TSS, IST)
     │                           ├─ interrupts::init_idt()   (IDT)
     │                           ├─ PICS.initialize()        (8259 PIC remap)
     │                           └─ sti (enable interrupts)
     │ memory::init(offset) ───► OffsetPageTable (page-table mapper)
     │ BootInfoFrameAllocator::init(memory_map) ─► physical frames
     │ allocator::init_heap(mapper, frames) ─► maps heap pages, inits #[global_allocator]
     │ Executor::new(); spawn(Task::new(keyboard::print_keypresses()))
     └ executor.run() -> !   loop { run_ready_tasks(); sleep_if_idle() /* hlt */ }
                                       ▲ TaskId pushed            │ Task::poll
                                       │ to task_queue            ▼
                               TaskWaker::wake          print_keypresses (async fn)
                                       ▲                  ScancodeStream.next().await
                                       │ WAKER.wake()       ─► pc-keyboard decode ─► print!
                                       │                            ▲ pop
  IRQ1 ─► IDT ─► keyboard_interrupt_handler ─► inb 0x60 ─► add_scancode ─► SCANCODE_QUEUE
                                                                           (ArrayQueue<u8>, cap 100)
  IRQ0 ─► timer_interrupt_handler ─► print!(".")
  #BP  ─► println!(frame), returns     #PF ─► println!(CR2, err) ─► hlt_loop
  #DF  ─► (on IST stack) panic!

  Output:  print!/println!  ─► vga_buffer::WRITER  ─► 0xb8000 (screen)
           serial_print!    ─► serial::SERIAL1     ─► COM1 0x3F8 (host stdio in tests)
  Tests:   test_runner ─► serial output ─► exit_qemu(port 0xf4) ─► QEMU exit code
```

### Major components

| Component | File(s) | Responsibility |
|---|---|---|
| Kernel entry | `src/main.rs` | Boot entry (`kernel_main`), subsystem init order, starts the executor, panic handler |
| Kernel library root | `src/lib.rs` | Module tree, `init()`, `hlt_loop()`, QEMU exit, custom test framework |
| Console output | `src/vga_buffer/mod.rs` | VGA text-mode writer plus `print!`/`println!` |
| Serial output | `src/serial/mod.rs` | UART 16550 on COM1 plus `serial_print!`/`serial_println!` |
| Segmentation | `src/interrupts/gdt.rs` | GDT, TSS, interrupt stack table entry for double faults |
| Interrupts | `src/interrupts/mod.rs` | IDT, exception handlers, PIC setup, timer and keyboard IRQs |
| Paging / frames | `src/memory/mod.rs` | Page-table access, manual address translation, physical frame allocator |
| Heap | `src/allocator/mod.rs`, `src/allocator/*.rs` | Heap mapping, the global allocator, three allocator designs |
| Async tasks | `src/task/mod.rs`, `src/task/*.rs` | `Task`/`TaskId`, a simple and a waker-based executor, the async keyboard stream |
| Tests | `tests/*.rs` and `#[test_case]`s in `src/` | Boot, panic, stack overflow and heap tests run inside QEMU |

### Entry points

- **Normal boot:** `src/main.rs` → `entry_point!(kernel_main)` (bootloader macro; it type-checks the signature and exports `_start`).
- **`cargo test` of the library:** `src/lib.rs` → `entry_point!(test_kernel_main)` under `#[cfg(test)]`.
- **`cargo test` of the binary:** under `cfg(test)`, `kernel_main` runs the same init and heap setup, then calls `test_main()` and `hlt_loop()` **instead of** starting the executor (the executor code is behind `#[cfg(not(test))]`). The binary has no `#[test_case]`s of its own, so it reports `tests running: 0` and exits through QEMU's exit device.
- **Integration tests:** each file in `tests/` defines its own `_start` or `entry_point!`.

---

## 2. File-by-file explanation

### Root / configuration

| File | Purpose |
|---|---|
| `Cargo.toml` | Package `rias`, edition 2024. `[[bin]] rias` with `test = true`, `bench = false`. Dependencies: `bootloader` 0.9 (feature `map_physical_memory`), `volatile` 0.2.6, `spin` 0.5.2, `x86_64` 0.15.5 (`instructions`, `abi_x86_interrupt`), `uart_16550` 0.6.0, `pic8259` 0.11.0, `pc-keyboard` 0.7.0, `linked_list_allocator` 0.9.0, `lazy_static` 1.0 (`spin_no_std`), and, for the async code, `crossbeam-queue` 0.3.11, `conquer-once` 0.2.0 and `futures-util` 0.3.4. All three async crates have default features off; `crossbeam-queue` and `futures-util` enable `alloc`. `[package.metadata.bootimage]` passes QEMU test args (`isa-debug-exit` at port `0xf4`, serial to stdio, no display), sets success exit code `33` and a 150 s timeout. `[[test]] stack_overflow` uses `harness = false`. |
| `.cargo/config.toml` | Nightly `build-std` for `core`, `compiler_builtins`, `alloc` (with `compiler-builtins-mem` for `memcpy` and friends); `json-target-spec = true`; `build-dir-new-layout = false`, because bootimage finds test binaries by their `deps/` parent directory and Cargo's new build-dir layout breaks that; default target `x86_64-rias.json`; runner `bootimage runner` for `target_os = "none"`. |
| `x86_64-rias.json` | Custom bare-metal target: `os: none`, linker `rust-lld`, `panic-strategy: abort`, `disable-redzone: true` (so interrupts can't corrupt the stack red zone), SSE/MMX disabled with soft-float (`rustc-abi: softfloat`). |
| `rust-toolchain.toml` | Pins the `nightly` channel, which the unstable features and `build-std` need. |
| `README.md` | Project intro, roadmap, and a link to the reference blog. The roadmap ticks boot, output, interrupts, memory, heap and keyboard. **Cooperative multitasking is still unchecked although it is implemented.** |
| `PLAN.md` | **Untracked.** Roadmap past the blog in 9 phases (0 cleanup → 8 stretch goals): time/logging, shell, memory management, preemptive threads, bootloader 0.11/APIC, user space, filesystem. It has its own list of known gaps, which overlaps with §7. |
| `.gitignore` | Ignores `/target` and `.DS_Store` (the `.DS_Store` line is uncommitted). |
| `qemu-system-x86_64` | **A tracked 0-byte file at the repo root.** No code refers to it. Its purpose is unclear and it looks accidental. `PLAN.md` Phase 0 plans to delete it. |

**Excluded from documentation:** `Cargo.lock` (lockfile) and `target/` (build output, ignored).

#### Working tree vs. HEAD

These changes are **uncommitted** on top of `a6ddc23`:
- `Cargo.toml` / `Cargo.lock`: the `crossbeam-queue`, `conquer-once` and `futures-util` dependencies.
- `tests/stack_overflow.rs`: `rias::gdt::…` → `rias::interrupts::gdt::…`, following the module move.
- `src/main.rs`: `test_main()` now runs **before** the executor, under `#[cfg(test)]`, and the executor setup is under `#[cfg(not(test))]`. This fixed a `cargo test` hang (see §4.6).
- `.gitignore`: `.DS_Store`.

`a6ddc23` already contains `src/task/` (which uses those crates) and the `gdt` move. So HEAD on its own should **not compile**: the library is missing dependencies and `stack_overflow` uses a path that no longer exists **(inferred from reading; HEAD was not built)**. The working tree builds **(verified)**.

### `src/lib.rs` — kernel library root

Declares the modules `allocator`, `interrupts` (which contains `gdt`), `memory`, `serial`, `task` and `vga_buffer`, and enables `extern crate alloc`. Crate attributes turn on `custom_test_frameworks` (with `test_runner` = `crate::test_runner` and the generated harness renamed to `test_main`) and `abi_x86_interrupt`. It provides the shared kernel helpers and the in-QEMU test framework that `main.rs` and every integration test use.

### `src/main.rs` — kernel binary

Runs the boot sequence (see §4.1) and then, in a normal build, hands control to the async `Executor`, which runs one task, `keyboard::print_keypresses()`. In a test build the executor is compiled out and the code calls `test_main()` instead. A spawn of `example_task()` is commented out; that function no longer exists. The `Box`/`Vec`/`Rc` heap demo has been removed, but its imports (`Box`, `Rc`, `vec`, `Vec`) and the `print` and `SimpleExecutor` imports are still there and unused. It defines two `#[panic_handler]`s chosen by `cfg(test)`: the normal one `println!`s the panic info to VGA and halts, and the test one delegates to `rias::test_panic_handler`.

### `src/vga_buffer/mod.rs` — VGA text console

The writer is memory-mapped at physical/virtual `0xb8000`, an 80×25 buffer of (ASCII byte, color byte) cells. Writes go through `volatile::Volatile` so the compiler can't drop them as dead stores. Text always goes into the **bottom row** and the screen scrolls up on a newline or when a line wraps. Bytes outside printable ASCII (`0x20..=0x7e`) and `\n` are shown as `0xfe` (■). The default color is red on black. Interrupt handlers, `task::keyboard`, `main.rs` and the tests use it through `print!`/`println!`.

### `src/serial/mod.rs` — serial port output

A `lazy_static` `SERIAL1` wraps a `uart_16550::Uart16550Tty<PioBackend>` on I/O port `0x3F8` (COM1) with default config, created on first use. QEMU is started with `-serial stdio` in tests, so this is how test results reach the host terminal. The test framework (`lib.rs`) and the tests use it.

### `src/interrupts/gdt.rs` — GDT and TSS

Builds a TSS whose interrupt stack table slot `DOUBLE_FAULT_IST_INDEX` (0) points at the **end** (top) of a 20 KiB `static mut` byte array, because x86 stacks grow down. It then builds a GDT with a kernel code segment plus that TSS, and `init()` loads both. This lets the double-fault handler run on a known-good stack even after a kernel stack overflow. The parent `interrupts` module (IST index), `lib::init` and `tests/stack_overflow.rs` use it.

### `src/interrupts/mod.rs` — IDT, exceptions, hardware IRQs

Declares the `gdt` submodule. Sets up the `InterruptDescriptorTable` and the chained 8259 PICs, which are remapped to vectors 32–47 so they don't overlap CPU exception vectors. The handlers use the `extern "x86-interrupt"` ABI. The keyboard handler now does as little as possible: it reads the scancode from port `0x60` and hands it to `task::keyboard::add_scancode`. Decoding has moved to the async task. Each IRQ handler must send an EOI to the PIC, or no more interrupts of that kind arrive.

### `src/memory/mod.rs` — paging and physical frames

Assumes the bootloader mapped **all physical memory** at `physical_memory_offset` (the `map_physical_memory` feature). Any physical address `p` can then be reached at virtual address `offset + p`, which is how page tables are read and written. It provides a hand-written 4-level page walk (learning code), an `x86_64::OffsetPageTable` mapper for real use, and a frame allocator driven by the bootloader memory map.

### `src/allocator/mod.rs` and `src/allocator/` — kernel heap

`allocator/mod.rs` defines the heap region (`HEAP_START = 0x4444_4444_0000`, `HEAP_SIZE = 100 KiB`, which is 25 pages of 4 KiB), maps it, and registers the `#[global_allocator]`. The submodules hold three allocator designs, each wrapped in `Locked<A>`. **Only `FixedSizeBlockAllocator` is active.** The bump and linked-list allocators, `Dummy`, and the `linked_list_allocator::LockedHeap` import are still compiled but unused, left over from earlier steps.

| File | Design |
|---|---|
| `src/allocator/bump.rs` | Bump allocator, plus the shared `Locked<A>` wrapper |
| `src/allocator/linked_list.rs` | Free-list allocator written in this repo (first-fit, no coalescing) |
| `src/allocator/fixed_size_block.rs` | Segregated free lists for 8–2048-byte blocks, falling back to `linked_list_allocator::Heap` (**active**) |

### `src/task/` — cooperative multitasking (new)

| File | Contents |
|---|---|
| `src/task/mod.rs` | `TaskId` (unique, from a global atomic counter) and `Task` (a pinned, boxed `Future<Output = ()>` plus its id). Declares the submodules. |
| `src/task/simple_executor.rs` | First-step executor: a `VecDeque<Task>` polled round-robin with a no-op waker. Pending tasks are re-queued straight away, so it busy-polls. **Imported in `main.rs` but unused.** |
| `src/task/executor.rs` | The real executor: tasks in a `BTreeMap`, a shared `ArrayQueue<TaskId>` of ready tasks, a cache of one `Waker` per task, and a sleep-when-idle loop. **Active.** |
| `src/task/keyboard.rs` | The bridge between the keyboard interrupt and async code: a global scancode `ArrayQueue<u8>`, an `AtomicWaker`, the `ScancodeStream` (`futures::Stream`), and the `print_keypresses` task. |

Everything in `task` allocates (`Box::pin`, `Arc`, `BTreeMap`, `ArrayQueue::new`), so it must only be used after `allocator::init_heap`.

### `tests/` — integration tests (each one is its own kernel)

| File | What it checks | How it works |
|---|---|---|
| `tests/basic_boot.rs` | `println!` works right after boot, before any `init()` | Raw `#[unsafe(no_mangle)] _start` → `test_main()`; uses `rias::test_runner` and `rias::test_panic_handler`. |
| `tests/should_panic.rs` | A failing assertion reaches the panic handler | `_start` calls `should_fail()` (`assert_eq!(0, 1)`). The custom panic handler prints `[ok]` and exits with Success. If execution continues past the panic, it exits with Failed. No `#[test_case]`s are used. |
| `tests/stack_overflow.rs` | A kernel stack overflow becomes a double fault handled on the IST stack | `harness = false`. Calls `rias::interrupts::gdt::init()` and loads its own `TEST_IDT`, whose double-fault handler (on `rias::interrupts::gdt::DOUBLE_FAULT_IST_INDEX`) prints `[ok]` and exits with Success. `stack_overflow()` recurses forever, and a `Volatile` read stops tail-call optimization. |
| `tests/heap_allocation.rs` | The heap allocator works and reuses memory | `entry_point!(main)` → full `init` plus heap setup → `test_main()`. Tests: `simple_allocation`, `large_vec` (1000 `u64`s), `many_boxes` (`HEAP_SIZE` = 102 400 short-lived boxes, which only fits if freed memory is reused), `many_boxes_long_lived` (same, with one box kept alive the whole time). |

There are no tests for `src/task/`.

---

## 3. Functions, methods and types

### `src/lib.rs`

| Item | Description |
|---|---|
| `enum QemuExitCode { Success = 0x10, Failed = 0x11 }` | `#[repr(u32)]` value written to QEMU's `isa-debug-exit` device. QEMU exits with `(value << 1) \| 1`, so Success becomes host exit code **33**, which matches `test-success-exit-code`. |
| `fn exit_qemu(exit_code)` | Writes the code to I/O port `0xf4`, which shuts QEMU down. Only works when QEMU runs with the `isa-debug-exit` device (the test args). |
| `fn hlt_loop() -> !` | Loops on `hlt` forever, sleeping the CPU until the next interrupt. Used as the terminal state after panics and faults. |
| `fn init()` | Kernel CPU setup in order: `interrupts::gdt::init()`, `interrupts::init_idt()`, `PICS.lock().initialize()`, then `interrupts::enable()` (`sti`). Timer and keyboard IRQs start arriving after this. |
| `trait Testable { fn run(&self) }` | Blanket-implemented for every `T: Fn()`. `run` prints the test's type name (`core::any::type_name::<T>()`), calls the test, then prints `[ok]` over serial. |
| `fn test_runner(tests: &[&dyn Testable])` | The custom test framework runner. Prints the test count, runs each test, then calls `exit_qemu(Success)`. A failing test panics instead, and the panic handler exits with Failed. |
| `fn test_panic_handler(info) -> !` | Shared test panic path. Prints `[failed]` and the error over serial, then `exit_qemu(Failed)` and `hlt_loop()`. |
| `fn test_kernel_main(_boot_info) -> !` *(cfg(test))* | Entry point for the library's unit tests: `init()` → `test_main()` → `hlt_loop()`. It does **not** set up the heap, so library unit tests can't allocate, and so can't exercise `task`. |
| `fn panic(info)` *(cfg(test))* | Panic handler for library unit tests; calls `test_panic_handler`. |

### `src/main.rs`

| Item | Description |
|---|---|
| `fn kernel_main(boot_info: &'static BootInfo) -> !` | The real kernel entry, registered with `entry_point!`. Prints the ASCII banner, calls `rias::init()`, builds the page-table mapper and frame allocator, calls `init_heap` (panics on failure), then branches on `cfg`. **`#[cfg(test)]`:** `test_main()` then `hlt_loop()` (`test_main` normally exits QEMU first). **`#[cfg(not(test))]`:** creates an `Executor`, spawns `Task::new(keyboard::print_keypresses())`, and calls `executor.run()`, which never returns. The old `println!("it did not crash yay")` is gone. |
| `fn panic(info)` *(not test)* | Prints the panic to VGA and halts. |
| `fn panic(info)` *(test)* | Delegates to `rias::test_panic_handler`. |

### `src/vga_buffer/mod.rs`

| Item | Description |
|---|---|
| `enum Color` | The 16 VGA colors (`#[repr(u8)]`, values 0–15). |
| `struct ColorCode(u8)` | Packed attribute byte, `background << 4 \| foreground`. `ColorCode::new(fg, bg)` builds it. |
| `struct ScreenChar { ascii_character, color_code }` | One 2-byte VGA cell (`#[repr(C)]` so the field order matches the hardware). |
| `BUFFER_HEIGHT = 25`, `BUFFER_WIDTH = 80` | Text-mode dimensions. |
| `struct Buffer { chars: [[Volatile<ScreenChar>; 80]; 25] }` | `#[repr(transparent)]` overlay on the VGA memory at `0xb8000`. |
| `struct Writer { column_position, color_code, buffer: &'static mut Buffer }` | Cursor state for the bottom row plus a reference to VGA memory. |
| `Writer::write_byte(byte)` | `\n` starts a new line. Any other byte wraps first if the row is full, then writes to `[last_row][column_position]` and moves the cursor forward. |
| `Writer::new_line()` | Copies rows 1..25 up one row (scrolling), clears the last row, and resets the column to 0. |
| `Writer::clear_row(row)` | Fills a row with spaces in the current color. |
| `Writer::write_string(s)` | Writes each byte, replacing non-printable and non-ASCII bytes with `0xfe`. UTF-8 multi-byte characters therefore show as several ■. |
| `impl fmt::Write for Writer` | `write_str` → `write_string`. This makes `write!`/`format_args!` work. |
| `static WRITER: Mutex<Writer>` (`lazy_static`) | The global console. It is lazily initialized because it needs a raw pointer cast, which can't happen in a `const`. |
| `macro print!` / `println!` | `#[macro_export]`ed (`rias::print`, `rias::println`) and forwarded to `_print`. |
| `fn _print(args)` | Locks `WRITER` with **interrupts disabled** (`without_interrupts`). This prevents a deadlock if an interrupt handler prints while the main code holds the lock. |
| Tests `test_println_simple`, `test_println_many`, `test_println_output` | The last one locks `WRITER` once with interrupts disabled, writes `"\n" + s`, and reads the characters back through the same guard (`writer.buffer.chars[BUFFER_HEIGHT - 2]`). |

### `src/serial/mod.rs`

| Item | Description |
|---|---|
| `static SERIAL1: Mutex<Uart16550Tty<PioBackend>>` | COM1 UART at port `0x3F8`, created on first use. Panics with `"failed to initialize UART"` if setup fails. |
| `fn _print(args)` | Writes the formatted text to `SERIAL1` with interrupts disabled. Panics if the write fails. |
| `macro serial_print!` / `serial_println!` | Exported macros. `serial_println!` uses `concat!` to append `\n` to a literal format string. |

### `src/interrupts/gdt.rs`

| Item | Description |
|---|---|
| `const DOUBLE_FAULT_IST_INDEX: u16 = 0` | IST slot used by the double-fault handler. |
| `static TSS: TaskStateSegment` (`lazy_static`) | Its IST[0] points at the top of a 5 × 4 KiB `static mut STACK` (address taken with `&raw const`). There is no guard page. |
| `static GDT: (GlobalDescriptorTable, Selectors)` | GDT holding a kernel code segment and a TSS descriptor, plus the selectors `append` returned. |
| `struct Selectors { code_selector, tss_selector }` | Remembers the selectors so `init` can load them. |
| `fn init()` | Loads the GDT (`lgdt`), reloads `CS`, and loads the TSS (`ltr`). Must run before the IDT that refers to the IST index is used. |

### `src/interrupts/mod.rs`

| Item | Description |
|---|---|
| `pub mod gdt` | See above. |
| `PIC_1_OFFSET = 32`, `PIC_2_OFFSET = 40` | Remapped vector bases for the primary and secondary PIC. |
| `static PICS: spin::Mutex<ChainedPics>` | The chained 8259 PIC pair. `lib::init` initializes it, and handlers use it to send EOIs. |
| `enum InterruptIndex { Timer = 32, Keyboard = 33 }` | IDT vectors for IRQ0 and IRQ1. `as_u8()` converts to an index. |
| `static IDT` (`lazy_static`) | Registers the breakpoint, double-fault (on IST stack 0), page-fault, timer and keyboard handlers. |
| `fn init_idt()` | Loads the IDT (`lidt`). |
| `breakpoint_handler(frame)` | Prints `EXCEPTION: BREAKPOINT` and the stack frame, then **returns**, so execution continues after `int3`. |
| `double_fault_handler(frame, _code) -> !` | `panic!`s with the frame. It runs on the IST stack, so it survives stack overflows. |
| `timer_interrupt_handler(_)` | Prints `.` on every PIT tick, then sends an EOI. This is why dots keep appearing on screen, mixed in with typed characters. |
| `keyboard_interrupt_handler(_)` | Reads one scancode from port `0x60`, calls `crate::task::keyboard::add_scancode(scancode)`, then sends an EOI. It no longer decodes or prints keys itself. |
| `page_fault_handler(frame, error_code)` | Prints the faulting address (`Cr2::read()`), the error code and the frame, then `hlt_loop()`. It never returns, and no demand paging is done. |
| Test `test_breakpoint_exception` | Runs `int3` and expects execution to continue. |

### `src/memory/mod.rs`

| Item | Description |
|---|---|
| `unsafe fn active_level_4_table(offset) -> &'static mut PageTable` | Reads CR3 to get the L4 table's physical frame and returns it as a mutable reference at `offset + phys`. Private. It is unsafe because the caller must guarantee the mapping exists and that no other `&mut` aliases it. |
| `pub unsafe fn translate_addr(addr, offset) -> Option<PhysAddr>` | Public wrapper around `translate_addr_inner`. **Not called anywhere**: an earlier learning step, superseded by `OffsetPageTable`. |
| `fn translate_addr_inner(addr, offset)` | Hand-written 4-level page walk: for each of P4/P3/P2/P1 it turns the current frame into a table reference through the offset and reads the entry. Returns `None` if an entry is not present and **panics on huge pages** (`"huge pass not supported"`). Result is the final frame plus the page offset. |
| `pub unsafe fn init(offset) -> OffsetPageTable<'static>` | Builds the `x86_64` crate's mapper over the active L4 table. Call it only once, to avoid aliasing `&mut`. |
| `pub fn create_example_mapping(page, mapper, frame_allocator)` | Test helper that maps `page` to the VGA frame `0xb8000` with PRESENT\|WRITABLE and panics on failure. **Not called anywhere** at present. |
| `struct EmptyFrameAllocator` | `FrameAllocator` that always returns `None`. **Unused** leftover. |
| `struct BootInfoFrameAllocator { memory_map: &'static MemoryMap, next: usize }` | Hands out physical frames from bootloader regions marked `Usable`, in order. |
| `BootInfoFrameAllocator::init(memory_map)` | `unsafe`: the caller vouches that the map is valid and those frames are really unused. |
| `BootInfoFrameAllocator::usable_frames()` | Iterator chain: regions → filter `Usable` → address ranges → `step_by(4096)` → `PhysFrame`. |
| `impl FrameAllocator<Size4KiB>::allocate_frame` | Returns `usable_frames().nth(next)` and increments `next`. Each call rebuilds and skips the iterator, so it costs **O(n)**. Frames are never freed. |

### `src/allocator/mod.rs`

| Item | Description |
|---|---|
| `static ALLOCATOR: Locked<FixedSizeBlockAllocator>` | The `#[global_allocator]` behind every `alloc` call (`Box`, `Vec`, `Rc`, `Arc`, `BTreeMap`, the `ArrayQueue`s). Built in a `const` context, so it is empty until `init_heap` runs. Allocating before then gets a null pointer from the empty fallback heap, which becomes an allocation-error panic **(inferred from Rust `alloc` semantics)**. |
| `HEAP_START = 0x_4444_4444_0000`, `HEAP_SIZE = 100 * 1024` | Virtual heap location (an arbitrary unused address that is easy to spot) and size. |
| `fn init_heap(mapper, frame_allocator) -> Result<(), MapToError<Size4KiB>>` | For each 4 KiB page in `[HEAP_START, HEAP_START + HEAP_SIZE)`: allocates a frame, then `map_to` with PRESENT\|WRITABLE and flushes the TLB entry. Fails with `FrameAllocationFailed` or a mapping error. Then calls `ALLOCATOR.lock().init(HEAP_START, HEAP_SIZE)`. |
| `struct Dummy` | `GlobalAlloc` that always returns null and panics on `dealloc`. **Unused** (first step of the heap tutorial). |
| `fn align_up(addr, align) -> usize` | Rounds `addr` up to `align` with a bitmask, `(addr + align - 1) & !(align - 1)`. `align` must be a power of two, which `Layout` guarantees. The bump and linked-list allocators use it. |

### `src/allocator/bump.rs`

| Item | Description |
|---|---|
| `struct Locked<A> { inner: spin::Mutex<A> }` | Newtype that works around Rust's orphan rule. `GlobalAlloc` takes `&self`, so each allocator needs interior mutability, and a foreign trait can't be implemented on `spin::Mutex<A>`, a foreign type. `const fn new` allows use in a `static`, and `lock()` returns the guard. The other allocator modules reuse it through `super::Locked`. |
| `struct BumpAllocator { heap_start, heap_end, next, allocations }` | Linear allocator. `next` is the bump pointer and `allocations` counts live allocations. |
| `BumpAllocator::new()` / `init(start, size)` | `const` empty constructor. `init` sets the bounds and resets `next`. |
| `GlobalAlloc::alloc` for `Locked<BumpAllocator>` | Aligns `next` up, checks the end for overflow and against `heap_end`, then bumps. Returns null when out of memory. |
| `GlobalAlloc::dealloc` | Only decrements the counter. Memory is reclaimed **all at once**, when the count drops to 0 and `next` resets to `heap_start`. |

### `src/allocator/linked_list.rs`

| Item | Description |
|---|---|
| `struct ListNode { size, next: Option<&'static mut ListNode> }` | Header written **inside** each free region. `start_addr()` is the node's own address; `end_addr()` is start + size. |
| `struct LinkedListAllocator { head: ListNode }` | A dummy head node of size 0 that starts the free list. |
| `new()` / `init(start, size)` | `init` adds the whole heap as one free region. |
| `add_free_region(addr, size)` | Asserts `addr` is aligned for `ListNode` and `size` can hold one, writes a node there, and pushes it to the **front** of the list. Adjacent free regions are not merged. |
| `find_region(size, align) -> Option<(&'static mut ListNode, usize)>` | First-fit scan. Unlinks the first region that `alloc_from_region` accepts and returns it with the aligned start address. |
| `alloc_from_region(region, size, align) -> Result<usize, ()>` | Accepts the region if the aligned allocation fits **and** the leftover is either 0 or large enough to hold a `ListNode`. |
| `size_align(layout) -> (usize, usize)` | Raises alignment to `align_of::<ListNode>()`, pads the size, and enforces a minimum size of `size_of::<ListNode>()`, so a freed block can always become a free-list node. |
| `GlobalAlloc` for `Locked<LinkedListAllocator>` | `alloc`: `find_region`, then puts any excess tail back on the list as a free region; null if nothing fits. `dealloc`: puts the block back with `add_free_region`. |

### `src/allocator/fixed_size_block.rs` (active global allocator)

| Item | Description |
|---|---|
| `struct ListNode { next }` | Intrusive singly linked node stored inside each free block. |
| `const BLOCK_SIZES = [8, 16, 32, 64, 128, 256, 512, 1024, 2048]` | Size classes. They are powers of two, so each size can also serve as its alignment. |
| `struct FixedSizeBlockAllocator { list_heads: [Option<&'static mut ListNode>; 9], fallback_allocator: linked_list_allocator::Heap }` | One free list per size class, plus the external crate's heap for larger requests and to create new blocks. |
| `new()` / `init(start, size)` | `const` constructor (uses a `const EMPTY` to repeat `None` in the array). `init` hands the whole heap to the fallback heap. |
| `fallback_alloc(layout)` | `allocate_first_fit` on the fallback heap; null on error. |
| `fn list_index(layout) -> Option<usize>` | Index of the smallest class `>= max(size, align)`; `None` if the request is over 2048 bytes. |
| `GlobalAlloc::alloc` | Small request: pop the class's free list, or, if it is empty, take a new `block_size`-aligned block from the fallback heap. Large request: go straight to the fallback heap. |
| `GlobalAlloc::dealloc` | Small block: write a `ListNode` into it and push it onto its class list. It is **never returned** to the fallback heap. Large block: `fallback_allocator.deallocate`. |

### `src/task/mod.rs`

| Item | Description |
|---|---|
| `struct TaskId(u64)` | `Copy`, `Ord`, so it can key a `BTreeMap` and be pushed into a lock-free queue. |
| `TaskId::new()` (private) | `fetch_add(1, Relaxed)` on a function-local `static NEXT_ID: AtomicU64`. Ids are unique and never reused. |
| `struct Task { id: TaskId, future: Pin<Box<dyn Future<Output = ()>>> }` | A heap-allocated, pinned, type-erased future. It is pinned because `Future::poll` takes `Pin<&mut Self>`, and async blocks may be self-referential. No `Send` bound (single core). |
| `Task::new(future: impl Future<Output = ()> + 'static)` | Boxes and pins the future and gives it a new id. `'static` because the task can outlive the caller's stack frame. |
| `Task::poll(&mut self, cx)` (private) | `self.future.as_mut().poll(cx)`. Visible to the child modules `executor` and `simple_executor`. |

### `src/task/simple_executor.rs`

| Item | Description |
|---|---|
| `struct SimpleExecutor { task_queue: VecDeque<Task> }` | FIFO of tasks. |
| `new()` / `spawn(task)` | Creates the executor / pushes a task to the back. |
| `run(&mut self)` | Pops a task, polls it with a dummy waker, and pushes it back if `Pending`. Returns when the queue is empty. Pending tasks are polled again straight away, so waiting tasks waste CPU (busy polling). |
| `dummy_raw_waker()` / `dummy_waker()` | A `RawWaker` with a null data pointer and a vtable whose `wake`/`wake_by_ref`/`drop` do nothing and whose `clone` makes another dummy. `Waker::from_raw` is `unsafe` because the vtable contract is not checked. |

### `src/task/executor.rs` (active executor)

| Item | Description |
|---|---|
| `struct TaskWaker { task_id, task_queue: Arc<ArrayQueue<TaskId>> }` | Waker payload: waking pushes its task id onto the executor's ready queue. |
| `TaskWaker::new(task_id, task_queue) -> Waker` | Wraps itself in an `Arc` and turns it into a `Waker` through `alloc::task::Wake`, with no hand-written vtable. |
| `TaskWaker::wake_task()` | `task_queue.push(task_id).expect("task_queue full")`. `ArrayQueue::push` is lock-free and doesn't allocate, so it is safe to call from the keyboard interrupt. It **panics** if the queue (capacity 100) is full. |
| `impl Wake for TaskWaker` | `wake` and `wake_by_ref` both call `wake_task`. |
| `struct Executor { tasks: BTreeMap<TaskId, Task>, task_queue: Arc<ArrayQueue<TaskId>>, waker_cache: BTreeMap<TaskId, Waker> }` | Tasks are stored by id. Only ids move through the ready queue, which the executor shares with every waker. Wakers are created once per task and reused. |
| `Executor::new()` | Empty maps and a 100-slot ready queue. Allocates, so it must run after `init_heap`. |
| `Executor::spawn(task)` | Inserts the task (panics on a duplicate id, which `TaskId::new` makes impossible) and pushes its id so it gets polled once. Panics `"queue full"` if the queue is full. There is no way for a running task to spawn another one: `spawn` needs `&mut Executor` and no spawner handle is exposed. |
| `Executor::run_ready_tasks()` | Destructures `self` so it can borrow the three fields separately. Pops ids until the queue is empty; skips ids whose task is gone; gets or creates the cached waker; polls. On `Ready` it removes the task and its waker. |
| `Executor::sleep_if_idle()` | Disables interrupts, checks whether the queue is empty, then either `enable_and_hlt()` (`sti; hlt`) or re-enables interrupts. Checking with interrupts off and using `sti; hlt` closes the race where an interrupt pushes a wakeup between the check and the `hlt`, which would otherwise sleep with work pending until the next interrupt. `sti` only takes effect after the following instruction, so no interrupt can land between `sti` and `hlt`. |
| `Executor::run(&mut self) -> !` | `loop { run_ready_tasks(); sleep_if_idle(); }`. |

### `src/task/keyboard.rs`

| Item | Description |
|---|---|
| `static SCANCODE_QUEUE: OnceCell<ArrayQueue<u8>>` | `conquer_once::spin::OnceCell`, so it can be initialized at runtime (the queue allocates) without `lazy_static`'s init-on-first-access, which would allocate inside the interrupt handler if a key arrived first. |
| `static WAKER: AtomicWaker` | `futures_util` atomic slot for the waker of whichever task is waiting on the stream. |
| `pub(crate) fn add_scancode(scancode)` | Called from `keyboard_interrupt_handler`. If the queue exists, pushes the byte and calls `WAKER.wake()`. If the queue is full or not yet created, it prints a `WARNING: …` and drops the byte. It neither allocates nor locks, apart from `println!`'s `WRITER` lock on the warning path. |
| `pub async fn print_keypresses()` | The kernel's only task. Creates the `ScancodeStream` and a `pc_keyboard::Keyboard` (US 104-key layout, scancode set 1, `HandleControl::Ignore`). For each scancode: `add_byte` → `process_keyevent` → `print!` of the Unicode character or the `RawKey` debug output. Never finishes. |
| `struct ScancodeStream { _private: () }` | The private field means it can only be built through `new`. |
| `ScancodeStream::new()` | `try_init_once(|| ArrayQueue::new(100))` and panics with `"ScancodeStream::new should be only called once"` on a second call. So there can be only one keyboard consumer. |
| `impl Stream for ScancodeStream { type Item = u8 }` → `poll_next` | Fast path: pop and return `Ready`. Otherwise `WAKER.register(cx.waker())` and pop **again**, which catches a scancode that arrived between the first pop and the registration. If that second pop succeeds, `WAKER.take()` removes the registration to avoid a pointless wakeup. Otherwise `Pending`. Never returns `Ready(None)`, so the stream never ends. |

### `tests/*.rs`

| Item | Description |
|---|---|
| `basic_boot::_start`, `test_println`, `panic` | Boots, prints over VGA, and reports through the shared runner and panic handler. |
| `should_panic::_start`, `should_fail`, `panic` | See §2. The panic handler is the success path. |
| `stack_overflow::_start`, `stack_overflow`, `TEST_IDT`, `init_test_idt`, `test_double_fault_handler`, `panic` | See §2. Its own IDT has only a double-fault entry, whose handler is the success path. |
| `heap_allocation::main`, `panic`, `simple_allocation`, `large_vec`, `many_boxes`, `many_boxes_long_lived` | See §2. |

---

## 4. Execution and data flow

### 4.1 Boot and initialization (`cargo run`)

1. **Start:** `bootimage runner` boots the disk image in QEMU. The bootloader switches to long mode, sets up identity and kernel mappings, maps all physical memory at `BootInfo::physical_memory_offset`, and calls `_start`, which `entry_point!` generates and which calls `kernel_main(&'static BootInfo)` in `src/main.rs`.
2. `println!` banner → `vga_buffer::_print` → `WRITER` (created now on first use) → volatile writes to `0xb8000`.
3. `rias::init()` (`src/lib.rs`):
   `interrupts::gdt::init()` loads the GDT, CS and TSS → `interrupts::init_idt()` loads the IDT → `PICS.initialize()` remaps the PICs to 32–47 → `sti`. From here the timer IRQ prints `.` periodically. Keys pressed now are dropped with `WARNING: scancode queue uninitialized`, because the queue doesn't exist yet (step 7).
4. **Paging:** `memory::init(VirtAddr::new(boot_info.physical_memory_offset))` reads CR3 and wraps the L4 table in an `OffsetPageTable`.
5. **Frames:** `BootInfoFrameAllocator::init(&boot_info.memory_map)` keeps a reference to the bootloader's region list.
6. **Heap:** `allocator::init_heap(&mut mapper, &mut frame_allocator)` takes 25 frames from the frame allocator, maps them at `0x4444_4444_0000…`, and `map_to` may take further frames for intermediate page tables. It then initializes `ALLOCATOR`'s fallback heap over that range.
7. **Executor:** `Executor::new()` allocates the ready queue. `Task::new(keyboard::print_keypresses())` boxes the (not yet started) async fn's future, and `spawn` queues its id. `executor.run()` polls it for the first time: the body runs up to its first `.await`, creating `SCANCODE_QUEUE` on the way (`ScancodeStream::new`). The queue is empty, so the task registers its waker and returns `Pending`, and the executor halts the CPU.
8. **Steady state:** the CPU sleeps in `sleep_if_idle` and wakes for each timer tick (prints `.`, queue still empty, sleeps again) or key press (§4.2). `kernel_main` never gets past `executor.run()` (in a normal build).

### 4.2 Keyboard input flow (interrupt → async task)

1. **Start:** a key press raises IRQ1 on the primary PIC → vector 33 → `keyboard_interrupt_handler` (`src/interrupts/mod.rs`) through the `x86-interrupt` ABI.
2. The handler reads the scancode from port `0x60` and calls `task::keyboard::add_scancode`, which pushes it onto `SCANCODE_QUEUE` (lock-free `ArrayQueue`) and calls `WAKER.wake()`.
3. `AtomicWaker::wake` takes the registered waker, the `TaskWaker` of `print_keypresses`, and calls it. `TaskWaker::wake_task` pushes that `TaskId` onto the executor's `task_queue`.
4. The handler sends an EOI (`PICS.lock().notify_end_of_interrupt(33)`) and returns with `iretq`. The CPU resumes after the `hlt` in `sleep_if_idle`.
5. `run()` loops back to `run_ready_tasks()`, pops the id, and polls the task with its cached waker. `ScancodeStream::poll_next` pops the scancode → `Keyboard::add_byte` → `KeyEvent` → `process_keyevent` → `DecodedKey` → `print!`. The next `poll_next` finds the queue empty, registers the waker again, and the task returns `Pending`.
6. **Result:** the character appears on the VGA screen and the CPU goes back to sleep. Interrupt context only queues bytes; decoding and printing run in task context.

### 4.3 Timer flow

PIT tick (IRQ0) → vector 32 → `timer_interrupt_handler` prints `.` → EOI. It does not interact with the executor. It only takes the CPU out of `hlt`, and `sleep_if_idle` goes back to sleep if no task is ready.

**Deadlock avoidance (both flows):** the main code's `print!` turns interrupts off while it holds `WRITER`, so a handler (the timer `.` or the `add_scancode` warnings) can never spin on a lock that the code it interrupted already holds.

### 4.4 Exception flow

- **Breakpoint (`int3`):** `breakpoint_handler` prints the frame and returns, and execution continues.
- **Page fault:** `page_fault_handler` reads CR2 (the faulting virtual address) and prints it with the `PageFaultErrorCode` flags, then halts forever.
- **Double fault:** this happens when an exception occurs while handling another one, for example a page fault on a guard page after a stack overflow, where pushing the frame faults again. The CPU switches to `TSS.interrupt_stack_table[0]` (`src/interrupts/gdt.rs`) before pushing the frame, so `double_fault_handler` runs on a fresh stack and `panic!`s → `main.rs` `panic` → VGA → `hlt_loop`.

### 4.5 Address translation and mapping

`OffsetPageTable` (from the `x86_64` crate) does the real work: `map_to(page, frame, flags, frame_allocator)` creates any missing P3/P2/P1 tables by taking frames from `BootInfoFrameAllocator` and writes them through the physical-memory offset. The result is a `MapperFlush`, which must be `.flush()`ed (`invlpg`). `translate_addr_inner` in `src/memory/mod.rs` shows the same walk by hand, but nothing calls it.

### 4.6 Test flow (`cargo test`)

1. `bootimage runner` builds a test kernel for each target (lib, bin, each `tests/*.rs`) and runs QEMU with the `test-args` from `Cargo.toml`. Cargo runs the targets in the order lib → bin → integration tests and **stops at the first failing target** unless `--no-fail-fast` is passed.
2. **Start:** the test kernel's entry calls `test_main()`, which the `custom_test_frameworks` feature generates from all `#[test_case]` items and which calls `rias::test_runner(&[...])`.
3. Each `Testable::run` prints `name...\t` and then `[ok]` over **serial**, which reaches the host stdout.
4. **Result:** after all tests pass, `exit_qemu(Success)` writes `0x10` to port `0xf4`, QEMU exits with 33, and bootimage reports success. A panic instead goes to `test_panic_handler` → `[failed]` and `Error: …` → `exit_qemu(Failed)`, and bootimage reports failure. A hang is killed after 150 s.
5. `should_panic` and `stack_overflow` don't use the runner. They decide the result themselves: in the panic handler or double-fault handler for success, and after execution continues past the expected fault for failure.
6. **Current state (verified):** `cargo test` completes and everything passes: the lib kernel (4 tests), the bin kernel (0 tests, exits cleanly), and all four integration tests. This used to hang: `kernel_main` called `executor.run()` (returns `!`) before `test_main()`, so the bin test kernel never exited, bootimage reported `Error: Test timed out` after 150 s, and Cargo skipped the integration tests. The fix puts `test_main()` first under `#[cfg(test)]` and the executor under `#[cfg(not(test))]`.

---

## 5. Important design decisions

- **Freestanding target.** `#![no_std]`, `#![no_main]`, and a custom target JSON with `panic=abort`, no red zone and soft-float. Without a red zone, interrupt frames can't corrupt leaf-function locals. Disabling SSE avoids having to save SIMD state in interrupts. `core`, `alloc` and `compiler_builtins` are rebuilt from source with `build-std`.
- **Module layout:** every subsystem is a directory module (`src/<name>/mod.rs`), which leaves room for submodules. `gdt` is a submodule of `interrupts`, since its only job is to provide the IST stack for the IDT.
- **Global singletons:** `lazy_static!` plus `spin::Mutex`. `WRITER`, `SERIAL1`, `TSS`, `GDT` and `IDT` all need runtime initialization (raw pointer casts, builder calls) and must be `'static`. A spinlock is used because there are no threads and no OS to block on. `PICS` and `ALLOCATOR` use plain `static`s because their constructors are `const`. `SCANCODE_QUEUE` uses `conquer_once::OnceCell` instead, so that its allocation happens at a chosen point in task context and never inside an interrupt handler.
- **Interrupt safety:** the print functions wrap their lock in `without_interrupts`. Everything runs on one core, and interrupts are the only source of concurrency. The keyboard path avoids locks altogether: the interrupt handler and the task share only lock-free structures (`ArrayQueue`, `AtomicWaker`), so the handler can't block or allocate.
- **Cooperative async multitasking:** tasks are Rust `Future`s polled by `Executor`. A task runs until it returns `Pending`, so a task that never yields would starve all others (there is no preemption). Wakers carry only a `TaskId` into a shared bounded queue, which makes waking cheap and interrupt-safe. Wakers are cached per task so polling doesn't allocate. The executor halts the CPU when idle, using the disable-interrupts / check / `sti; hlt` pattern to avoid lost wakeups. `SimpleExecutor` remains as the earlier, busy-polling design.
- **Physical memory access:** every page table is reached through the bootloader's full physical-memory mapping at a fixed offset, so no recursive mapping or temporary mappings are needed. `OffsetPageTable` relies on this layout.
- **Allocator design:** `Locked<A>` gets around the orphan rule and gives `GlobalAlloc::alloc(&self)` interior mutability. The active allocator uses segregated power-of-two free lists, which give O(1) allocation and free for blocks up to 2 KiB. Free lists are intrusive: their nodes live inside the free memory itself, so they need no extra storage. Blocks are aligned to their size class. Larger requests go to `linked_list_allocator`.
- **IST double-fault stack:** a statically allocated 20 KiB stack in the TSS keeps the kernel from triple-faulting (which would reboot the machine) on stack overflow. `tests/stack_overflow.rs` checks this.
- **Custom test framework:** the built-in `test` crate needs `std`, so tests run as small kernels inside QEMU. Results go over serial, and pass or fail is signalled through the `isa-debug-exit` port.
- **Error handling:** this is mostly a panic-based design. `init_heap` returns a `Result`, and `main.rs` calls `.expect()` on it. Allocators return null on out-of-memory, as `GlobalAlloc` requires. Full queues in the executor panic (`expect`), while a full scancode queue drops input with a warning. Hardware faults print diagnostics and halt.
- **External crates:**

| Crate | Used for |
|---|---|
| `bootloader` 0.9 | BIOS bootloader, `BootInfo` (memory map, physical-memory offset), `entry_point!` |
| `x86_64` | Port I/O, `hlt`/`int3`/`sti`/`enable_and_hlt`, control registers (CR2, CR3), GDT/TSS/IDT structures, paging types and `OffsetPageTable` |
| `volatile` | Volatile reads and writes to VGA memory |
| `spin` | `no_std` mutexes for globals |
| `lazy_static` (`spin_no_std`) | Runtime-initialized statics |
| `uart_16550` | Serial port driver |
| `pic8259` | 8259 PIC remap and EOI |
| `pc-keyboard` | Scancode-to-key decoding (now in the keyboard task) |
| `linked_list_allocator` | Fallback heap inside `FixedSizeBlockAllocator` (and `LockedHeap`, imported but unused) |
| `crossbeam-queue` (`alloc`) | `ArrayQueue`: bounded, lock-free MPMC queue for scancodes and ready task ids |
| `conquer-once` | `OnceCell` (spin variant) for the scancode queue |
| `futures-util` (`alloc`) | `Stream`/`StreamExt` (`.next().await`) and `AtomicWaker` |

- **Unsafe / FFI boundaries:** raw pointer casts to `0xb8000` and to page tables; `static mut STACK`; port I/O; `extern "x86-interrupt"` handler ABI; `Waker::from_raw` in `SimpleExecutor`; `#[unsafe(no_mangle)] extern "C" fn _start` in the tests. The edition-2024 syntax `&raw const` and `unsafe {}` blocks inside `unsafe fn` is used throughout.

---

## 6. Getting started

**Prerequisites** (from the config):
- A Rust **nightly** toolchain, which `rust-toolchain.toml` selects automatically. `build-std` also needs the `rust-src` component **(inferred; not declared in the repo)**.
- The **`bootimage`** tool, because `.cargo/config.toml` sets `runner = "bootimage runner"`. bootimage also needs the `llvm-tools-preview` component **(inferred from bootimage's requirements; not stated in the repo)**.
- **QEMU** (`qemu-system-x86_64`) on `PATH` for `cargo run` and `cargo test`.

**Commands supported by the configuration:**

| Command | Effect |
|---|---|
| `cargo build` | Builds the kernel for `x86_64-rias.json`, the default target. Succeeds with 8 warnings **(verified)**, see §7. |
| `cargo run` | Builds a bootable image and boots it in QEMU (through `bootimage runner`). Type to see keys echoed among the timer dots. |
| `cargo test` | Builds and boots each test kernel in QEMU with the `test-args`. Success is QEMU exit code 33. Passes: lib, bin and all integration tests **(verified)**. |
| `cargo test --lib` | Library unit tests (4 pass, verified). |
| `cargo test --test basic_boot --test heap_allocation --test should_panic --test stack_overflow` | Just the integration tests (all pass, verified). Plain `cargo test` runs these too. `--no-fail-fast` keeps going past a failing target. |

**Configuration files:** `Cargo.toml` (deps, bootimage test settings), `.cargo/config.toml` (target, build-std, runner, build-dir layout), `x86_64-rias.json` (target spec), `rust-toolchain.toml` (nightly). No environment variables are read by the code.

---

## 7. Known gaps and uncertainties

**Verified by building and running (2026-11-05):**
- **`cargo test` passes** (lib 4, bin 0, integration 4 files). The earlier bin-target hang is fixed (see §4.6).
- **Build warnings (8):** unused imports `BumpAllocator`, `LinkedListAllocator`, `LockedHeap` (`src/allocator/mod.rs`); `task::keyboard` self-import and `core::iter::Scan` (`src/task/keyboard.rs`); `Box`, `Rc`, `vec`, `Vec`, `print`, `SimpleExecutor` (`src/main.rs`); and an elided-lifetime lint in `src/allocator/bump.rs:16` (`lock()` should return `MutexGuard<'_, A>`). A `cargo test` build adds one more warning: `Task`, `Executor` and `keyboard` are unused in `src/main.rs` because the executor code is compiled out under `cfg(test)`.

**Confirmed by reading the code:**
- **HEAD doesn't build on its own:** the dependencies `src/task/` needs and the `stack_overflow` path fix are uncommitted (see §2).
- **Dead code left from tutorial steps:** `allocator::Dummy`, `memory::translate_addr`, `memory::create_example_mapping`, `memory::EmptyFrameAllocator` and `task::simple_executor::SimpleExecutor`. They are `pub`, so the compiler doesn't warn about them. The commented-out `example_task()` spawn in `main.rs` refers to a function that no longer exists. The `#[allow(dead_code)]` at the top of `vga_buffer/mod.rs` applies only to the `use volatile::Volatile;` item that follows it, not to the `Color` enum.
- **Executor limitations:** no spawner handle, so tasks can't spawn tasks; the ready queue holds 100 ids, and `TaskWaker::wake_task` panics when it is full. That panic would happen **inside the keyboard interrupt handler**. With one task this can't happen in practice, because `AtomicWaker` hands out each registration only once. There are no task priorities or timers, and a task that never returns `Pending` blocks everything.
- **Keyboard pipeline:** scancodes that arrive before the keyboard task's first poll (between `sti` in `init()` and `executor.run()`) are dropped with `WARNING: scancode queue uninitialized`. The scancode queue holds 100 bytes, and overflow drops input with a warning. `ScancodeStream::new` panics if called twice, so there can be only one keyboard consumer. Input is only echoed; nothing buffers lines or interprets them (no shell yet).
- **Timer handler** prints `.` on every tick, which mixes with the echoed keys. It looks like a debug placeholder.
- **Allocator limitations:** `FixedSizeBlockAllocator` never returns freed small blocks to the fallback heap, so memory stays tied to its size class. `LinkedListAllocator` never merges adjacent free regions. The heap is a fixed 100 KiB and can't grow. The bump and linked-list allocators have no tests (`PLAN.md` Phase 0 says they are tested, which is not the case).
- **Frame allocator:** each `allocate_frame` is O(n) and frames are never freed. There is no deallocation path.
- **Exceptions:** only breakpoint, page fault and double fault have handlers. Any other exception (e.g. `#UD`, `#GP`) finds no IDT entry and escalates to a double fault **(inferred from x86 semantics)**. The page fault handler only reports and halts; there is no demand paging. `translate_addr_inner` panics on huge pages.
- **The double-fault stack** is a plain static array with no guard page, so overflowing it is not detected.
- **`tests/should_panic.rs`** declares `#![test_runner(test_runner)]` but defines no `#[test_case]`s and never calls `test_main()`. The `serial_println!` after `should_fail()` can only run if the assert doesn't panic, and in that case the test name is printed twice before Failed.
- **`tests/basic_boot.rs`** never calls `rias::init()`, so it tests only VGA output before GDT/IDT setup.
- **No tests for `src/task/`**, and the library's test kernel doesn't set up a heap, so `task` can't be unit-tested there as things stand.

**Fixed since the previous version of this document:**
- The possible deadlock in `vga_buffer::test_println_output` (a second `WRITER.lock()` while holding the guard) was fixed in `fdaa258 fix tests`. The test now reads through the existing guard, and it passes.

**Unclear or inferred:**
- **The tracked, empty `qemu-system-x86_64` file** at the repo root has no visible purpose. It may be an accidental creation, for example from a shell redirect.
- **README roadmap** still shows cooperative multitasking as not done (see §2). Treat the code as the source of truth.
- The behavior of allocating before `init_heap` (null pointer, then an allocation-error panic) is inferred from `linked_list_allocator::Heap::empty()` and the semantics of Rust's `alloc`, not observed.
