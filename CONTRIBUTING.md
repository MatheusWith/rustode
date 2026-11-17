# Contributing to rustode

Thanks for helping. rustode holds two kinds of work: the study of the App (the reverse engineering of VSCodium 1.147.0, in `09-analises/` and `docs/`) and the rustode editor (Rust, in `crates/` and `xtask/`). Before you start:

1. **Read [`AGENTS.md`](AGENTS.md).** Two rules outweigh all the others:
   - the folders `01-app/` to `08-testes-do-build/` are read-only evidence (§0.1);
   - every claim cites its evidence or is marked as a hypothesis (§0.3).

   Editor work also follows §11.
2. **Choose what to work on.**
   - For the study, start with [`ROADMAP.md`](ROADMAP.md) and [`docs/lacunas.md`](docs/lacunas.md).
   - For the editor, start with the ROADMAP's "Editor: Phase 1 milestones" and "Gap Analysis against VS Code", and with the ADR of the component you touch ([`docs/adr/`](docs/adr/)).
3. **Record things in the right place.**
   - Findings, including benchmark reports, go in [`09-analises/`](09-analises/).
   - Decisions go in [`docs/adr/`](docs/adr/).
   - Open questions go in the gaps.
   - Behavior studied in VS Code and new dependencies go in [`ATTRIBUTION.md`](ATTRIBUTION.md).

   The templates are in [`docs/modelos/`](docs/modelos/). Write every document in English.

More reading:
- [`AGENTS.md`](AGENTS.md) §5–9 and §11: work sequence, tools, skills, where to record, editor rules;
- [`docs/README.md`](docs/README.md): index;
- [`docs/arquitetura.md`](docs/arquitetura.md);
- [`06-port/PORT-NOTES.md`](06-port/PORT-NOTES.md).

## Editor code: how the work is done

- **Tests first, at the highest seam** ([ADR 0019](docs/adr/0019-tests-at-three-seams.md)). The seams are the workspace API (`crates/workspace/tests/`), input to frame per frontend (snapshots, from M2 on) and the CLI process (`crates/cli/tests/`, `xtask/tests/`). Tests assert on behavior, not internals. They are deterministic and use no network, real home directory or real profile.
- **Frontends go through the workspace** ([ADR 0013](docs/adr/0013-crates-and-workspace-api.md), [ADR 0014](docs/adr/0014-in-process-frontends.md)). The CLI, TUI and GUI depend on no rustode crate other than `rustode-workspace`, and `xtask/tests/frontend_deps.rs` fails otherwise. They hold no business logic.
- **Behavior studied in VS Code is reimplemented, never translated** ([ADR 0009](docs/adr/0009-mit-and-reimplemented-behavior.md)). Study it in the source tree (`02-engenharia-reversa/fonte/arvore/`), then credit it in three places: a row of the `ATTRIBUTION.md` table "Behavior studied and reimplemented by the editor", the doc comment of the file that implements it, and the component's ADR.
- **Dependencies** ([ADR 0010](docs/adr/0010-ecosystem-crates-allowed.md)). Declare them once in `[workspace.dependencies]`. Each one needs a license that `deny.toml` allows and a row in `ATTRIBUTION.md`, "Editor dependencies".
- **Numbers** ([`AGENTS.md`](AGENTS.md) §11.6). A measured number comes with its command, folder, commit, build profile and date. An estimated one says where it came from.
- **Benchmarks against the App** ([`AGENTS.md`](AGENTS.md) §11.5, [ADR 0024](docs/adr/0024-comparison-with-the-app.md)). Run one editor instance at a time, under the memory guard, and always on a verified temporary copy of the App, never in place.

## Editor code: commits, branches and checks

The local hooks and CI enforce the rules in this section. When a hook rejects a commit or push, the fix is described here.

### Setup

```sh
pre-commit install --hook-type pre-commit --hook-type commit-msg --hook-type pre-push
```

Required tools: `cargo-nextest`, `committed`, `cargo-deny`, `cargo-audit`, `cargo-llvm-cov`, `gitleaks`, `semgrep`, `pre-commit`.

Coverage gates: `cargo xtask coverage`. Without rustup (for example Arch's `rust` package), point it at the system LLVM: `LLVM_COV=/usr/bin/llvm-cov LLVM_PROFDATA=/usr/bin/llvm-profdata cargo xtask coverage`.

Hooks are not copied by `git clone`. In a clone without them, run the commands of "What runs when" by hand ([`AGENTS.md`](AGENTS.md) §8).

### Commit messages

Follow [Conventional Commits](https://www.conventionalcommits.org/): `type(scope): subject`.

- **Types:** `feat`, `fix`, `perf`, `refactor`, `test`, `docs`, `build`, `ci`, `chore`, `revert`.
- **Scopes (optional, closed list):** one per crate (`core`, `syntax`, `fs`, `config`, `workspace`, `protocol`, `trace`, `cli`, `tui`, `gui`, `xtask`), plus `bench`, `deps`, `docs`, `ci`, `hooks`.
- **Subject:** imperative, lower case, no trailing period, title line at most 72 characters.
- **Body:** explains why; wrapped at 72 characters.
- **Trailers:** a co-author trailer has the form `Co-Authored-By: Name <email>`.

Check a message before committing: `committed --commit-file <file>` and `cargo xtask commit-msg <file>`.

### Branches

- Never push to `main`; every change reaches `main` through a pull request, squash-merged, so the pull request title becomes the commit and must itself be a valid commit message.
- Branch names start with a commit type and a slash: `feat/…`, `fix/…`, `perf/…`, `refactor/…`, `test/…`, `docs/…`, `build/…`, `ci/…`, `chore/…`, `revert/…`, followed by words of lower-case letters and digits joined by `-` (for example `feat/rope-buffer-2`).

### What runs when

| When | Checks |
|---|---|
| commit | `cargo fmt --check`, `cargo clippy -D warnings`, `cargo nextest` (fast tests), `gitleaks`, `semgrep` (rules in `.semgrep/`, offline, metrics off) |
| commit message | `committed` and `cargo xtask commit-msg` (types, scopes, trailers) |
| push | branch name and target, every commit message in the range, full test suite, `clippy`, `cargo deny`; the micro-benchmark gate at 5% only with `RUSTODE_BENCH_GATE=1` |
| pull request (CI) | all of the above, the pull request title, coverage gates, `cargo audit`, the micro-benchmark gate at 10% against the base commit |

Do not bypass hooks with `--no-verify`: CI runs the same checks and will fail the pull request.

### Micro-benchmark gate

A change may not make a benchmarked core operation slower than on `main`: more than 5% fails the local gate, and more than 10% fails CI ([ADR 0026](docs/adr/0026-micro-benchmark-gate.md)).

- **The benchmarks** are criterion benchmarks in `crates/core/benches/core.rs`, written against the public API of `rustode-core`, with fixed seeds: building a 1 MiB buffer; line and position lookups; inserting and deleting a char at random offsets; typing a word with one cursor and with 64; undoing and redoing 100 typed words, and a word typed at 64 cursors. `cargo nextest` never runs them. To run them alone: `cargo bench -p rustode-core --bench core`.
- **Run the gate before pushing a change that can affect the core:** `cargo xtask bench-gate`. It compares with `main` at 5%; `--base <rev>` picks another base (for example `origin/main` after a fetch), `--threshold <percent>` another limit, and `--rounds <n>` how many times each tree is measured (2 by default).
  - It checks the base out in a git worktree under `<target>/bench-gate/` and builds both trees. Then it measures the base and the working tree (uncommitted changes included) in alternating rounds on your machine, running the built benchmarks directly, so that no compile falls between two measurements.
  - It keeps each benchmark's fastest mean per tree, because other programs can only slow a run down. It prints a table of those means and exits with 1 when a benchmark is slower than the limit, or with 2 when it could not run.
- **In the pre-push hook** the gate is opt-in, because it takes minutes: `RUSTODE_BENCH_GATE=1 git push`. Without the variable the hook passes at once.
- **In CI**, the `bench-gate` job runs on every pull request: `cargo xtask bench-gate --base <base commit> --threshold 10`, with both trees measured on the same runner.
- **Measure on a quiet machine.** The gate compares two runs made minutes apart, so a build or another editor running at the same time shows up as a change. Close them first, as for any benchmark (AGENTS.md §11.5). BENCH_NOISE_PLACEHOLDER
- **When the gate fails**, run it again. If the same benchmark is still slower, the slowdown is real, and the fix belongs in the same pull request. The gate has no override.
- **When a pull request changes what a benchmark measures**, it renames the benchmark. The gate then reports it as new, instead of comparing two different workloads. Reviewers check such renames.

### Exceptions

- **Coverage:** at least 85% of lines on the core crates and 60% on the frontends. An exception is justified in a comment on the pull request. The thresholds are never lowered ([ADR 0020](docs/adr/0020-coverage-gates.md)).
- **Lints:** an `#[allow(...)]` goes on the smallest item that needs it, with its reason on the same line, as in `xtask/src/coverage.rs:73` ([ADR 0021](docs/adr/0021-strict-lints.md)).

## Code review checklist

Every pull request is reviewed against this list. The pull request template ([`.github/pull_request_template.md`](.github/pull_request_template.md)) repeats it word for word, so change both together.

- [ ] The title is a valid Conventional Commit; it becomes the squash commit on `main`
- [ ] Tests came first, at the highest seam: workspace API, frontend snapshot or CLI process (ADR 0019)
- [ ] Tests assert on behavior, not internals; they are deterministic and use no network, real home directory or real profile
- [ ] Frontends depend on no rustode crate but the workspace, and hold no business logic (ADR 0013, ADR 0014)
- [ ] Behavior studied in VS Code is reimplemented, not translated, and credited in `ATTRIBUTION.md`, the file's doc comment and the component's ADR (ADR 0009)
- [ ] New dependencies pass `cargo deny`, have MIT-compatible licenses and are listed in `ATTRIBUTION.md` (ADR 0010)
- [ ] Coverage gates hold; any exception is justified in a comment on this pull request (ADR 0020)
- [ ] Every lint exception is justified next to its `allow` (ADR 0021)
- [ ] Every number is labeled measured (with its command, folder and commit) or estimated (with its source); no regression beyond the micro-benchmark gate (ADR 0026)
- [ ] Benchmarks against the App ran on a verified temporary copy, one editor instance at a time, under the memory guard (AGENTS.md §11.5)
- [ ] Nothing written to folders 01 to 08; no personal data
- [ ] Docs, ADRs and the roadmap's milestones and Gap Analysis are updated where the change affects them

## License

The editor code is declared MIT in `Cargo.toml` ([ADR 0009](docs/adr/0009-mit-and-reimplemented-behavior.md)). The `LICENSE` file and the license of this project's analysis documents are still the owner's decision (see the ROADMAP, "Next steps"). The analyzed material keeps the licenses listed in [`ATTRIBUTION.md`](ATTRIBUTION.md).
