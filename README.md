# Monster

[![CI](https://github.com/BitIntx/monster-lang/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/BitIntx/monster-lang/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)

Monster is an experimental low-level, ahead-of-time systems programming language.

The current compiler is written in Rust and targets LLVM IR.

`.mnst source -> lexer -> parser -> AST -> semantic analysis -> LLVM IR -> opt-18 (-O2) -> clang-18 -> native binary`

The compiler executable is named `mst`, and Monster source files use the `.mnst` extension.

## Docs

A lightweight static documentation site now lives under [`docs/`](./docs/index.html).

It is designed to work both as:

- a simple in-repo reference
- a future GitHub Pages site
- a stable target for editor help links

## Install

Install from GitHub Releases with one command:

```bash
curl -fsSL https://raw.githubusercontent.com/BitIntx/monster-lang/main/install/install-release.sh | bash
mst --help
```

This installer supports Linux and macOS, and downloads the most recent published release, including prereleases.

Install on Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/BitIntx/monster-lang/main/install/install-release.ps1 | iex
mst --version
```

Install system-wide:

```bash
curl -fsSL https://raw.githubusercontent.com/BitIntx/monster-lang/main/install/install-release.sh | sudo env PREFIX=/usr/local bash
mst --version
```

Pin a specific release:

```bash
curl -fsSL https://raw.githubusercontent.com/BitIntx/monster-lang/main/install/install-release.sh | env MST_VERSION=v0.1.0-rc3 bash
```

Pin a specific release on Windows PowerShell:

```powershell
$env:MST_VERSION = "v0.1.0-rc3"
irm https://raw.githubusercontent.com/BitIntx/monster-lang/main/install/install-release.ps1 | iex
```

The release installer currently supports Linux x86_64, macOS x86_64, macOS arm64, and Windows x86_64.
On Linux and macOS it installs `mst` into `~/.local/bin` by default.
On Windows it installs `mst.exe` into `%LOCALAPPDATA%\Programs\mst\bin` and adds that directory to the user `PATH`.
For `mst build` and `mst run`, you still need `clang-18` or `clang`, and `opt-18` or `opt` on your `PATH`.

Install from source instead:

```bash
./install/install.sh
mst --help
```

Uninstall:

```bash
rm -f ~/.local/bin/mst
sudo rm -f /usr/local/bin/mst
```

Windows:

```powershell
Remove-Item "$env:LOCALAPPDATA\Programs\mst\bin\mst.exe" -Force
```

If you installed from a local source checkout, `./install/uninstall.sh` also works.

## Usage

After installing `mst`, you can use it directly from your terminal:

```bash
mst check exam.mnst
mst emit-llvm exam.mnst
mst build exam.mnst
mst build --debug exam.mnst
mst run exam.mnst
mst run --debug exam.mnst
mst run examples/argv.mnst -- hello
mst run examples/file_io.mnst -- exam.mnst
mst run examples/enum.mnst
mst clean
mst --help
mst --version
```

Generated binaries and intermediate LLVM files are written to `target/mst/`.
`--debug` builds skip LLVM `-O2` optimization and link with `clang -g -O0`.
Use `--` to forward remaining CLI arguments to the compiled Monster program.

## VS Code

The VS Code extension now lives in the separate [`monster-vscode`](https://github.com/BitIntx/monster-vscode) repository.

It currently provides:

- `.mnst` file association
- syntax highlighting
- comment and bracket rules
- starter snippets
- a Monster language icon for themes that do not already define one

To work on the extension locally, clone `monster-vscode`, open it in VS Code, and press `F5`.

## Current Status

Monster is already a working compiler prototype.

Implemented today:

- Lexer with source position tracking
- Parser with operator precedence
- AST for functions, statements, expressions, and types
- Semantic analysis with type checking, mutability checks, and return-path validation
- LLVM IR code generation
- Builtin I/O via LLVM runtime helpers
- CLI commands for `check`, `emit-llvm`, `build`, `run`, and `clean`

Supported language features:

- `import "path/to/file.mnst";`
- `import "path/to/file.mnst" as module;`
- `fn`
- `extern fn`
- `let`
- `let mut`
- `return`
- `return;`
- `main(argc: i32, argv: **u8)` entry arguments
- `enum Name { Variant, ... }`
- `if` / `else`
- `while`
- `break`
- `continue`
- `i32`
- `u8`
- `usize`
- `bool`
- `str`
- `void`
- integer literals
- `true` / `false`
- variable references
- function calls
- qualified module function calls like `math.add(...)`
- assignment statements
- fixed-size arrays: `[T; N]`
- slices: `[T]` and `slice(array)`
- array literals and indexing
- nested array index assignment
- `struct` definitions, literals, and field access
- raw pointers: `*T`, `&expr`, `*ptr`, `ptr[i]`
- arithmetic operators: `+ - * /`
- comparison operators: `== != < <= > >=`
- `sizeof(T)`
- string literals
- builtins: `print_i32`, `print_bool`, `print_str`, `print_ln_i32`, `print_ln_bool`, `print_ln_str`, `read_i32`, `len`
- file I/O builtins: `read_file(path, &len)` and `write_file(path, data, len)`
- string/byte builtins: `strlen`, `memcmp`, `memcpy`, `str_eq`
- explicit casts with `as`

`print_*` writes without a trailing newline, while `print_ln_*` appends one.

## Example

```mnst
fn main() -> i32 {
    print_ln_str("Hello, World!");
    return 0;
}
```

More advanced code is possible now too:

```mnst
struct Pair {
    left: i32,
    right: i32,
}

extern fn abs(value: i32) -> i32;

fn main() -> i32 {
    let pair: Pair = Pair { left: 10, right: 20 };
    let mut values: [i32; 3] = [pair.left, abs(-7), 30];
    values[1] = len(values);
    return values[0] + values[1];
}
```

Monster now also has payload-free enums with C-like variants:

```mnst
enum Color {
    Red,
    Green,
    Blue,
}

fn is_red(color: Color) -> bool {
    return color == Red;
}
```

It also supports `sizeof(T)` as a `usize` expression:

```mnst
struct Pair {
    left: i32,
    right: i32,
}

fn main() -> i32 {
    let bytes: usize = sizeof(Pair);
    return bytes as i32;
}
```

Monster can already express a manual growable vector with libc allocation:

```mnst
struct VecI32 {
    data: *i32,
    len: i32,
    cap: i32,
}

extern fn malloc(size: i32) -> *i32;
extern fn realloc(ptr: *i32, size: i32) -> *i32;
extern fn free(ptr: *i32) -> void;
```

See [`examples/growable_vec_i32.mnst`](./examples/growable_vec_i32.mnst) for a full `VecI32` example that grows with `malloc` / `realloc` / `free`, and [`examples/growable_vec_i32.ll`](./examples/growable_vec_i32.ll) for the raw LLVM IR emitted by the current compiler.

Monster also supports file-based imports plus loop control:

```mnst
import "math.mnst" as math;

fn main() -> i32 {
    let mut i: i32 = 0;
    let mut sum: i32 = 0;

    while i < 10 {
        i = i + 1;

        if i == 4 {
            continue;
        }

        if i > 7 {
            break;
        }

        sum = math.add(sum, i);
    }

    return sum;
}
```

## Build From Source

Requirements:

- Rust
- `clang-18`
- `opt-18`

Build the compiler:

```bash
cargo build
```

Run the compiler directly with Cargo:

```bash
cargo run -- check exam.mnst
cargo run -- emit-llvm exam.mnst
cargo run -- build exam.mnst
cargo run -- build --debug exam.mnst
cargo run -- run exam.mnst
cargo run -- run --debug exam.mnst
cargo run -- clean
cargo run -- --help
cargo run -- --version
```

Or run the built compiler binary:

```bash
./target/debug/mst check exam.mnst
./target/debug/mst emit-llvm exam.mnst
./target/debug/mst build exam.mnst
./target/debug/mst build --debug exam.mnst
./target/debug/mst run exam.mnst
./target/debug/mst run --debug exam.mnst
./target/debug/mst clean
./target/debug/mst --help
./target/debug/mst --version
```

## CI

GitHub Actions runs the compiler on `ubuntu-latest` and checks:

- `cargo build`
- `cargo test`
- LLVM IR verification and `-O2` optimization through `opt-18`
- end-to-end LLVM build and run tests against `exam.mnst`
- an end-to-end growable `VecI32` example using `malloc` / `realloc` / `free`

## Example Program

- [`exam.mnst`](./exam.mnst): a Hello, World! starting point with comments summarizing the rest of the current language surface
- [`examples/argv.mnst`](./examples/argv.mnst): `main(argc, argv)` plus forwarded CLI arguments
- [`examples/enum.mnst`](./examples/enum.mnst): payload-free enums and enum comparison
- [`examples/file_io.mnst`](./examples/file_io.mnst): file reading and writing with `read_file` / `write_file`
- [`examples/growable_vec_i32.mnst`](./examples/growable_vec_i32.mnst): a manual growable vector built with raw pointers and libc allocation
- [`examples/growable_vec_i32.ll`](./examples/growable_vec_i32.ll): the raw LLVM IR generated from the growable `VecI32` example
- [`examples/imports/main.mnst`](./examples/imports/main.mnst): aliased `import`, qualified module calls, and `break` / `continue`
- [`examples/imports/math.mnst`](./examples/imports/math.mnst): imported helper module used by the loop-control example
- [`examples/match.mnst`](./examples/match.mnst): payload enum matching with `Variant => expr` and `Variant(binding) => expr`
- [`examples/string_bytes.mnst`](./examples/string_bytes.mnst): `strlen`, `memcmp`, `memcpy`, and `str_eq` against a copied C string buffer

## Roadmap

Near-term goals:

- payload-carrying enums / tagged unions
- namespaced types and broader module imports beyond function-level aliasing
- more complete libc and memory utilities
- growable vectors beyond the current manual `VecI32` pattern
- `sizeof`-driven allocation patterns for self-hosting data structures
- better diagnostics with source snippets
- stronger semantic analysis and type checking

Long-term direction:

- low-level systems programming
- thin runtime
- predictable performance
- fast compile times and fast execution
- a stronger LLVM pipeline first, then an eventual direct native backend

## License

This project is licensed under the MIT License. See the [LICENSE](./LICENSE) file for details.
