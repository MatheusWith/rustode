# Usage: python3 -I deltas.py <runs-dir>
# Matches each injected character (SEND lines) to the first later received event with the same
# character (TEXT for the stand-in, KEYDOWN for the App) and prints send->receive deltas and losses.
import sys, os, re, statistics as st
R = sys.argv[1]; agg = {}; lost = {}
def events(path, prefix, quote):
    pat = re.compile(rf"^{prefix} (\S+) {quote}(.){quote}")
    return [(float(m[1]), m[2]) for m in map(pat.match, open(path)) if m]
for n in sorted(os.listdir(R)):
    d0 = os.path.join(R, n)
    for tgt, f, prefix, sfx in (("standin", "s1-standin.out", "TEXT", "s1"), ("app", "s2-app-keys.out", "KEYDOWN", "s2")):
        recv = [[t, c, False] for t, c in events(os.path.join(d0, f), prefix, '"')]
        for inj in ("fake", "eis"):
            for t, c in events(os.path.join(d0, f"{sfx}-{inj}.out"), "SEND", "'"):
                m = next((r for r in recv if not r[2] and r[1] == c and r[0] >= t - 5), None)
                if m: m[2] = True; agg.setdefault((tgt, inj), []).append(m[0] - t)
                else: lost.setdefault((tgt, inj), []).append(f"run {n} {c!r}")
for k in sorted(set(agg) | set(lost)):
    d = agg.get(k, []); l = lost.get(k, [])
    print(f"{k[0]:8} {k[1]:5} received={len(d)} lost={len(l)} {l if l else ''} "
          f"min={min(d):.2f} median={st.median(d):.2f} max={max(d):.2f} ms")
