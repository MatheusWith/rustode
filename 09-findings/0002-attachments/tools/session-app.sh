#!/bin/bash
# Runs inside run_virtual.sh. Usage: BIN=<dir with fakeinput, eistype> session-app.sh <out-dir> <profile-dir>
set -u
echo "session WAYLAND_DISPLAY=$WAYLAND_DISPLAY DBUS=$DBUS_SESSION_BUS_ADDRESS"
exec bwrap --dev-bind / / --unshare-net --die-with-parent \
  "$HOME/.local/opt/node-v24.22.1-linux-x64/bin/node" "$(dirname "$0")/app-driver.cjs" "$1" "$2"
