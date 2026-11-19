# ADR 0016: The TUI is built on ratatui and has every Phase 1 feature except the minimap

- **Status:** accepted (2026-11-16, decided by the owner in the Phase 1 grilling session); implemented in part (M2). Done: the width bands, key-to-frame recording and the snapshot tests (`a8c6252`); tabs and the explorer (`3081db1`); quick open, the command palette and go to line (`b813f94`); Dark Modern with 256-color, 16-color and `NO_COLOR` fallback (`818073e`). Not yet: the mouse, find and replace, and applying the user's keybindings ([`ROADMAP.md`](../../ROADMAP.md), "Editor: Phase 1 milestones")
- **Build:** rustode at `d6debfb` when decided; status checked at `7c2467c`

## Context

A terminal frontend serves terminal users and small windows. It is also the cheapest way to exercise the workspace API before the GUI exists: M2 comes before M3 ([ADR 0018](0018-phase-1-milestones.md)). ratatui is MIT, and it sends only the cells that changed between two frames. Its `Buffer::diff` returns the changed cells (ratatui-core 0.1.2, `src/buffer/buffer.rs:471`, in the local cargo registry).

## Decision

The TUI is built on ratatui and has every Phase 1 feature except the minimap. Its five acceptance criteria:

1. **Width breakpoints:**
   - sidebar, editor and status bar at 120 columns or more;
   - sidebar collapsed from 80 to 119;
   - editor only below 60;
   - a "terminal too small" notice below 40×10.

   Resizing redraws without stale cells.
2. **Key-to-frame p99 under 16 ms,** with cell-diff rendering, recorded through the trace metric (`rustode_workspace::trace::LatencyRecorder`, `crates/trace/src/latency.rs:12`).
3. **VS Code keybindings** wherever the terminal allows them, mouse support, the command palette and the Dark Modern theme.
4. **Color fallback:** `NO_COLOR` is respected, with 256- and 16-color fallback, and no meaning is conveyed by color alone.
5. **Snapshot tests** at each width band, with ratatui's test backend ([ADR 0019](0019-tests-at-three-seams.md), seam 2).

Containment rules:

1. The TUI depends on no rustode crate other than the workspace ([ADR 0013](0013-crates-and-workspace-api.md)).
2. Logic is tested at the workspace seam. TUI tests cover presentation only.
3. Behavior studied in VS Code for the TUI is credited in [`ATTRIBUTION.md`](../../ATTRIBUTION.md) when it is implemented ([ADR 0009](0009-mit-and-reimplemented-behavior.md)). Examples are the default keybindings and the theme colors.

## The guarantee

The TUI renders the same workspace state as the GUI, at every width band, and never uses color as the only signal.

## Consequences

- The decision does not say what 60 to 79 columns shows ([`gaps.md`](../gaps.md) G17). Until the owner decides, the TUI shows what it shows from 80 to 119: the status bar and no sidebar (`crates/tui/src/view.rs:32-68`).
- The App's default dark theme is "Dark 2026", not Dark Modern (`src/vs/workbench/services/themes/common/workbenchThemeService.ts:43`, in the source tree). Whether rustode follows it, and whether the theme's color values may be reused, is open ([`gaps.md`](../gaps.md) G18).
- "Wherever the terminal allows" admits that some VS Code keybindings may not reach the TUI. Each one the TUI cannot offer is listed in the roadmap's Gap Analysis.
