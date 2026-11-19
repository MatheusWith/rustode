# ADR 0025: Typing latency is measured with keystrokes injected into both editors through the virtual compositor

- **Status:** accepted (2026-11-16, the owner's choice of option a in the Phase 1 grilling session; feasibility measured on 2026-11-17 in [`09-findings/0002`](../../09-findings/0002-virtual-wayland-input.md)); not applied until [ADR 0028](0028-virtual-display-input.md) is accepted
- **Build:** rustode at `d6debfb`; the App: local VSCodium 1.147.0; KWin 6.8.2 (`kwin_wayland --virtual`)

## Context

Typing latency is one of the metrics the owner cares about most. CDP can type into the App but not into rustode, and CDP input skips the Wayland path that a real keystroke takes.

Finding 0002 measured the following on the virtual compositor:

- it accepts keystrokes through `org_kde_kwin_fake_input` v6, and through libei with the EIS file descriptor from `org.kde.KWin.EIS.RemoteDesktop.connectToEIS` on the session's private D-Bus;
- it does not advertise `zwp_virtual_keyboard_v1`;
- both paths reached the App (trusted DOM `keydown`, read over CDP) and an eframe stand-in for the rustode GUI;
- 143 of 144 injected characters arrived. The one loss was the App's first key through `fake_input`. libei lost nothing and had the smaller spread.

## Decision

Both editors receive keystrokes through the same Wayland virtual-input path, on the virtual display. A measurement ends at the next presented frame: the wgpu present on the rustode side, and a CDP frame trace event on the App side.

Containment rules:

1. libei is the path, and `org_kde_kwin_fake_input` is the fallback (finding 0002, "Injection recipe").
2. The injector runs inside the `run_virtual.sh` session. It refuses to start unless `WAYLAND_DISPLAY` starts with `qa-virt-` and the session bus is not under `/run/user/`. This matters because the real session advertises the same global and the same D-Bus method (finding 0002, "Evidence").
3. Before any timing is used, the number of keys received must equal the number sent.
4. On the App, CDP is used only for setup and for reading results back, never to deliver the measured keys.
5. rustode records input-to-frame with `LatencyRecorder::record_between(input, presented)`, re-exported as `rustode_workspace::trace` (`crates/trace/src/latency.rs:44`). Both ends of the measurement must be the same kind of event: a frame presented.
6. No keystroke is injected for a benchmark until ADR 0028 is accepted. Until then, AGENTS.md §3.2 and ADR 0006 rule 2 apply as written.

## The guarantee

Both editors get the same keystrokes through the same path, and no injected keystroke can reach the owner's desktop.

## Consequences

- The send-to-receive deltas in finding 0002 are not a latency measurement. The two sides use different clocks and different end points.
- Open questions: which App-side event marks the frame that shows the key ([`gaps.md`](../gaps.md) G15), and why the first key was lost once (G16).
- The wgpu present path has not been tried on the virtual display; the stand-in used glow ([ADR 0017](0017-gui-egui-wgpu.md)).
- If ADR 0028 is rejected, typing latency is measured on neither editor through this path, and this ADR needs a new decision.
