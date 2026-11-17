#!/bin/bash
# Runs inside run_virtual.sh. Usage: BIN=<dir with fakeinput, eistype, standin> session-standin.sh <out-dir>
set -u; OUT=$1
echo "session WAYLAND_DISPLAY=$WAYLAND_DISPLAY DBUS=$DBUS_SESSION_BUS_ADDRESS"
"$BIN/standin" > "$OUT/s1-standin.out" 2> "$OUT/s1-standin.err" & SP=$!
sleep 4
"$BIN/fakeinput" "Hello fake 1" 40 > "$OUT/s1-fake.out"; sleep 1
"$BIN/eistype" " eis typed 2" 40 > "$OUT/s1-eis.out"; sleep 1
kill $SP; wait $SP 2>/dev/null; grep BUFFER "$OUT/s1-standin.out" | tail -1
