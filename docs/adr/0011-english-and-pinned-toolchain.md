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

## Addendum (2026-11-18): the study's names are in English too

At the owner's request, containment rule 2 changed: every folder and file name the project writes is now English, including the study's (`09-findings/`, `docs/templates/`, `docs/gaps.md`, `assemble.sh`, the ADR and finding slugs). The evidence folders and the files `assemble.sh` generates in them were renamed too (`02-package-analysis/`, `03-documents/`, `04-screenshots/`, `07-vscodium-source/`, `08-build-tests/`; `manifest-sha256.txt`, `source/tree/`, `out-formatted/`, …), by the owner's explicit decision; the copies were moved in place and checked against the manifest, not edited. Only names copied verbatim from the live sources stay in Portuguese (AGENTS.md, "Language").

## Addendum (2026-11-19): rustup is now installed, and the pin is enforced

The 2026-11-17 measurement above is superseded. Measured today: `pacman -Qo /usr/bin/cargo /usr/bin/rustc` shows both owned by `rustup 1.29.3-1` (symlinks to `/usr/bin/rustup`), and `rustup show` reports the active toolchain as `1.99.1-x86_64-unknown-linux-gnu`, "active because: overridden by `/home/with/Projects/rustode/rust-toolchain.toml`". The guarantee in this ADR — every rustup machine builds with 1.99.1 — now holds on the development machine too, not only as a hypothesis about CI.

While adding the `llvm-tools-preview` component needed for `cargo xtask coverage` (`rustup component add llvm-tools-preview`; CONTRIBUTING.md:72, "Setup"), a stale partial download left in `~/.rustup/downloads/*.partial` by an earlier interrupted attempt made `rustup` report `cargo` and `clippy` as already installed while their binaries were missing from `~/.rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu/bin/`, breaking `cargo` entirely (`error: the 'cargo' binary, normally provided by the 'cargo' component, is not applicable to the '1.99.1-x86_64-unknown-linux-gnu' toolchain`). Fixed by removing the partial download and the stale update-hash, then reinstalling the toolchain with every component the project needs locally:

```sh
rm -f ~/.rustup/downloads/*.partial ~/.rustup/update-hashes/1.99.1-x86_64-unknown-linux-gnu
rustup toolchain uninstall 1.99.1-x86_64-unknown-linux-gnu
rustup toolchain install 1.99.1 --profile minimal --component rustfmt --component clippy --component llvm-tools-preview
```

Confirmed restored: `cargo 1.99.1 (5f94df478 2026-08-27)` and `clippy 0.1.99 (58cf8f9c35 2026-10-13)`.

`rust-toolchain.toml`'s component list (`rustfmt`, `clippy`) is left unchanged: `llvm-tools-preview` is added once, locally, the same way CI adds it only to the `coverage` job (`.github/workflows/ci.yml:44-46`) rather than to every job.

**Coverage, measured.** `cargo xtask coverage`, run from the project root (worktree `rustode-wt/docs`, branch `docs/design-rules`) on 2026-11-23 at `709a424` (the working tree differed from the commit only in Markdown files), debug build, with `CARGO_TARGET_DIR` pointing at the shared target folder. It exited 0 after 1 min 56 s and printed `core: 94.3% of 16486 lines (minimum 85%)` and `frontends: 91.0% of 14345 lines (minimum 60%)`: both gates pass. This is one run, so no variation is known. The earlier failure ("failed to find llvm-tools-preview") is gone.
