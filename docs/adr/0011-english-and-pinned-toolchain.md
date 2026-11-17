# ADR 0011: Code, comments, commits and documents are in English; the editor uses Rust 2024 on a pinned stable toolchain

- **Status:** accepted (2026-11-16, decided by the owner in the Phase 1 grilling session; applied in `rust-toolchain.toml`, `Cargo.toml:7-8` and `rustfmt.toml`)
- **Build:** rustode at `d6debfb`

## Context

The project's documents switched to English on 2026-11-16, at the owner's request (AGENTS.md, "Language"). The editor adds code, comments and commit messages to that. On the toolchain side, different compiler versions produce different lints and formatting, so hook results stop agreeing with CI.

## Decision

Everything written for the editor is in English: code, comments, commit messages, pull requests and documents. The editor uses Rust edition 2024 on one pinned stable toolchain.

Containment rules:

1. Three files hold the toolchain choice:
   - `rust-toolchain.toml:2` pins `channel = "1.99.1"`, with `rustfmt` and `clippy`;
   - `Cargo.toml:7-8` sets `edition = "2024"` and `rust-version = "1.99"`;
   - `rustfmt.toml` formats as edition 2024.
2. The study's folder and file names keep their Portuguese spelling (AGENTS.md, "Language"). New editor names are English.
3. Quotes from evidence stay verbatim, in whatever language they are.
4. A toolchain change is a commit of its own (type `build`), so that a new lint or format result can be traced to it.

## The guarantee

Every machine with rustup builds with 1.99.1, and a lint or formatting difference never comes from the compiler version.

## Consequences

- Measured on the development machine (2026-11-17): there is no rustup (`command -v rustup` prints nothing), and `rustc --version` prints `rustc 1.99.1 (58cf8f9c3 2026-10-13) (Arch Linux rust 1:1.99.1-4)`. The pin is therefore not enforced there; the system compiler matches it today, and a system update would change it without warning. `CONTRIBUTING.md` already covers coverage without rustup (system `llvm-cov`).
- Hypothesis: CI gets 1.99.1 because `actions-rust-lang/setup-rust-toolchain` is given no version (`.github/workflows/ci.yml:25-28`) and reads `rust-toolchain.toml`. CI has never run (the repository is not pushed, [ADR 0007](0007-own-git-repository.md)), so this is not confirmed.
- A move to a newer edition or toolchain needs a commit that fixes every new lint, not a lowered lint level ([ADR 0021](0021-strict-lints.md)).
