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

Finding [0002](../../09-findings/0002-virtual-wayland-input.md) ran an eframe 0.36.2 stand-in on the virtual display. Input reached it through the same winit → egui-winit → `egui::Event` path that the GUI will use.

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
- The finding's stand-in used glow, not wgpu. The wgpu path has not been tried on the virtual display ([0002](../../09-findings/0002-virtual-wayland-input.md), "Limitations").
- The App's default dark theme is "Dark 2026", not Dark Modern ([`gaps.md`](../gaps.md) G18).
- The tabs, explorer and status bar (`crates/gui/src/tabs.rs`, `explorer.rs`, `status.rs`) make three choices of their own. A quick Ctrl+Tab switches straight to the previously used tab, without VS Code's editor picker; the quick input it would use exists now (`quick_input.rs`), the picker does not yet. A tab with unsaved changes says so in its AccessKit description; VS Code's tab label is the name alone. The explorer tree points screen readers at its selected row as the active descendant, so they report focus on that row.
- The breakpoints, scaling and keyboard (`crates/gui/src/layout.rs`, `focus.rs`, `status.rs`, `editor.rs`) make five choices of their own:
  - VS Code never collapses the sidebar. Its grid keeps the parts at their minimum widths and clips the editor. rustode collapses the sidebar below 390 points, the sidebar's 170 plus the editor's 220, with no activity bar. Asking for the sidebar (Ctrl+B, Ctrl+Shift+E, Ctrl+Shift+X) shows it anyway, at most half the window wide. With VS Code's 400 × 270 minimum window, the collapse only happens where the window manager ignores the minimum. The status bar is hidden when the window is shorter than 127 points.
  - Each editor line is 19 points rounded to whole physical pixels: 24 pixels at 1.25, 29 at 1.5. VS Code keeps 19 CSS pixels and lets Chromium snap them. Lines, selections and carets therefore tile with no seams, which the frame tests check pixel by pixel at 1, 1.25, 1.5 and 2.
  - The status bar items stay labels, not VS Code's buttons, because they run no command yet. The focused item is the bar's AccessKit active descendant, as the explorer's selected row is.
  - Tab traversal is egui's own, in the order widgets are drawn. That order matches VS Code's document order: sidebar, active tab, close button, editor, status bar.
  - Platform scale factors are honored through eframe and winit, and tested through egui_kittest's native pixels-per-point. On a real display, fractional scaling (Wayland `wp_fractional_scale_v1`, X11 `Xft.dpi`) has not been checked, because these tests start no display session.
- The quick input and the find widget (`crates/gui/src/quick_input.rs`, `find.rs`) float on a foreground layer above the workbench and use egui's one-line `TextEdit` for their fields, which containment rule 1 allows: it never holds a document. Their own choices:
  - the quick input takes VS Code's width from the window but is centered over the editor area, not the window;
  - in the AccessKit tree it is an editable combo box that controls a list box, its selected option being the active descendant, so screen readers report focus on that option; the find widget is a dialog whose toggles are buttons with a toggled state, where VS Code uses checkboxes (`src/vs/base/browser/ui/toggle/toggle.ts:166-167`);
  - the find widget covers the editor's first lines, where VS Code adds space above line 1 while it is shown (`editor.find.addExtraSpaceOnTop`, on by default: `src/vs/editor/common/config/editorOptions.ts:1811`, `:1853`), and it has no Find in Selection or Preserve Case, no narrow layouts and no resizing;
  - the go-to-line prompts and the "No results" count with an empty find field follow VS Code, where the TUI words them its own way;
  - its buttons and the quick input's rows take clicks but not focus, and the fields keep Tab, the arrows and Escape, so keyboard users reach every action through its key.
- The minimap (`crates/gui/src/minimap.rs`, data from `crates/workspace/src/minimap.rs`) makes its own choices:
  - it draws the blocks VS Code draws with `editor.minimap.renderCharacters` off, not the glyphs it draws by default, and a full-width char is one column, not two;
  - only `editor.minimap.enabled` is a setting; side, size, scale, `maxColumn` and `showSlider` are VS Code's defaults, fixed, and there is no selection, find-match or diagnostic mark, section header or fade of the slider;
  - `editor.action.toggleMinimap` flips a session override of the setting, which a later edit of the setting replaces, because rustode does not write `settings.json` yet; the command is in the palette and has no default key, as in VS Code;
  - there is no vertical scrollbar, so none of the width is kept for one, and VS Code's rule against the start line flickering between frames is not done;
  - it is not in the AccessKit tree: it repeats what the editor already exposes;
  - the blocks are a mesh cached per document version, language, lines drawn and size, so a frame without a change only moves and paints it, and the slider's 500 ms is one scheduled frame, not a timer loop (`crates/gui/tests/frame/minimap.rs`).

## Addendum (2026-11-23): the activity bar

[ADR 0033](0033-activity-bar-and-side-bar-location.md) replaces one choice recorded above: the words "with no activity bar" in the first bullet of the breakpoints, scaling and keyboard consequence. Both frontends get an Activity Bar (Explorer, Search, Run and Debug, Extensions), configurable by `workbench.activityBar.location` and `workbench.sideBar.location`. The side bar collapse below 390 points and every other rule here still hold; the 390 points are measured again when the Activity Bar is drawn.

