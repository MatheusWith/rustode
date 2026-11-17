# ADR 0017: The GUI is egui on eframe with wgpu; it repaints only on change and draws text with its own virtualized widget

- **Status:** accepted (2026-11-16, decided by the owner in the Phase 1 grilling session); not implemented yet (M3; `crates/gui/src/lib.rs` is a one-line placeholder)
- **Build:** rustode at `d6debfb`; egui 0.36.2 as read in the local cargo registry

## Context

The App draws its whole UI in a Chromium renderer (README, "Architecture of the analyzed app"). A native GUI can render with the GPU directly and stay near zero CPU when idle, provided it repaints only when input arrives or state changes.

egui's built-in `TextEdit` does not fit a code editor's files. In egui 0.36.2 (`src/widgets/text_edit/builder.rs`), every frame it:

- copies the whole text: `let prev_text = text.as_str().to_owned();` (`:485`);
- builds its layout job from another full copy, through `mask_if_password`, which returns `text.to_owned()` (`:501-504`, `:1034-1038`);
- lays out the whole text (`:658`, `:678`).

For a 1 GB file, that is at least 2 GB copied per frame (estimated from the code: two full copies per frame).

Finding [0002](../../09-analises/0002-entrada-virtual-wayland.md) ran an eframe 0.36.2 stand-in on the virtual display. Input reached it through the same winit → egui-winit → `egui::Event` path that the GUI will use.

## Decision

The GUI is egui on eframe with the wgpu renderer and AccessKit on, with every Phase 1 feature, including the minimap. Its six acceptance criteria:

1. VS Code's width breakpoints for the sidebar and the status bar;
2. input-to-photon p99 under 16 ms at 60 Hz;
3. near-zero idle CPU and GPU;
4. HiDPI and fractional scaling;
5. full keyboard navigation, and AccessKit exposing the UI to Orca;
6. snapshot tests with egui_kittest, including the AccessKit tree ([ADR 0019](0019-tests-at-three-seams.md), seam 2).

Containment rules:

1. The editor area is a custom virtualized widget that asks the workspace only for the visible lines. egui's `TextEdit` is never used for documents.
2. A repaint is requested only on input or on a workspace event (reactive repaint).
3. The GUI depends on no rustode crate other than the workspace ([ADR 0013](0013-crates-and-workspace-api.md)). Logic is tested at the workspace seam.
4. Behavior studied in VS Code for the GUI is credited in [`ATTRIBUTION.md`](../../ATTRIBUTION.md) when it is implemented ([ADR 0009](0009-mit-and-reimplemented-behavior.md)). Examples are the breakpoints, keybindings, theme and minimap.
5. Moving to a GUI toolkit other than egui needs a new decision.

## The guarantee

A frame costs the GUI only the visible lines, whatever the file size, and an idle window does not repaint.

## Consequences

- The custom widget means writing text layout, selection drawing, input-method support and accessibility for the editor area. It is the largest single piece of M3 (estimated from the scope).
- The finding's stand-in used glow, not wgpu. The wgpu path has not been tried on the virtual display ([0002](../../09-analises/0002-entrada-virtual-wayland.md), "Limitations").
- The App's default dark theme is "Dark 2026", not Dark Modern ([`lacunas.md`](../lacunas.md) G18).
- The tabs, explorer and status bar (`crates/gui/src/tabs.rs`, `explorer.rs`, `status.rs`) make three choices of their own. A quick Ctrl+Tab switches straight to the previously used tab, without VS Code's editor picker, which needs quick open (ticket 24). A tab with unsaved changes says so in its AccessKit description; VS Code's tab label is the name alone. The explorer tree points screen readers at its selected row as the active descendant, so they report focus on that row.
