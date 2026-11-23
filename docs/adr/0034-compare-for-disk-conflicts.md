# ADR 0034: "Compare" for a disk conflict is a line diff computed in the workspace and drawn read-only by each frontend

- **Status:** accepted (2026-11-23, owner: build it now; supersedes the "no Compare" note of ticket 10)
- **Build:** rustode on `feat/conflict-compare` (based on `afe5161`); studied: the source tree of the App (local VSCodium 1.147.0)

## Context

Ticket 10 gave a save that hits a disk conflict two choices, Overwrite and Revert, and left VS Code's third one, "Compare", out because rustode has no diff editor (Gap Analysis row "Compare conflicting changes"). Without it the user must choose before seeing what changed on disk.

VS Code (Code - OSS, studied; reimplemented, never translated, [ADR 0009](0009-mit-and-reimplemented-behavior.md)):

- the stale-save message offers `ResolveSaveConflictAction` first and `SaveModelIgnoreModifiedSinceAction` second, in a sentence that says "compare your version with the file contents or overwrite…" (`src/vs/workbench/contrib/files/browser/editors/textFileSaveErrorHandler.ts:129-132`);
- Compare (`workbench.files.action.resolveConflict`, label "Compare") opens a diff editor on the file's content (a virtual `conflictResolution` document) against the model, titled "{name} (in file) ↔ {name} (in {product}) - Resolve save conflict", and shows a help notification (`:247-269`);
- from that editor the toolbar offers `acceptLocalChanges` ("Use your changes and overwrite file contents") and `revertLocalChanges` ("Discard your changes and revert to file contents"); either saves with `ignoreModifiedSince` or reverts, reopens the original editor and closes the comparison (`src/vs/workbench/contrib/files/browser/fileActions.contribution.ts:199-200`; `textFileSaveErrorHandler.ts:410-448`).

## Decision

1. **The workspace computes the comparison; the frontends draw it** ([ADR 0014](0014-in-process-frontends.md)). `Workspace::conflict_compare(id)` returns a `ConflictCompare`: the title, every line once as `Same`, `Removed` (only in the file on disk) or `Added` (only in the unsaved text) with both line numbers, where each block of changes starts, the counts, and a `notice` for what cannot be shown. `ConflictCompare::side_by_side()` gives the same lines as rows with the file on the left, pairing a removed and an added line.
2. **The command is `workbench.files.action.resolveConflict`, label "File: Compare"**, registered in `COMMANDS`. It checks that the document has a conflict (otherwise `Error::NoConflict`, "f.txt has no conflict to resolve", like the other two) and pushes `Event::CompareConflict`; it changes nothing. The prompt's choices are Compare, Overwrite, Revert, and its sentence is now VS Code's own. From the comparison, Overwrite and Revert are the existing commands, run by the same ids; Close keeps the conflict.
3. **A hand-written Myers line diff** (`crates/workspace/src/compare/myers.rs`, from the 1986 paper: the shortest edit script in `O(N·D)` time and `O(D²)` memory), over line ids. No new dependency. The `similar` crate (Apache-2.0, allowed by `deny.toml`) was considered: it is neither in `Cargo.lock` nor in the offline registry, so using it would need a download and a new ATTRIBUTION row, and rustode needs its own caps anyway. The algorithm is about 100 lines with property tests (random inputs rebuild the new text, the script is never longer than delete-all-insert-all, the paper's example has distance 5).
4. **Limits** (hostile numbers, AGENTS.md §11.7.1), constants in `crates/workspace/src/compare.rs`: a side over 8 MiB (`COMPARE_MAX_BYTES`) or 50,000 lines (`COMPARE_MAX_LINES`), or texts that differ in more than 2,000 added plus removed lines (`COMPARE_MAX_CHANGES`, which bounds the search's memory to about 32 MB, estimated as 2,000² snapshot entries of 8 bytes), are not diffed. The model then has no lines and a notice that names the side, its size and the limit, and says Overwrite and Revert still work; this is not an error. A line over 2,000 chars is matched whole but kept shortened for drawing. These limits were chosen here; they are not measured and are not VS Code's.
5. **Line endings are not compared** (a trailing `\r` is dropped) and neither is a missing final newline. Texts that differ only there show no changed line and say so in the notice.
6. **Frontends.**
   - GUI: side by side, like VS Code, as a read-only overlay over the window; only the rows in view are laid out. Buttons: Previous change, Next change, Overwrite, Revert, Close comparison. Keys: arrows, PageUp, PageDown, Home, End scroll; N and P move between changes; Esc closes. The view holds the keyboard focus so the editor does not type the keys.
   - TUI: unified, one column. A terminal is often 80 columns wide, too narrow for two columns of code; each line shows both line numbers and a `-` or `+` mark. Keys: arrows, PageUp, PageDown, Home, End scroll; `n`, `p` change; `o` Overwrite, `r` Revert; Esc or `q` closes.
   - A line only on one side is marked with `-` or `+` as well as tinted, so meaning never rests on color.

## The guarantee

Opening, scrolling and closing the comparison never write the file or change the document: the conflict stays until Overwrite or Revert runs (`crates/workspace/tests/conflict_compare.rs`, `the_command_asks_the_frontend_to_open_the_comparison_and_resolves_nothing`). A huge input never freezes the UI: it is refused by size before any diff work, and the diff itself is bounded.

## Consequences

- The comparison is a snapshot taken when it opens; if the file changes again while it is open, it stays stale until reopened. The comparison closes when the conflict goes away.
- The diff is by lines; VS Code's diff editor also marks changed words inside a line.
- VS Code's "learn more" notification after opening the comparison is not reimplemented.
- The `enabled` predicate for commands is a later ticket; until then the command refuses outside a conflict with the error above.
