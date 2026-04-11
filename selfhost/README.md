# Monster Self-Hosting Experiments

This directory contains Monster programs that are intended to become early
self-hosted compiler pieces.

Current contents:

- `token.mnst`: token kind enum and token struct used by the prototype lexer
- `lexer.mnst`: Monster-written lexer prototype that scans a source buffer into a `TokenBuffer`
- `main.mnst`: CLI entrypoint that reads a `.mnst` file with `read_file` and runs the lexer prototype

Run it with the current Rust compiler:

```bash
mst run selfhost/main.mnst -- exam.mnst
mst run selfhost/main.mnst -- examples/match.mnst
```

The Rust test suite also builds this self-hosted lexer and checks its full token
kind sequence against the Rust lexer on a broad syntax sample:

```bash
cargo test selfhost_lexer_matches_rust_lexer_kind_sequence
```

This is intentionally not a full replacement for the Rust lexer yet. It is the
first checked-in self-hosting slice: Monster code processing Monster source and
building a small token buffer with token kinds close to the Rust lexer.
