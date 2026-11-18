# 0002: Can keystrokes be injected into rustode and the App on the virtual display?

> **Date:** 2026-11-17 · **Confidence:** measured (6 sessions; delivery timings estimated) · **Gap:** none (opens two candidate gaps, see Next steps) · **Components:** `kwin_wayland --virtual`, App renderer, stand-in for the rustode GUI

## Question

The Phase 1 benchmark wants both editors to receive keystrokes through the same Wayland virtual-input path on the virtual display ([ADR 0006](../docs/adr/0006-tests-on-virtual-display.md)), so typing latency is measured on equal terms. Does KWin's virtual compositor (`kwin_wayland --virtual`, started by `run_virtual.sh`) accept a virtual-keyboard or fake-input client, and does the injected input reach each app?

## Short answer

Yes, through two paths. `zwp_virtual_keyboard_v1` is not available, because KWin 6.8.2 does not advertise it. The two paths that do work are:

- **`org_kde_kwin_fake_input` v6**, a Wayland global on the virtual compositor's socket;
- **libei**, with the EIS socket obtained from `org.kde.KWin.EIS.RemoteDesktop.connectToEIS` on the virtual session's private D-Bus.

Both paths typed the test text into the App (Electron on native Wayland, read back over CDP, keydown `isTrusted=true`) and into an eframe/egui stand-in for the rustode GUI (read back from its own stdout). Neither path needed a prompt, a permission override or anything installed. Over 3 recorded sessions per target, 143 of 144 injected characters arrived. The one loss was the first keystroke (Shift+H) of one App session through `fake_input`. libei lost nothing and had the smaller spread, so it is the proposed path. The real desktop session advertises the same global and the same D-Bus object, so every injector must check that it is talking to the virtual compositor before it sends anything.

## Evidence

All runs were done on 2026-11-17 with the App at `01-app/VSCode-linux-x64/codium` (integrity checked before and after, silence), an isolated profile in `mktemp -d /tmp/rustode-app.XXXXXX`, CDP on port 9333 and no network (`bwrap --unshare-net`). The tools and raw outputs are in [`0002-attachments/`](0002-attachments/).

| What | Where | Note |
|---|---|---|
| Globals of the virtual compositor: `org_kde_kwin_fake_input` v6, `zwp_text_input_manager_v3` v2, no `zwp_virtual_keyboard_manager_v1`, no `zwp_input_method_manager_v2` | `wayland-info` run inside `run_virtual.sh` (command below) | measured; `kwin_wayland --version` = `kwin 6.8.2` |
| Private bus exposes `org.kde.KWin` with `/org/kde/KWin/EIS/RemoteDesktop`, method `connectToEIS(in i capabilities, out h, out i cookie)` | `gdbus introspect` inside the session (command below) | measured; session bus was `unix:path=/tmp/dbus-…`, the real one is `unix:path=/run/user/1000/bus` |
| `fake_input` accepted with no prompt: `bound org_kde_kwin_fake_input v6 on qa-virt-<pid>` | console of each session; tool `0002-attachments/tools/fakeinput/src/main.rs` | measured; `authenticate` is sent and nothing is asked |
| EIS accepted: `connectToEIS(caps=1) -> fd 6 cookie 1`, `seat rustode-spike seat kbd=1`, `device rustode-spike eis keyboard kbd=1`, `keyboard resumed` | console of each session; tool `0002-attachments/tools/eistype/eistype.c` | measured; capability 1 = keyboard |
| Stand-in received all 72 characters: `BUFFER "Hello fake 1 eis typed 2"` in each run | `0002-attachments/runs/{1,2,3}/s1-standin.out` | measured; `RECV`/`TEXT` lines come from `egui::Event` |
| App received `"Hello fake 1 eis typed 2"` in runs 1 and 3 and `"ello fake 1 eis typed 2"` in run 2 | `0002-attachments/runs/{1,2,3}/s2-app.png`; `s2-app-keys.out` (25, 23, 25 keydowns, all `trusted=true`) | measured; read from `.view-lines` over CDP |
| App runs on native Wayland: `--ozone-platform=wayland` on its child processes | `ps` from the driver, printed per session | measured; the App log also says `'--ozone-platform=wayland' is not compatible with Vulkan` (software rendering, as ADR 0006 expects) |
| Send → receive deltas, 3 runs | `python3 -I 0002-attachments/tools/deltas.py 0002-attachments/runs` | **estimated** meaning; see Limitations |
| Real session advertises the same `org_kde_kwin_fake_input` v6 and the same `connectToEIS` method | `wayland-info` and `gdbus introspect` in the owner's session, read-only (nothing injected) | measured; this is why the guards below are mandatory |
| Guards refuse outside the virtual session: `refusing: WAYLAND_DISPLAY="wayland-1" is not a qa-virt-* socket` (exit 2), and the same refusal from `eistype` | both tools run once from the real session | measured |

Output of `deltas.py` (each sent character matched with the first later event carrying the same character):

| Target | Path | Received / sent | Min | Median | Max |
|---|---|---|---|---|---|
| App (DOM `keydown`) | libei | 36 / 36 | 0.44 ms | 0.75 ms | 1.02 ms |
| App (DOM `keydown`) | `fake_input` | 35 / 36 (run 2 lost `'H'`) | 0.48 ms | 0.76 ms | 7.96 ms |
| Stand-in (egui frame) | libei | 36 / 36 | 0.20 ms | 6.40 ms | 16.40 ms |
| Stand-in (egui frame) | `fake_input` | 36 / 36 | 1.26 ms | 8.95 ms | 16.93 ms |

Commands (from the project root unless stated otherwise; `$T` is a temporary folder from `mktemp -d`):

```sh
# Integrity, before and after (from 01-app/VSCode-linux-x64): silence = intact
sha256sum -c --quiet ../../02-package-analysis/manifest-sha256.txt

# Globals and D-Bus of the virtual compositor: probe.sh runs inside the session
#   wayland-info | grep "interface: '"
#   gdbus introspect --session --dest org.kde.KWin --object-path /org/kde/KWin/EIS/RemoteDesktop
~/Projects/vscode/qa/suite/tools/run_virtual.sh "$T/probe.sh"

# Build the tools (nothing is installed: libei 1.6.0, libsystemd, wayland 1.26 and gcc were already present;
# crates come from crates.io)
mkdir -p "$T/bin"
(cd 09-findings/0002-attachments/tools/fakeinput && CARGO_TARGET_DIR="$T/target" cargo build --release --locked && cp "$T/target/release/fakeinput" "$T/bin/")
(cd 09-findings/0002-attachments/tools/standin   && CARGO_TARGET_DIR="$T/target" cargo build --release --locked && cp "$T/target/release/standin"   "$T/bin/")
gcc -O2 -o "$T/bin/eistype" 09-findings/0002-attachments/tools/eistype/eistype.c $(pkg-config --cflags --libs libei-1.0 libsystemd)

# One run (repeated for n = 1, 2, 3); check MemAvailable >= 1500 MB before each
O=09-findings/0002-attachments/runs/$n; mkdir -p "$O"
BIN="$T/bin" ~/Projects/vscode/qa/suite/tools/run_virtual.sh 09-findings/0002-attachments/tools/session-standin.sh "$O"
P=$(mktemp -d /tmp/rustode-app.XXXXXX); mkdir -p "$P/ws"
BIN="$T/bin" ~/Projects/vscode/qa/suite/tools/run_virtual.sh 09-findings/0002-attachments/tools/session-app.sh "$O" "$P"
pkill -TERM -f -- "--user-data-dir=$P/ud"; rm -rf "$P"

python3 -I 09-findings/0002-attachments/tools/deltas.py 09-findings/0002-attachments/runs
```

## How each app is reached

- **Routing.** Both paths inject into KWin's seat, the way a physical keyboard does. KWin then delivers the keys over `wl_keyboard` to whichever surface has keyboard focus. The tools don't address a window. Each session had a single window, and it had focus without anything being done to give it focus.
- **The App.** The App runs as a native Wayland client (`--ozone-platform=wayland`), so the keys arrive as real `wl_keyboard` events. Chromium turns them into trusted DOM `keydown` events (`isTrusted=true`). The driver uses CDP only to open an untitled editor (`Control+n`), install a `keydown` listener and read the text back. None of the measured text goes through CDP.
- **The rustode stand-in.** It is `0002-attachments/tools/standin`, an eframe 0.36.2 window (glow, Wayland) with one `TextEdit`. rustode has no GUI yet, and this stand-in is not rustode. It uses the same winit → egui-winit → `egui::Event` path that the planned eframe GUI will use, and it prints each `Key` and `Text` event with a wall-clock timestamp.

## Injection recipe

1. **Start the session.** Start the virtual compositor with `run_virtual.sh`. It gives the session its own `WAYLAND_DISPLAY=qa-virt-<pid>` and its own D-Bus (`dbus-run-session`).
2. **Run the injector inside that session.** Run it as a child of the session script, so it inherits both variables. Before anything else, the injector must refuse to run unless both of these hold:
   - `WAYLAND_DISPLAY` starts with `qa-virt-`;
   - `DBUS_SESSION_BUS_ADDRESS` is not under `/run/user/`.

   Both tools in `0002-attachments/tools/` implement these checks.
3. **Inject.**
   - **libei (proposed):** call `connectToEIS(1)` on `org.kde.KWin` at `/org/kde/KWin/EIS/RemoteDesktop`. Then call `ei_setup_backend_fd` on the returned fd, bind the keyboard capability on the seat and wait for `EI_EVENT_DEVICE_RESUMED`. After that, call `ei_device_start_emulating` and, for each key, `ei_device_keyboard_key` followed by `ei_device_frame`.
   - **`fake_input` (fallback):** bind `org_kde_kwin_fake_input`, send `authenticate`, then send `keyboard_key(evdev_code, 1/0)`, with a roundtrip after each key.
4. **Confirm what arrived.**
   - App: read the editor text and the trusted `keydown` events over CDP (`0002-attachments/tools/app-driver.cjs`).
   - rustode: read the app's own output, which is the stand-in's stdout today.

   Before timing anything, the harness compares the received count with the sent count, because a lost key was observed.
5. **Clean up.** Close the App with SIGTERM sent only to the processes that carry its profile (gap G8), then remove the profile folder.

## Limitations

- **The deltas are not a latency measurement.**
  - The send timestamp is `CLOCK_REALTIME` in the injector.
  - The App timestamp is `performance.timeOrigin + performance.now()`, which is quantized (0.1 ms steps observed) and can drift from `CLOCK_REALTIME`.
  - The stand-in timestamps when egui runs its next frame, so its numbers include waiting for that frame, up to one 60 Hz interval (16.7 ms).

  The two targets are measured at different points and must not be compared. Typing latency for the benchmark needs input-to-frame timing on both sides.
- **One lost keystroke out of 72 sent to the App**, plus 72 sent to the stand-in. It was the first key of the session (Shift+H, `fake_input`, run 2), and neither Shift nor H reached the renderer. One case doesn't make a rule. The cause is a hypothesis: a race between the App window getting keyboard focus or keymap and the first event from a newly created fake device. A warm-up key and a received-count check are the obvious mitigations; they were not tested.
- **The first exploratory sessions are not counted.** Three sessions of each kind ran before the tools were moved into `0002-attachments/`; all their characters arrived, but their raw files were not kept. They ran the App without `NODE_DISABLE_COMPILE_CACHE=1`, and Node wrote 3 new files into `01-app/VSCode-linux-x64/resources/app/node-compile-cache/extension-host/v24.22.1-x64-7034790f-1000/` (`ef46d9a3`, `960fc38d`, `0cecd230`).
  - These files are not in the manifest and not in the live app, so `sha256sum -c` still passes; it only checks the files it lists.
  - `assemble.sh` removes them on its next run (`rsync --delete`).
  - The recorded runs set `NODE_DISABLE_COMPILE_CACHE=1`, and `find 0[1-8]* -newer <marker>` was empty afterwards.
- **Not tested:**
  - typing speed above one key per 40 ms;
  - non-ASCII text and key combinations other than Shift;
  - several windows competing for focus;
  - the real eframe GUI with wgpu (the stand-in uses glow);
  - the input-method path (`zwp_text_input_v3`).
- **Behavior of the real session.** Whether the real session would also accept a client without a prompt was deliberately not tested, because that would mean injecting into the owner's desktop. Only the advertised global and the D-Bus method were observed there.

## Next steps

- Candidate gap: the lost first keystroke on the App. Repeat with a warm-up key, and find out whether focus, keymap or device creation is the cause.
- Candidate gap: §0.6 integrity does not detect files added to `01-app/`, and launching the App from `01-app/` writes to the compile cache. Proposed fix (owner's decision): `launch-built-app` and the harness set `NODE_DISABLE_COMPILE_CACHE=1`, and the integrity check also lists files missing from the manifest.
- Build the typing-latency scenario on libei, measuring input to frame on both editors.

## Proposed amendment (owner's decision)

ADR 0006 and AGENTS.md were not changed. If the owner approves, the change goes through as its own ADR change (spec, "Agent rulebook").

**ADR 0006, containment rule 2.** Current text: "Native input is still forbidden without authorization (AGENTS.md §3.2); on the virtual display the app is driven only over CDP." Proposed text:

> 2. Native input stays forbidden in the real session without authorization (AGENTS.md §3.2). On the virtual display, keystrokes may be injected only through the virtual compositor's own input paths: libei with the fd from `org.kde.KWin.EIS.RemoteDesktop.connectToEIS` on the session's private D-Bus (preferred), or `org_kde_kwin_fake_input` on its `qa-virt-*` socket. The injecting client must run inside the `run_virtual.sh` session and refuse to start unless `WAYLAND_DISPLAY` starts with `qa-virt-` and the session bus is not the real one (`/run/user/<uid>/bus`). Tools that write to `/dev/uinput` (`ydotool`, `dotool`) or connect to the real session's socket or bus remain forbidden, because they reach the owner's desktop. Everything else on the virtual display is still driven over CDP.

Add to "Consequences": "Injected keystrokes are counted against what was received before any timing is used (finding 0002 saw one lost first key)."

**AGENTS.md §3.2.** Current text: "Native input (e.g. `ydotool`) needs the owner's explicit authorization, with the duration announced beforehand." Proposed text:

> Native input in the real session (e.g. `ydotool`, anything using `/dev/uinput`) needs the owner's explicit authorization, with the duration announced beforehand. Injection into the virtual display through libei or `org_kde_kwin_fake_input`, under the guards of ADR 0006 rule 2, does not need it.

**Related, separate decision.** Set `NODE_DISABLE_COMPILE_CACHE=1` whenever the App is launched from `01-app/` (the `launch-built-app` skill and the QA launchers), so that running the App does not write into evidence folder 01.
