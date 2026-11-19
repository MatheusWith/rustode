# ADR 0028: Keystrokes may be injected on the virtual display through the virtual compositor's own input paths (amends ADR 0006 rule 2 and AGENTS.md §3.2)

- **Status:** proposed — owner's decision (2026-11-17, from the "Proposed amendment" of [`09-findings/0002`](../../09-findings/0002-virtual-wayland-input.md)). [ADR 0006](0006-tests-on-virtual-display.md) and [`AGENTS.md`](../../AGENTS.md) §3.2 are unchanged until the owner accepts it.
- **Build:** the App: local VSCodium 1.147.0; KWin 6.8.2 (`kwin_wayland --virtual`)

## Context

Two rules forbid keystroke injection today:

- ADR 0006, containment rule 2: "Native input is still forbidden without authorization (AGENTS.md §3.2); on the virtual display the app is driven only over CDP."
- AGENTS.md §3.2: "Native input (e.g. `ydotool`) needs the owner's explicit authorization, with the duration announced beforehand."

The typing-latency method the owner chose ([ADR 0025](0025-typing-latency-injected-input.md)) injects keystrokes. The Phase 1 decisions require these rules to be amended first, and the amendment is the owner's decision.

Finding 0002 measured:

- two input paths work on the virtual compositor without any prompt: libei through `org.kde.KWin.EIS.RemoteDesktop.connectToEIS`, and `org_kde_kwin_fake_input` v6;
- the real desktop session advertises the same global and the same D-Bus method. An injector that connected to the wrong socket or bus would type into the owner's desktop;
- the finding's tools refuse to run outside the virtual session: `refusing: WAYLAND_DISPLAY="wayland-1" is not a qa-virt-* socket` (exit 2), and the same refusal from `eistype`.

## Decision (proposed)

1. **ADR 0006, containment rule 2**, is replaced by:

   > 2. Native input stays forbidden in the real session without authorization (AGENTS.md §3.2). On the virtual display, keystrokes may be injected only through the virtual compositor's own input paths: libei with the fd from `org.kde.KWin.EIS.RemoteDesktop.connectToEIS` on the session's private D-Bus (preferred), or `org_kde_kwin_fake_input` on its `qa-virt-*` socket. The injecting client must run inside the `run_virtual.sh` session and refuse to start unless `WAYLAND_DISPLAY` starts with `qa-virt-` and the session bus is not the real one (`/run/user/<uid>/bus`). Tools that write to `/dev/uinput` (`ydotool`, `dotool`) or connect to the real session's socket or bus remain forbidden, because they reach the owner's desktop. Everything else on the virtual display is still driven over CDP.

2. **ADR 0006, "Consequences"**, gains: "Injected keystrokes are counted against what was received before any timing is used (finding 0002 saw one lost first key)."

3. **AGENTS.md §3.2**: the sentence "Native input (e.g. `ydotool`) needs the owner's explicit authorization, with the duration announced beforehand." is replaced by:

   > Native input in the real session (e.g. `ydotool`, anything using `/dev/uinput`) needs the owner's explicit authorization, with the duration announced beforehand. Injection into the virtual display through libei or `org_kde_kwin_fake_input`, under the guards of ADR 0006 rule 2, does not need it.

Containment rules:

1. Nothing changes until the owner accepts this ADR. When the owner does, ADR 0006 gets a dated addendum that points here and carries the new rule 2 ([`README.md`](README.md)), and AGENTS.md §3.2 gets the new text, in the same commit.
2. Every injector implements the guards, as the finding's tools do: `09-findings/0002-attachments/tools/fakeinput/src/main.rs` and `09-findings/0002-attachments/tools/eistype/eistype.c`.
3. The scope is keyboard input on the virtual display. Pointer input, or anything in the real session, needs a new decision.

## The guarantee

No injected event can reach the owner's desktop, because the injector refuses to start outside the virtual session (measured in finding 0002 from the real session: exit 2, nothing injected).

## Consequences

- If accepted: the typing-latency scenarios of ADR 0025 can run.
- If rejected: typing latency through the virtual compositor is not measured, and ADR 0025 needs a new decision.
- A related but separate decision, which is not part of this ADR: finding 0002 also proposes setting `NODE_DISABLE_COMPILE_CACHE=1` whenever the App is launched from `01-app/` (the `launch-built-app` skill and the QA launchers), and an integrity check that also lists files missing from the manifest ([`gaps.md`](../gaps.md) G13). Editor benchmarks avoid the problem by running a verified temporary copy (AGENTS.md §11.5).
