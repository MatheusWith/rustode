# ADR 0032: rustode adopts filmcraft's never-crash and design rules, enforced by lints and tests

- **Status:** accepted (2026-11-19, decided by the owner in the design-rules grilling session; recorded 2026-11-23). Implemented ticket by ticket (`.scratch/design-rules-adoption/`, a local, unversioned scratch area); `AGENTS.md` §11.7 says, rule by rule, what is applied and what is not yet.
- **Build:** rustode at `8c3c9ab`; reference: the owner's filmcraft project (`AGENTS.md` §0, `docs/architecture.md` §1–3, read only)

## Context

The owner added design rules to `AGENTS.md` §11.7 on 2026-11-17, taken from filmcraft. The audit of 2026-11-19 (measured at `753a255` and `ff00434`, re-checked on 2026-11-23 where this ADR says so) found that the adaptation had lost parts of the rules and that most were not enforced by anything:

- **Lints.** Only `todo` and `unwrap_used` are warned (`Cargo.toml:83-84`), which `-D warnings` turns into errors. `unreachable!` is used in production code at four sites, measured on 2026-11-23 with `grep -rn "unreachable!" crates xtask --include=*.rs` outside `tests/`: `crates/workspace/src/extensions.rs:465` and `crates/workspace/src/extensions/manifest.rs:400`, `:428`, `:432`. `expect`, `panic!`, `unimplemented!`, indexing and string slicing are not linted. The audit counted 228 indexing and slicing sites at `ff00434`; that count was not repeated at `8c3c9ab`, and ticket 11 to 13 of the plan re-measure it.
- **Workers.** A worker that panics is not reported. `Extensions::queue` starts a new worker only when the next job cannot be sent (`crates/workspace/src/extensions.rs:486-498`), and the job that was running when it died stays "busy". The audit counted ten such threads and jobs; the figure is the owner's plan's and is re-measured by ticket 14 to 16.
- **No last-resort guard.** `Workspace::execute` and `execute_with` (`crates/workspace/src/commands.rs:679`, `:755`) and the frontends' frames do not run under `catch_unwind`, and nothing writes a crash log.
- **Hostile input is untested.** No parser is run on mutated bytes. The audit found that a deeply nested `when` clause overflows the stack, which aborts the process; this is a finding of that audit, not re-run here.
- **Saving is not fully atomic.** `crates/fs/src/io.rs:20-36` writes a temporary sibling with a fixed name (`.<name>.rustode-save`), does not fsync the file or the folder, and leaves the temporary file when `write` or `set_permissions` fails, because only the rename's error removes it. A second copy of the logic exists for extension state (`crates/workspace/src/extensions/storage.rs:64-76`, which also does not fsync).
- **Errors.** Messages carry internal details, some errors are plain `String`s, some are swallowed, and a disk conflict is settled silently in the TUI.

filmcraft solves the same problems and its rules are tested in a larger codebase. All paths in this ADR that name filmcraft files are relative to `~/Projects/filmcraft/` (read only, 2026-11-23): `AGENTS.md:6-47` ("0. Never crash", rules 1 to 9), the layer check in `docs/architecture.md:86-103`, and, for saving, `docs/architecture.md:162-163` and `crates/format/src/atomic.rs:26-55`.

## Decision

**filmcraft is the reference.** Its rules come into rustode, adapted for an editor, and are enforced by lints and tests rather than by prose. This ADR adopts these, each with its filmcraft source:

| Rule here | filmcraft source (in `~/Projects/filmcraft/`) | What rustode adds or changes |
|---|---|---|
| **F0.2** No panicking shortcut outside tests: `unwrap`, `expect`, `panic!`, `unreachable!`, `todo!`, `unimplemented!` | `AGENTS.md:17-21` | The lost clause is restored: a fallback is allowed only when it is truly correct, and **never silently corrupts a document, a settings file or extension state**. An unfinished feature returns an "unsupported" error. The four `unreachable!` become `Result`s |
| **F0.4** Every number from input is hostile | `AGENTS.md:28-33` | Adds: slice strings only at char boundaries (`char_indices`, `is_char_boundary`), no `clamp` with bounds that can cross, and `string_slice` and `indexing_slicing` denied in production. No `#[allow]` for the existing sites: they become `get()` or `char_indices` |
| **F0.6** Every thread or job runs under `catch_unwind` and reports its failure as an error | `AGENTS.md:36-38` | A new L1 crate, `rustode-guard`, owns it: `spawn_worker(name, job, on_failure)` and supervised child processes. A channel poll tells "disconnected" from "empty" and treats disconnection as a failure, so a dead worker never leaves a language server "running", an extension "busy" or a search "running" |
| **F0.7** Last-resort guard | `AGENTS.md:39-41`, `crates/ui-egui/src/crash.rs:1-40` | A panic hook writes a crash log (message, `file:line`, thread, backtrace) and keeps the last summary; a command guard around `Workspace::execute` and `execute_with`; a frame guard in each frontend that shows an error window; `panic = "unwind"` kept |
| **F0.8** Prove it | `AGENTS.md:42-44` | Mutation tests with `proptest` as a dev-dependency (never of a frontend): fixed seeds, no regression files on disk, run under `catch_unwind`. **Level 1:** bytes that come from disk, the network or a child process (settings, keybindings, `launch.json`, VSIX packages and manifests, LSP and DAP messages, decoded files). **Level 2:** what the user types (globs, regexes, find and replace text, quick open queries, command arguments, CLI arguments, benchmark scenarios) gets hostile-parameter tests. A failing seed becomes a named regression test with its input inlined |
| **F0.9** Enforced | `AGENTS.md:45-47` | `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unreachable, clippy::unimplemented, clippy::todo))]` and `clippy::string_slice` and `clippy::indexing_slicing` at **every crate root**, test doubles included, and an xtask test fails when a crate root lacks the attribute. `cfg_attr(not(test), …)` leaves integration tests and their helpers alone. `arithmetic_side_effects` stays off (the audit counted 609 hits at `ff00434`, mostly bounded counters) |
| **A3.9** Atomic save | `docs/architecture.md:162-163`, `crates/format/src/atomic.rs:26-55` ("A3.9" is the owner's label from the grilling session) | One helper in `rustode-fs`: a temporary sibling created exclusively with a unique name, written, `sync_all`, permissions kept, renamed over the target, the **folder fsynced on Unix**, the temporary file removed on any error, and a symlinked target written through to the file it points to. Extension state uses the same helper. Whether VS Code writes through a symlink is **to be confirmed in the source tree** (ADR 0009) before ticket 22 builds it |
| **Error messages** | `AGENTS.md:14-16` (rule 1: context is added, never discarded) | A message says **what happened, to which item, and what to do** when something can be done. Context is added at the layer that knows it. No `{:?}`, no Rust type names (`DocumentId(0)`) and no path-less I/O errors in a message. A frontend never swallows the error of a user action. Errors are typed with `Display`, and a semgrep rule forbids `{:?}` of errors in frontend code |

Rules that filmcraft has and this ADR does not adopt: the project validator that runs after every edit, snapshot-based undo, the wasm check, and its video-specific time base and data model. `AGENTS.md` §11.7 already carries rustode's own versions of filmcraft's rules 5 (bounded recursion), 6 (no lock cascades), the layers (`docs/architecture.md:86-103`, ticket 04) and exact positions.

Containment rules:

1. **`AGENTS.md` §11.7 is the statement of these rules**, with a citation for everything it says is applied and "Not applied yet (ticket NN of the design-rules plan)" for everything that is not. A rule moves from the second to the first only in the change that applies it.
2. **A new rule is applied from the pull request that introduces it.** Code that lands on `main` during the plan follows the new rules from that point; later tickets include it in their scope.
3. **No exception by `#[allow]` for F0.2 and F0.4 in production code.** The one allowed case is F0.2's: a call that is provably infallible from local code alone, under a single-item `#[allow(...)]` with its reason on the same line (ADR 0021).
4. **Coverage gates stay at 85% and 60%** and are never lowered (ADR 0020). The new L1 crates are core crates for this purpose.
5. **Phase 1 keeps `panic = "unwind"`.** No profile may set `abort`: the guards depend on it.

## The guarantee

An escaped panic, a dead worker or a hostile byte never ends the session, never leaves a state stuck as "running", and never loses or silently corrupts a document, a settings file or extension state. A rule says "applied" only when a lint, a test or a hook enforces it, and says "not applied yet" while it does not.

## Consequences

- **A lot of mechanical work.** Denying indexing and string slicing touches every crate. The plan does it in batches (tickets 11 to 13), crate by crate, so each pull request stays reviewable.
- **A new crate and two more.** `rustode-guard` (L1) is new. The plan also moves HTTP to `rustode-net` (L1) and the test stand-in and the benchmark harness to tools crates (`test-registry`, `rustode-bench`); the layer table in §11.7.5 lists them already, and a layer check (ticket 04) will keep the layers true.
- **`proptest` is a new dev-dependency.** It needs a row in `ATTRIBUTION.md` with kind `dev` and a license `deny.toml` allows (ADR 0010); the dependency check of ticket 03 fails without the row.
- **Documents that cite lines drift.** The citations in §11.7 were re-measured at `8c3c9ab` (2026-11-23) and will drift again; the rule "re-measure and cite the commit" (§0.3, §11.6) applies each time a ticket touches them.
- **The owner's `AGENTS.md` §11.7 of 2026-11-17 is replaced** by the version in the same pull request as this ADR. Its examples that were wrong (the lint lines and the registry lines) are corrected there.
- Anything that changes which of filmcraft's rules rustode follows needs a new decision, not an extension of this one.
