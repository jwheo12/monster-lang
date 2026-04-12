# Monster Standard Library

This directory is the first small slice of Monster's standard library.

For now, standard library modules are ordinary `.mnst` files that are imported
with relative paths from examples and experiments. The goal is to grow this
carefully from real compiler, example, and self-hosting needs instead of
designing a large API up front.

Current modules:

- `vec_i32.mnst`: a concrete growable `VecI32` built with `malloc`, `realloc`,
  `free`, raw pointers, and `defer`-friendly cleanup.
