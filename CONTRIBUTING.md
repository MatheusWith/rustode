# Contributing to rustode

Thanks for helping. Before you start:

1. **Read [`AGENTS.md`](AGENTS.md).** Two rules outweigh all the others:
   - the folders `01-app/` to `08-testes-do-build/` are read-only evidence (§0.1);
   - every claim cites its evidence or is marked as a hypothesis (§0.3).
2. **Start with [`ROADMAP.md`](ROADMAP.md) and [`docs/lacunas.md`](docs/lacunas.md)** to choose what to investigate.
3. **Record things in the right place.**
   - Findings go in [`09-analises/`](09-analises/).
   - Decisions go in [`docs/adr/`](docs/adr/).
   - Open questions go in the gaps.

   The templates are in [`docs/modelos/`](docs/modelos/). Write every document in English.

More reading:
- [`AGENTS.md`](AGENTS.md) §5–9: work sequence, tools, skills, where to record;
- [`docs/README.md`](docs/README.md): index;
- [`docs/arquitetura.md`](docs/arquitetura.md);
- [`06-port/PORT-NOTES.md`](06-port/PORT-NOTES.md).

## Editor code: commits, branches and checks

The local hooks and CI enforce the rules in this section. When a hook rejects a commit or push, the fix is described here.

### Setup

```sh
pre-commit install --hook-type pre-commit --hook-type commit-msg --hook-type pre-push
```

Required tools: `cargo-nextest`, `committed`, `cargo-deny`, `cargo-audit`, `cargo-llvm-cov`, `gitleaks`, `semgrep`, `pre-commit`.

Coverage gates: `cargo xtask coverage`. Without rustup (for example Arch's `rust` package), point it at the system LLVM: `LLVM_COV=/usr/bin/llvm-cov LLVM_PROFDATA=/usr/bin/llvm-profdata cargo xtask coverage`.

### Commit messages

Follow [Conventional Commits](https://www.conventionalcommits.org/): `type(scope): subject`.

- **Types:** `feat`, `fix`, `perf`, `refactor`, `test`, `docs`, `build`, `ci`, `chore`, `revert`.
- **Scopes (optional, closed list):** one per crate (`core`, `syntax`, `fs`, `config`, `workspace`, `protocol`, `trace`, `cli`, `tui`, `gui`, `xtask`), plus `bench`, `deps`, `docs`, `ci`, `hooks`.
- **Subject:** imperative, lower case, no trailing period, title line at most 72 characters.
- **Body:** explains why; wrapped at 72 characters.
- **Trailers:** a co-author trailer has the form `Co-Authored-By: Name <email>`.

### Branches

- Never push to `main`; every change reaches `main` through a pull request, squash-merged, so the pull request title becomes the commit and must itself be a valid commit message.
- Branch names start with a commit type and a slash: `feat/…`, `fix/…`, `perf/…`, `refactor/…`, `test/…`, `docs/…`, `build/…`, `ci/…`, `chore/…`, `revert/…`, followed by lower-case words joined by `-`.

### What runs when

| When | Checks |
|---|---|
| commit | `cargo fmt --check`, `cargo clippy -D warnings`, `cargo nextest` (fast tests), `gitleaks`, `semgrep` (rules in `.semgrep/`, offline, metrics off) |
| commit message | `committed` and `cargo xtask commit-msg` (types, scopes, trailers) |
| push | branch name and target, every commit message in the range, full test suite, `clippy`, `cargo deny` |
| pull request (CI) | all of the above, the pull request title, coverage gates, `cargo audit` |

Do not bypass hooks with `--no-verify`: CI runs the same checks and will fail the pull request.

## License

The license of this project's analysis documents is still the owner's decision (see the ROADMAP, "Next steps"). The analyzed material keeps the licenses listed in [`ATTRIBUTION.md`](ATTRIBUTION.md).
