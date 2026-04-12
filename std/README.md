# Monster Standard Library

This directory is the first small slice of Monster's standard library.

For now, standard library modules are ordinary `.mnst` files. Imports under
`std/` are resolved through `MST_STD_PATH`, the installed `share/mst/std`
directory, and the compiler checkout's `std/` directory. The goal is to grow
this carefully from real compiler, example, and self-hosting needs instead of
designing a large API up front.

Current modules:

- `vec_i32.mnst`: a concrete growable `VecI32` built with `malloc`, `realloc`,
  `free`, raw pointers, and `defer`-friendly cleanup.
