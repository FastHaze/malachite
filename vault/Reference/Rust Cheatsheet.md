---
tags: [reference, rust, code]
---
# Rust Cheatsheet

## Ownership

```rust
fn longest<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.len() >= b.len() { a } else { b }
}
```

## Iterators

```rust
let total: u32 = (1..=10).filter(|n| n % 2 == 0).map(|n| n * n).sum();
let words: Vec<&str> = "one two three".split_whitespace().collect();
```

## Errors

```rust
use anyhow::{Context, Result};

fn read(path: &str) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("reading {path}"))
}
```

## Cargo

```bash
cargo test --lib            # library tests only
cargo run --release         # optimised build
cargo clippy --all-targets  # lints
```

See also [[Git Commands]].
