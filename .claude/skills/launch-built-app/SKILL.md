---
name: launch-built-app
description: Opens the built app in a temporary profile, on a virtual display and with the debugging port on, to observe behavior over CDP without touching the owner's desktop or profiles.
disable-model-invocation: true
argument-hint: "What do you want to observe in the app?"
---

# Open the built app for observation

The rules of [`AGENTS.md`](../../../AGENTS.md) §3 apply:
- isolated profile;
- no keyboard or mouse;
- network only if needed;
- one instance at a time.

## Process

### 1. Check the memory

```sh
awk '/MemAvailable/ {print int($2/1024) " MB free"}' /proc/meminfo
```

Below 1800 MB, don't open it: tell the owner.

### 2. Prepare the profile and open on a virtual display

```sh
P=$(mktemp -d /tmp/rustode-app.XXXXXX)
~/Projects/vscode/qa/suite/tools/run_virtual.sh \
  "$HOME/Projects/rustode/01-app/VSCode-linux-x64/codium" \
  --user-data-dir="$P/ud" --extensions-dir="$P/ext" --shared-data-dir="$P/shared" \
  --remote-debugging-port=9333 --disable-workspace-trust --new-window "$P/ws" &
```

- No network: prefix the command with `bwrap --dev-bind / / --unshare-net --die-with-parent`.
- To see the traffic: use `~/Projects/vscode/qa/suite/scenarios/run_network.sh` instead of this step.
- The app copied into `01-app/` is identical to the original (manifest). Run it from there or from `~/Projects/VSCode-linux-x64/codium`.

### 3. Connect over CDP

Wait for `curl -s http://127.0.0.1:9333/json/version` to answer. Drive the app with one of these:
- the `playwright-core` in `~/Projects/vscode/node_modules/playwright-core` (`chromium.connectOverCDP`);
- the utilities in `~/Projects/vscode/qa/suite/scenarios/lib.cjs` (`connect`, `ui`, `recorder`);
- REA's `list_electron_targets` and `inspect_electron_page` (AGENTS.md §6, "REA in this project").

### 4. Close and clean up

```sh
pkill -TERM -f -- "--user-data-dir=$P/ud"; sleep 3; pkill -KILL -f -- "--user-data-dir=$P/ud"; rm -rf "$P"
```

Send SIGTERM first, and only to processes with the profile on their command line. SIGTERM to all processes at once leaves the app stuck (gap G8).

## Output

What was observed goes into an analysis (the `write-finding` skill). Keep its screenshots and provenance sidecar ([`docs/templates/provenance.md`](../../../docs/templates/provenance.md)) outside folders 01 to 08.
