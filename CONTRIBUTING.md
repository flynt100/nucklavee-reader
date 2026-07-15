# Contributing to Nucklavee

Nucklavee is an experimental v0.1 Markdown/HTML transformation and semantic
retrieval project. Public APIs and storage formats may change before v1.0.

## Before opening a change

Use GitHub issues for reproducible bugs and focused proposals. Security
problems must follow `SECURITY.md`, not a public issue. PDF work should follow
the Phase 6 roadmap and its fixture/acceptance criteria rather than an ad hoc
parser addition.

Install a stable Rust toolchain with `rustfmt` and `clippy`, then run:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features --locked
cargo build --release --locked
cargo package --locked
cargo audit
cargo deny check licenses
```

Parser and emitter changes need representative fixtures, semantic round-trip
coverage, deterministic output, and an explanation of intentional
normalization. Public API or persisted-format changes need migration and
compatibility notes.

Keep pull requests focused. Explain the user impact, tradeoffs, tests, and any
remaining limitations. Never commit credentials, local configuration,
databases, vector indexes, or generated build output.

AI-assisted changes are welcome, but contributors remain responsible for
understanding the code, verifying provenance and licenses, running the full
gate, and explaining the behavior in the pull request.
