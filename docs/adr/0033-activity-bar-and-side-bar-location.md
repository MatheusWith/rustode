# ADR 0033: Both frontends get an Activity Bar and a configurable side bar, replacing ADR 0017's "no activity bar"

- **Status:** accepted (2026-11-19, decided by the owner in the design-rules grilling session; recorded 2026-11-23). Not implemented yet: tickets 29, 38 and 39 of the design-rules plan (`.scratch/design-rules-adoption/`, a local, unversioned scratch area). It supersedes one choice of [ADR 0017](0017-gui-egui-wgpu.md) (see its addendum); the rest of ADR 0017 stands.
- **Build:** rustode at `8c3c9ab`; studied: the source tree of the App (local VSCodium 1.147.0, `02-package-analysis/source/tree/`)

## Context

[ADR 0017](0017-gui-egui-wgpu.md), "Consequences", records that rustode collapses the sidebar below 390 points "with no activity bar", and the TUI has none either. Views are reached by keys only: Ctrl+Shift+E, Ctrl+Shift+F, Ctrl+Shift+X, and Ctrl+B in the GUI. The TUI has no Ctrl+B. The side bar's state (visible or not, which view) lives in each frontend, which [ADR 0014](0014-in-process-frontends.md) says it should not.

VS Code's defaults, read in the source tree:

- The Activity Bar is shown by default: `workbench.activityBar.location` is a string of `default`, `top`, `bottom` or `hidden`, default `default` (`src/vs/workbench/browser/workbench.contribution.ts:674-687`).
- `workbench.sideBar.location` is `left` or `right`, default `left` (`src/vs/workbench/browser/workbench.contribution.ts:585-590`).
- The side bar is hidden at startup when no folder is open: its default is `workbenchState === WorkbenchState.EMPTY` (`src/vs/workbench/browser/layout.ts:3056`; the key is declared at `:2934`).
- Ctrl+B is `workbench.action.toggleSidebarVisibility` (`src/vs/workbench/browser/actions/layoutActions.ts:288-310`).
- The view containers, by their `order`: Explorer 0 (`src/vs/workbench/contrib/files/browser/explorerViewlet.ts:266`), Search 1 (`src/vs/workbench/contrib/search/browser/search.contribution.ts:59`), Source Control 2 (`src/vs/workbench/contrib/scm/browser/scm.contribution.ts:63`), Run and Debug 3 (`src/vs/workbench/contrib/debug/browser/debug.contribution.ts:454-461`), Extensions 4 (`src/vs/workbench/contrib/extensions/browser/extensions.contribution.ts:114-122`).

## Decision

1. **Both frontends draw an Activity Bar** with four items, in VS Code's order: **Explorer, Search, Run and Debug, Extensions**. In the GUI it is a narrow column of icons; in the TUI, a narrow column of letters. Source Control, Accounts and Manage are left out until a feature backs them: no item without a feature behind it.
2. **Each item runs a command** of the workspace's registry, so keys, the palette and clicks reach the same action: the existing show-view commands for Explorer, Search and Extensions, and a Run and Debug view command that ticket 39 adds.
3. **`workbench.activityBar.location`** (`default`, `top`, `bottom`, `hidden`) and **`workbench.sideBar.location`** (`left`, `right`) are settings of rustode, with VS Code's values and defaults. Where a frontend cannot draw a value (for example `top` or `bottom` in the TUI), it draws the nearest one and the workspace reports that it did, instead of failing; the choice is made in ticket 38.
4. **The side bar is hidden at startup when no folder is open**, and **Ctrl+B toggles it in both frontends**.
5. **The layout state lives in the workspace** (ticket 29), driven by commands: side bar visibility, the active view, the Activity Bar location, the focus order of the parts, and the visibility of search, extensions and problems. Widths in points and columns stay in each frontend.
6. **The Run and Debug view** (call stack, variables, breakpoints) is built on the existing debug session state, in its own pull request (ticket 39).

Containment rules:

1. This ADR replaces only the words "with no activity bar" in [ADR 0017](0017-gui-egui-wgpu.md)'s collapse rule; the 390-point collapse of the side bar, and everything else there, stands. The width bands of the sidebar and the status bar are re-checked when the Activity Bar is drawn (its width is part of the 390 points; to be measured in ticket 38).
2. Behavior studied in VS Code is reimplemented, never translated, and credited in `ATTRIBUTION.md`, in the doc comment of the file and in this ADR ([ADR 0009](0009-mit-and-reimplemented-behavior.md)).
3. The Activity Bar has tests at the workspace API (the commands and the layout state) and at input to frame in each frontend (ratatui's test backend, egui_kittest snapshots), including the side bar hidden without a folder and Ctrl+B ([ADR 0019](0019-tests-at-three-seams.md)).
4. An item is added only with the feature behind it.

## The guarantee

The same Activity Bar and side bar state, from the same commands, in both frontends; no item that does nothing.

## Consequences

- The side bar's state leaves the frontends, which is also what removes the divergence between the TUI and the GUI (the TUI has no Ctrl+B today).
- The Activity Bar takes width from the sidebar breakpoints measured in [ADR 0017](0017-gui-egui-wgpu.md); they are measured again when it is drawn.
- `workbench.sideBar.location` `right` moves the explorer to the other side of the editor groups ([ADR 0031](0031-editor-groups.md)); the layout tests cover it.
- Anything that adds or removes an Activity Bar item is a change of this decision only when it breaks rule 4 above.
