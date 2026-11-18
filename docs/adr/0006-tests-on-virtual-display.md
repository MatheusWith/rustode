# ADR 0006: UI tests run on a virtual display

- **Status:** proposed (in use since 2026-11-16; not explicitly approved by the owner)
- **Build:** local VSCodium 1.147.0

## Context

The owner uses the computer while the tests run and doesn't want the keyboard or focus taken over. They interrupted a long run that used `ydotool` during the 2026-11-13 QA. Test app windows opened in the real session can steal focus. Also, generating the compile cache at the end of the build opens the app, which fails without a display.

## Decision

UI tests and the final build step run inside `kwin_wayland --virtual` with their own D-Bus (`dbus-run-session`), through `~/Projects/vscode/qa/suite/tools/run_virtual.sh`.

Containment rules:

1. The virtual compositor has its own D-Bus bus and doesn't talk to the real session's KWin.
2. Native input is still forbidden without authorization ([`AGENTS.md`](../../AGENTS.md) §3.2); on the virtual display the app is driven only over CDP.
3. Timing comparisons use the reference run on the same virtual display.

## The guarantee

No test window shows up on the owner's desktop, and the timing numbers are comparable with each other.

## Consequences

- The virtual display has no GPU, so rendering is in software and the absolute times don't hold for the real session.
- Native captures (About, G5) still require the real session and authorization.
