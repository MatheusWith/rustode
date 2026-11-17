# ADR 0009: The editor code is MIT; behavior studied in VS Code is reimplemented, never translated, and credited

- **Status:** accepted (2026-11-16, decided by the owner in the Phase 1 grilling session; applied in `Cargo.toml:9` and in the [`ATTRIBUTION.md`](../../ATTRIBUTION.md) table "Behavior studied and reimplemented by the editor")
- **Build:** rustode at `d6debfb`; studied: the source tree of the App (local VSCodium 1.147.0)

## Context

Code - OSS and VSCodium are MIT ([`ATTRIBUTION.md`](../../ATTRIBUTION.md), "Analyzed software"). The editor is worth building only if it behaves like VS Code where users expect it to: undo grouping, multi-cursor, encodings, configuration. The study holds the exact source tree of the App (`02-engenharia-reversa/fonte/arvore/`), so that behavior can be read line by line. That raises two risks:

- translating the TypeScript line by line would make rustode a port of code it does not own, with that code's structure and bugs;
- using the behavior without credit would hide where it came from.

## Decision

The editor code is released under MIT, the license of Code - OSS and VSCodium. Behavior is studied in the source tree and reimplemented from scratch in Rust. No VS Code code is copied or translated line by line.

Containment rules:

1. The license is declared once, in `[workspace.package]` (`Cargo.toml:9`). Every crate inherits it with `license.workspace = true`.
2. Each studied behavior is credited in three places:
   - a row of the `ATTRIBUTION.md` table "Behavior studied and reimplemented by the editor", with the source-tree path and lines and the rustode file;
   - the doc comment of the rustode file that implements it;
   - the ADR of the component (for the M1 crates, [ADR 0013](0013-crates-and-workspace-api.md)).
3. VS Code's own tests may be read to pin down the expected behavior. rustode's tests restate it at rustode's seams ([ADR 0019](0019-tests-at-three-seams.md)). For example, the undo-grouping tests follow `src/vs/editor/test/browser/controller/cursor.test.ts:5925-6240` (ATTRIBUTION.md).
4. A deliberate difference from the studied behavior is written down where it lives. Two examples:
   - invalid UTF-8 falls back to Windows-1252 instead of replacement characters (`crates/workspace/tests/open_save.rs:100`; VS Code: `src/vs/workbench/services/textfile/common/encoding.ts:88-91`);
   - a keybinding whose `when` clause does not parse is dropped and reported (commit `46e0234`). VS Code keeps it with no condition (`src/vs/workbench/services/keybinding/common/keybindingIO.ts:55-57`, `src/vs/platform/contextkey/common/contextkey.ts:205-233`).
5. "From scratch" applies to VS Code code only. Third-party crates are covered by [ADR 0010](0010-ecosystem-crates-allowed.md).

## The guarantee

Every piece of behavior taken from VS Code can be traced to the source-tree lines it came from, and no rustode file is a translation of a VS Code file.

Check: each row of the ATTRIBUTION table cites a source-tree path that exists (`test -e 02-engenharia-reversa/fonte/arvore/<path>`), and the rustode file it names has a doc comment that cites the same source.

## Consequences

- Measured: 10 behaviors are credited so far, in `core`, `fs`, `config` and `workspace` (the 10 rows of the ATTRIBUTION table at `d6debfb`).
- Measured: rule 2 is not fully applied yet. Two of the files listed in the table cite no source in their doc comments: `crates/fs/src/encoding.rs` and `crates/core/src/selection.rs`. Command: `grep -n -e 'src/vs' -e 'studied' crates/fs/src/encoding.rs crates/core/src/selection.rs` prints nothing. Adding the comments is a code change.
- No `LICENSE` file exists yet. Its copyright line names the holder, so writing it is the owner's decision. The license of the analysis documents is a separate decision, also still the owner's ([`NOTICE`](../../NOTICE); ROADMAP, "Next steps").
- This ADR does not settle whether data, as opposed to behavior, may be reused. One example is a theme's color values ([`lacunas.md`](../lacunas.md) G18).
