# 0004: How do rustode and the App compare on the whole versioned list? (the M9 suite, typing latency not measured)

> **Date:** 2026-11-23 · **Confidence:** measured (every number comes from a run of `rustode bench` with its repetitions; each scenario's work was checked equal on both sides; the limits of the comparison are in Limitations) · **Gap:** closes G24, moves G22 forward, opens G25 and G26 · **Components:** `rustode bench`, `bench/launch.cjs`, `bench/app_internals.cjs`, `rustode_workspace::bench`, `rustode-gui`, the App (a verified copy), `kwin_wayland --virtual`

## Question

[ADR 0024](../docs/adr/0024-comparison-with-the-app.md) lists the scenarios of the comparison with the App; [finding 0003](0003-first-app-comparison.md) ran four of them (startup, idle memory, opening 1 MB, 100 MB and 1 GB files). Ticket 42 (M9) asks for the rest and for the whole list at one commit: 10k random inserts then the undo of all of them, and a regex search over a monorepo, with the same work on both sides. What do rustode and the App cost on each, and where is the comparison not like for like?

## Scope

Measured, both sides, at one commit (`6d7b04e50f98`; the 1 GB open at `8ee6e839b64e`, see Evidence), list version 3 (`bench/scenarios.toml`): startup to first frame cold and warm, idle memory, opening 1 MB, 100 MB and 1 GB, 10k random inserts then undo of all, regex search over a generated monorepo.

**Not measured: typing latency, waiting on ADR 0028.** [ADR 0028](../docs/adr/0028-virtual-display-input.md) is not accepted and [AGENTS.md](../AGENTS.md) §11.5.5 forbids injecting a keystroke into either editor for a benchmark. Nothing here sends a key: the GUI runs its scripts on itself, and the App is driven over CDP, which §3.2 allows.

## Short answer

All numbers are medians (p50) of the runs listed under Evidence, on the virtual display with software rendering for both editors (Mesa llvmpipe), release build of `rustode-gui`, the App from a verified copy; ratios are computed from those medians.

- **Startup, with a common first-frame event (G22).** With a warm file cache `rustode-gui` presented its first frame 154 ms after the spawn (n = 20); the App's display compositor finished the first frame drawn after the first line entered the DOM 1,939 ms after the spawn (n = 20): **12.6 times** rustode's (computed). With a cold file cache: rustode 179 ms, App 2,184 ms, 12.2 times (computed). The App's number is a first swap that may be one frame early (the second swap, 1,947 ms warm, bounds it) and includes the cost of the trace that finds it: a control run without the trace had first paint 1,798 ms and first line 1,891 ms against 1,835 and 1,928 ms with it, so about 37 ms of the App's number is the trace (one pair of runs). 
- **Idle memory**, 20 s after ready (n = 5): rustode 132 MB `Rss` and 103 MB `Pss`; the App 1,605 MB `Rss` and 853 MB `Pss` (the App also loads extensions, a terminal and source control that rustode lacks).
- **Opening files**, launch to first frame (rustode) against launch to first swap (App): 1 MB 151 ms against 1,931 ms; 100 MB 322 ms against 2,390 ms; 1 GB 2,337 ms against 6,359 ms.
- **10k random inserts, then undo of all** (1 MiB file, n = 5): the inserts took rustode 13.8 ms and the App 9,338 ms (676.7 times, computed); the undo of all, 10.8 ms against 9,774 ms. Until the next frame is presented: 23.5 ms against 9,490 ms for the inserts, 17.9 ms against 10,075 ms for the undo. Peak memory (`VmHWM`, sum over processes) after the script: 144 MB against 2,246 MB. Both editors held the same text after the inserts and after the undo (hash and length equal). **Not like for like**: the two APIs do different work per edit (Limitations).
- **Regex search over a monorepo** (5,000 source files, 91.8 MB in 5,626 files, n = 5): first result 96.7 ms against 420 ms; complete 96.7 ms against 532 ms. Both found 10,436 matches in 3,002 files, the number the generator's oracle expects and an independent `grep` confirms.
- **Not measured: typing latency**, waiting on ADR 0028.
- **Method findings:** the App's `TextModel` and command service are reachable over CDP without a global handle (`bench/app_internals.cjs`); the App's first presented frame can be read from a Chromium trace (`Display::DrawAndSwap`); `rustode-gui` now exits with status 1 and a hint, not 0 and silence, when no graphics adapter works (G24, closed).

## Evidence

Every run is one `rustode bench <scenario>` from the clone at `<clone>` (cwd), on 2026-11-23 (UTC stamps are in the run ids), all with `rustode` at `6d7b04e50f98` (`git` clean in every report) except `open-file-1gb`, at `8ee6e839b64e`: that commit only makes the runner skip the in-process open, with a note, instead of refusing the whole run when memory is short (`rustode-gui` is byte-identical: the same sha256). The release build (`cargo build --release --locked -p rustode-cli -p rustode-gui`), `rustode-gui` sha256 `3f3a3e96b70e13fd2c534090af7f7f2c345035a5570d9a4904680b5a7715ea95`, list version 3. Machine: 11th Gen Intel(R) Core(TM) i5-11400H @ 2.70GHz, 12 CPUs, 7.5 GB RAM, Linux 7.3.4-arch1-2, KWin virtual, Node v24.22.1, playwright-core 1.61.0-alpha-2026-06-04. App: VSCodium 1.147.0 (`22d8dccf0f29`), intact before, in the copy and after each run ("ok: 3008 listed files match, 0 files outside the manifest" in every report), 0 files added to the copy in every run (`app/copy-writes.txt`), and `sha256sum -c --quiet` of `01-app/` plus `find 0[1-8]* -newer <marker>` printed nothing before and after the whole battery (`battery.log`). The App rendered in software in every run (`gpu`: `ANGLE (Mesa, llvmpipe (LLVM 23.1.4 256 bits), OpenGL ES 3.2 Mesa 26.2.7-arch1.1)`, `gpu_compositing: disabled_software`). The commit hashes in this finding are those of the branch before it was rebased onto `main` (`1d23fbb`); the rebase had no conflict and the code under test is the same.

Raw outputs (small files only: `report.txt`, `report.json`, each driver's `results.json`, `driver-config.json` and `driver.log`, the first launch's `app.log`, the battery log) are in [`0004-attachments/runs/`](0004-attachments/runs/); private paths are replaced (`<project>`, `<clone>`, `<target>`, `<runs>`, `<copies>`, `<scratch>`, `<qa>`, `<node>`, `~`). The run folders with fixtures, screenshots and watchdog series stay outside the repository.

Runs:

- 2026-11-23-120921-startup-cold
- 2026-11-23-121315-startup-warm
- 2026-11-23-123453-startup-warm (control, `--no-app-trace`, App only)
- 2026-11-23-121702-idle-rss
- 2026-11-23-122212-open-file-1mb
- 2026-11-23-122321-open-file-100mb
- 2026-11-23-125142-open-file-1gb
- 2026-11-23-122615-edit-undo-10k
- 2026-11-23-122932-search-monorepo

Two attempts were refused by the memory guards before the runs above, as designed, and are not evidence: the first `open-file-1gb` (the in-process 1 GiB open needs 3,772 MB available and the machine had 3,519, which then refused the whole run; this is what the commit `8ee6e839b64e` changes) and the first control run (1,716 MB available where the App needs 2,000). Both were repeated; in the repeated `open-file-1gb` the in-process `Workspace::open` was skipped with a note (3,376 MB available), so finding 0003's 1.64 s for it is not repeated here.

Times are in ms and memory in MB. `n` is the number of recorded launches (warm-ups excluded); p95 is nearest-rank, so with n ≤ 20 it is close to the maximum. The tables list every metric of the report; "launch to first swap" and "launch to second swap" are the App's presented-frame events (G22), "launch to first frame" is rustode's.

### Startup, first frame

**2026-11-23-120921-startup-cold** (status ok)

| Side | Metric | n | min | p50 | p95 | max |
|---|---|---|---|---|---|---|
| rustode | launch to first frame (ms) | 20 | 176 | 179 | 334 | 344 |
| rustode | main to first frame (ms) | 20 | 170 | 173 | 326 | 338 |
| rustode | shutdown after SIGTERM (ms) | 20 | 60.2 | 61.0 | 63.0 | 63.6 |
| rustode | RSS at first frame (MB) | 20 | 126 | 126 | 126 | 127 |
| rustode | PSS at first frame (MB) | 20 | 102 | 102 | 103 | 103 |
| App | launch to first paint (ms) | 20 | 2,064 | 2,080 | 2,109 | 2,115 |
| App | launch to workbench (ms) | 20 | 2,052 | 2,067 | 2,098 | 2,104 |
| App | launch to first line shown (ms) | 20 | 2,155 | 2,181 | 2,212 | 2,212 |
| App | launch to first swap (ms) | 20 | 2,166 | 2,184 | 2,213 | 2,222 |
| App | launch to second swap (ms) | 20 | 2,175 | 2,195 | 2,223 | 2,231 |
| App | first swap after DOM mark (ms) | 20 | 16.6 | 17.5 | 19.2 | 20.0 |
| App | workbench to first line (ms) | 20 | 101 | 111 | 125 | 128 |
| App | shutdown after SIGTERM (ms) | 20 | 121 | 250 | 329 | 345 |
| App | RSS at first line (MB) | 20 | 1,141 | 1,155 | 1,175 | 1,178 |
| App | PSS at first line (MB) | 20 | 581 | 591 | 603 | 604 |

**2026-11-23-121315-startup-warm** (status ok)

| Side | Metric | n | min | p50 | p95 | max |
|---|---|---|---|---|---|---|
| rustode | launch to first frame (ms) | 20 | 147 | 154 | 169 | 178 |
| rustode | main to first frame (ms) | 20 | 145 | 152 | 167 | 175 |
| rustode | shutdown after SIGTERM (ms) | 20 | 60.2 | 62.8 | 65.3 | 68.1 |
| rustode | RSS at first frame (MB) | 20 | 128 | 128 | 131 | 131 |
| rustode | PSS at first frame (MB) | 20 | 99.1 | 100 | 106 | 106 |
| App | launch to first paint (ms) | 20 | 1,803 | 1,835 | 1,962 | 1,976 |
| App | launch to workbench (ms) | 20 | 1,791 | 1,825 | 1,950 | 1,966 |
| App | launch to first line shown (ms) | 20 | 1,894 | 1,928 | 2,054 | 2,104 |
| App | launch to first swap (ms) | 20 | 1,899 | 1,939 | 2,064 | 2,124 |
| App | launch to second swap (ms) | 20 | 1,905 | 1,947 | 2,074 | 2,172 |
| App | first swap after DOM mark (ms) | 20 | 15.5 | 17.5 | 19.4 | 27.7 |
| App | workbench to first line (ms) | 20 | 96.8 | 105 | 115 | 138 |
| App | shutdown after SIGTERM (ms) | 20 | 123 | 202 | 323 | 328 |
| App | RSS at first line (MB) | 20 | 1,197 | 1,228 | 1,245 | 1,264 |
| App | PSS at first line (MB) | 20 | 565 | 595 | 605 | 614 |

Control, the App alone without the trace (`--no-app-trace`), to bound the trace's cost:

**2026-11-23-123453-startup-warm** (status ok)

| Side | Metric | n | min | p50 | p95 | max |
|---|---|---|---|---|---|---|
| App | launch to first paint (ms) | 20 | 1,767 | 1,798 | 1,878 | 1,888 |
| App | launch to workbench (ms) | 20 | 1,753 | 1,788 | 1,868 | 1,879 |
| App | launch to first line shown (ms) | 20 | 1,856 | 1,891 | 1,972 | 1,984 |
| App | workbench to first line (ms) | 20 | 95.9 | 103 | 112 | 112 |
| App | shutdown after SIGTERM (ms) | 20 | 210 | 271 | 351 | 2,172 |
| App | RSS at first line (MB) | 20 | 1,233 | 1,272 | 1,298 | 1,312 |
| App | PSS at first line (MB) | 20 | 586 | 605 | 616 | 623 |

No launch needed SIGKILL after SIGTERM in these runs (`needed_sigkill`: 0 in every launch of every run; in finding 0003 half of the cold launches did), no launch left part of its main file in the page cache after the eviction (`cache_reference_resident_pct` 0.0, one attempt each), and no trace was missing (`trace_error` empty).

### Idle memory

**2026-11-23-121702-idle-rss** (status ok)

| Side | Metric | n | min | p50 | p95 | max |
|---|---|---|---|---|---|---|
| rustode | launch to first frame (ms) | 5 | 147 | 150 | 153 | 153 |
| rustode | main to first frame (ms) | 5 | 145 | 147 | 151 | 151 |
| rustode | shutdown after SIGTERM (ms) | 5 | 60.4 | 60.8 | 60.9 | 60.9 |
| rustode | RSS at first frame (MB) | 5 | 129 | 129 | 130 | 130 |
| rustode | PSS at first frame (MB) | 5 | 103 | 103 | 104 | 104 |
| rustode | idle RSS, all processes (MB) | 5 | 132 | 132 | 132 | 132 |
| rustode | idle PSS, all processes (MB) | 5 | 103 | 103 | 104 | 104 |
| App | launch to first paint (ms) | 5 | 1,793 | 1,803 | 1,832 | 1,832 |
| App | launch to workbench (ms) | 5 | 1,783 | 1,791 | 1,819 | 1,819 |
| App | launch to first line shown (ms) | 5 | 1,879 | 1,893 | 1,918 | 1,918 |
| App | launch to first swap (ms) | 5 | 1,890 | 1,904 | 1,931 | 1,931 |
| App | launch to second swap (ms) | 5 | 1,899 | 1,913 | 1,938 | 1,938 |
| App | first swap after DOM mark (ms) | 5 | 16.8 | 17.5 | 18.6 | 18.6 |
| App | workbench to first line (ms) | 5 | 96.1 | 99.0 | 102 | 102 |
| App | shutdown after SIGTERM (ms) | 5 | 250 | 252 | 261 | 261 |
| App | RSS at first line (MB) | 5 | 1,319 | 1,328 | 1,352 | 1,352 |
| App | PSS at first line (MB) | 5 | 605 | 614 | 624 | 624 |
| App | idle RSS, all processes (MB) | 5 | 1,575 | 1,605 | 1,628 | 1,628 |
| App | idle PSS, all processes (MB) | 5 | 833 | 853 | 874 | 874 |

### Opening files

**2026-11-23-122212-open-file-1mb** (status ok)

| Side | Metric | n | min | p50 | p95 | max |
|---|---|---|---|---|---|---|
| rustode | Workspace::open (ms) | 20 | 1.2 | 1.2 | 1.3 | 1.6 |
| rustode | launch to first frame (ms) | 5 | 150 | 151 | 151 | 151 |
| rustode | main to first frame (ms) | 5 | 147 | 148 | 149 | 149 |
| rustode | shutdown after SIGTERM (ms) | 5 | 60.6 | 61.8 | 64.1 | 64.1 |
| rustode | RSS at first frame (MB) | 5 | 133 | 134 | 134 | 134 |
| rustode | PSS at first frame (MB) | 5 | 108 | 108 | 108 | 108 |
| App | launch to first paint (ms) | 5 | 1,800 | 1,813 | 1,817 | 1,817 |
| App | launch to workbench (ms) | 5 | 1,788 | 1,801 | 1,804 | 1,804 |
| App | launch to first line shown (ms) | 5 | 1,917 | 1,922 | 1,935 | 1,935 |
| App | launch to first swap (ms) | 5 | 1,918 | 1,931 | 1,933 | 1,933 |
| App | launch to second swap (ms) | 5 | 1,924 | 1,938 | 1,943 | 1,943 |
| App | first swap after DOM mark (ms) | 5 | 14.9 | 17.1 | 17.4 | 17.4 |
| App | workbench to first line (ms) | 5 | 119 | 128 | 130 | 130 |
| App | shutdown after SIGTERM (ms) | 5 | 188 | 329 | 345 | 345 |
| App | RSS at first line (MB) | 5 | 1,339 | 1,362 | 1,393 | 1,393 |
| App | PSS at first line (MB) | 5 | 611 | 622 | 638 | 638 |

**2026-11-23-122321-open-file-100mb** (status ok)

| Side | Metric | n | min | p50 | p95 | max |
|---|---|---|---|---|---|---|
| rustode | Workspace::open (ms) | 10 | 128 | 129 | 152 | 152 |
| rustode | launch to first frame (ms) | 5 | 320 | 322 | 325 | 325 |
| rustode | main to first frame (ms) | 5 | 318 | 320 | 322 | 322 |
| rustode | shutdown after SIGTERM (ms) | 5 | 61.1 | 61.6 | 63.1 | 63.1 |
| rustode | RSS at first frame (MB) | 5 | 246 | 246 | 246 | 246 |
| rustode | PSS at first frame (MB) | 5 | 220 | 220 | 220 | 220 |
| App | launch to first paint (ms) | 5 | 1,780 | 1,786 | 1,794 | 1,794 |
| App | launch to workbench (ms) | 5 | 1,771 | 1,773 | 1,782 | 1,782 |
| App | launch to first line shown (ms) | 5 | 2,368 | 2,387 | 2,394 | 2,394 |
| App | launch to first swap (ms) | 5 | 2,367 | 2,390 | 2,393 | 2,393 |
| App | launch to second swap (ms) | 5 | 2,377 | 2,403 | 2,408 | 2,408 |
| App | first swap after DOM mark (ms) | 5 | 14.1 | 15.5 | 15.8 | 15.8 |
| App | workbench to first line (ms) | 5 | 594 | 606 | 623 | 623 |
| App | shutdown after SIGTERM (ms) | 5 | 252 | 263 | 268 | 268 |
| App | RSS at first line (MB) | 5 | 1,497 | 1,512 | 1,554 | 1,554 |
| App | PSS at first line (MB) | 5 | 748 | 773 | 806 | 806 |

**2026-11-23-125142-open-file-1gb** (status ok)

| Side | Metric | n | min | p50 | p95 | max |
|---|---|---|---|---|---|---|
| rustode | launch to first frame (ms) | 3 | 2,068 | 2,337 | 2,620 | 2,620 |
| rustode | main to first frame (ms) | 3 | 2,064 | 2,333 | 2,617 | 2,617 |
| rustode | shutdown after SIGTERM (ms) | 3 | 61.2 | 62.5 | 117 | 117 |
| rustode | RSS at first frame (MB) | 3 | 1,301 | 1,302 | 1,303 | 1,303 |
| rustode | PSS at first frame (MB) | 3 | 1,275 | 1,294 | 1,295 | 1,295 |
| App | launch to first paint (ms) | 3 | 1,820 | 1,838 | 2,050 | 2,050 |
| App | launch to workbench (ms) | 3 | 1,812 | 1,830 | 2,042 | 2,042 |
| App | launch to first line shown (ms) | 3 | 6,299 | 6,350 | 6,617 | 6,617 |
| App | launch to first swap (ms) | 3 | 6,306 | 6,359 | 6,597 | 6,597 |
| App | launch to second swap (ms) | 3 | 6,368 | 6,382 | 6,673 | 6,673 |
| App | first swap after DOM mark (ms) | 3 | 13.3 | 13.9 | 16.9 | 16.9 |
| App | workbench to first line (ms) | 3 | 4,469 | 4,539 | 4,576 | 4,576 |
| App | shutdown after SIGTERM (ms) | 3 | 253 | 256 | 260 | 260 |
| App | RSS at first line (MB) | 3 | 2,313 | 2,314 | 2,372 | 2,372 |
| App | PSS at first line (MB) | 3 | 1,721 | 1,726 | 1,779 | 1,779 |

### 10k random inserts, then undo of all

**2026-11-23-122615-edit-undo-10k** (status ok)

| Side | Metric | n | min | p50 | p95 | max |
|---|---|---|---|---|---|---|
| rustode | launch to first frame (ms) | 5 | 150 | 152 | 153 | 153 |
| rustode | RSS at first frame (MB) | 5 | 135 | 136 | 136 | 136 |
| rustode | PSS at first frame (MB) | 5 | 110 | 110 | 111 | 111 |
| rustode | 10k inserts, applying (ms) | 5 | 13.5 | 13.8 | 13.8 | 13.8 |
| rustode | 10k inserts, to next frame (ms) | 5 | 23.2 | 23.5 | 23.6 | 23.6 |
| rustode | undo of all, applying (ms) | 5 | 10.7 | 10.8 | 11.1 | 11.1 |
| rustode | undo of all, to next frame (ms) | 5 | 17.4 | 17.9 | 18.2 | 18.2 |
| rustode | undo steps (n) | 5 | 10,000 | 10,000 | 10,000 | 10,000 |
| rustode | peak RSS before the script (MB) | 5 | 135 | 136 | 136 | 136 |
| rustode | peak RSS after the script (MB) | 5 | 144 | 144 | 144 | 144 |
| rustode | peak RSS, largest process (MB) | 5 | 144 | 144 | 144 | 144 |
| App | launch to first line shown (ms) | 5 | 1,909 | 1,911 | 1,927 | 1,927 |
| App | RSS at first line (MB) | 5 | 1,137 | 1,158 | 1,171 | 1,171 |
| App | PSS at first line (MB) | 5 | 588 | 601 | 608 | 608 |
| App | 10k inserts, applying (ms) | 5 | 8,710 | 9,338 | 9,471 | 9,471 |
| App | 10k inserts, to next frame (ms) | 5 | 8,871 | 9,490 | 9,625 | 9,625 |
| App | undo of all, applying (ms) | 5 | 8,964 | 9,774 | 9,990 | 9,990 |
| App | undo of all, to next frame (ms) | 5 | 9,287 | 10,075 | 10,286 | 10,286 |
| App | undo steps (n) | 5 | 10,000 | 10,000 | 10,000 | 10,000 |
| App | peak RSS before the script (MB) | 5 | 1,178 | 1,194 | 1,204 | 1,204 |
| App | peak RSS after the script (MB) | 5 | 2,199 | 2,246 | 2,313 | 2,313 |
| App | peak RSS, largest process (MB) | 5 | 1,117 | 1,156 | 1,168 | 1,168 |

Equivalence, from the report (each launch of each side, against the other and against the script):

- text hash after the inserts: rustode 35e9dae1; App 35e9dae1: equal
- text hash after the undo: rustode 58a6184b; App 58a6184b: equal
- chars after the inserts: rustode 1058576; App 1058576: equal
- undo steps: rustode 10000; App 10000; oracle 10000: equal

### Regex search over a monorepo

**2026-11-23-122932-search-monorepo** (status ok)

| Side | Metric | n | min | p50 | p95 | max |
|---|---|---|---|---|---|---|
| rustode | launch to first frame (ms) | 5 | 142 | 144 | 173 | 173 |
| rustode | RSS at first frame (MB) | 5 | 126 | 126 | 126 | 126 |
| rustode | PSS at first frame (MB) | 5 | 99.5 | 99.7 | 100 | 100 |
| rustode | peak RSS before the script (MB) | 5 | 126 | 126 | 126 | 126 |
| rustode | peak RSS after the script (MB) | 5 | 132 | 133 | 133 | 133 |
| rustode | peak RSS, largest process (MB) | 5 | 132 | 133 | 133 | 133 |
| rustode | search, first result (ms) | 5 | 94.5 | 96.7 | 105 | 105 |
| rustode | search, complete (ms) | 5 | 94.5 | 96.7 | 105 | 105 |
| rustode | files with matches (n) | 5 | 3,002 | 3,002 | 3,002 | 3,002 |
| rustode | matches (n) | 5 | 10,436 | 10,436 | 10,436 | 10,436 |
| App | RSS at first line (MB) | 5 | 1,203 | 1,222 | 1,231 | 1,231 |
| App | PSS at first line (MB) | 5 | 561 | 574 | 586 | 586 |
| App | peak RSS before the script (MB) | 5 | 1,266 | 1,274 | 1,282 | 1,282 |
| App | peak RSS after the script (MB) | 5 | 1,607 | 1,621 | 1,627 | 1,627 |
| App | peak RSS, largest process (MB) | 5 | 454 | 465 | 474 | 474 |
| App | search, first result (ms) | 5 | 389 | 420 | 447 | 447 |
| App | search, complete (ms) | 5 | 508 | 532 | 544 | 544 |
| App | files with matches (n) | 5 | 3,002 | 3,002 | 3,002 | 3,002 |
| App | matches (n) | 5 | 10,436 | 10,436 | 10,436 | 10,436 |

Equivalence (rustode, the App and the generator's oracle):

- files with matches: rustode 3002; App 3002; oracle 3002: equal
- matches: rustode 10436; App 10436; oracle 10436: equal

Commands, from `<clone>` (`<project>` holds `01-app/` and `02-package-analysis/`; `<target>`, `<runs>`, `<copies>` and `<marker>` are scratch folders outside the project; each `rustode bench` ran through the machine-exclusive wrapper that holds both build slots and waits for 1,800 MB available):

```sh
export CARGO_TARGET_DIR=<target> CARGO_BUILD_JOBS=3
cargo build --release --locked -p rustode-cli -p rustode-gui
for sc in startup-cold startup-warm idle-rss open-file-1mb open-file-100mb open-file-1gb edit-undo-10k search-monorepo; do
  $CARGO_TARGET_DIR/release/rustode bench $sc \
    --scenarios bench/scenarios.toml --project <project> \
    --out <runs> --app-copy-dir <copies> \
    --gui-bin $CARGO_TARGET_DIR/release/rustode-gui
done
# the control run, App only, no trace:
$CARGO_TARGET_DIR/release/rustode bench startup-warm --no-app-trace --side app \
  --scenarios bench/scenarios.toml --project <project> --out <runs> --app-copy-dir <copies>
# integrity of 01-app/, before and after (nothing printed = intact):
cd <project>/01-app/VSCode-linux-x64 && sha256sum -c --quiet ../../02-package-analysis/manifest-sha256.txt
# nothing in folders 01 to 08 changed during the work:
cd <project> && find 0[1-8]* -newer <marker>
# the tests behind the new scenarios (debug build):
cargo nextest run --workspace --locked
```

| What | Where | Note |
|---|---|---|
| The GUI exits 1 with a hint when no adapter works (G24) | `0004-attachments/g24-check.txt`: `rustode-gui` on the virtual display with the machine's default EGL driver and `RUSTODE_LOG=info`: `ERROR eframe::native::run: Exiting because of error: WGPU error: No suitable graphics adapter found`, then `error: app creation error: the window closed before it presented a frame: no usable graphics adapter`, `exit status: 1` | measured once at `6d7b04e50f98`; finding 0003 measured exit status 0 and no message before the fix. Regression test: `crates/gui/src/run.rs`, `a_loop_that_ended_without_a_frame_is_an_error_with_a_hint` |
| The App's models and services are reachable without keys | `bench/app_internals.cjs`: a DOM event listener of the workbench gives its closure scopes (`Runtime.getProperties` on `[[Scopes]]`); the module scope lists the classes captured by other functions; classes are found by their public methods, instances by `Runtime.queryObjects` | measured on the App copy; found: `TextModel` (`pushEditOperations`, `pushStackElement`, `canUndo`, `getPositionAt`, `undo`), the instantiation service and the `commandService` identifier |
| The trace and `process.hrtime` share a clock | a `performance.mark` in the page and `process.hrtime` read just after: trace `ts` minus the node time was -2.1 ms and -2.3 ms in two trials (the CDP round trip) | measured in two throwaway trials, not in the attachments |
| Search progress bar | `src/vs/workbench/contrib/search/browser/searchView.ts:1799-1810` (`onSearchComplete` calls `progressComplete()` and sets the state to idle); in the App's DOM the sidebar's `.monaco-progress-container` is `active` while the search runs and `done` after | measured in a throwaway trial; the partial results message ("8265 results in 2386 files") appears while it streams, so the first version of the driver read partial counts: the final counts only came from waiting for `done` |
| Find in Files arguments | `src/vs/workbench/contrib/search/browser/searchActionsBase.ts:73-86` (`IFindInFilesArgs`), `:92-151` (`findInFilesCommand`) | code |
| Edit script generator | `crates/workspace/src/bench.rs` (`Mulberry32`, `edit_offsets`); the same generator in `bench/app_internals.cjs`; both pinned to the same six edits (`crates/workspace/tests/bench.rs`, `crates/cli/tests/bench.rs`) | tests |
| The search oracle | `crates/cli/src/bench/fixture.rs` (`planted_in`): counts the planted tokens by hand, not with a regex engine; `crates/cli/tests/bench.rs` (`the_search_scenario_generates_a_tree_whose_counts_an_independent_grep_confirms`) checks it against `grep -rEo` | test |

## How it was measured

What `rustode bench` does for a launch is in [finding 0003](0003-first-app-comparison.md) ("How it was measured"): the verified copy, the isolated profiles, `kwin_wayland --virtual`, no network, software rendering, the QA watchdog. What is new:

**10k inserts, then undo of all** (`edit-undo-10k`). Both editors open the 1 MiB fixture (numbered ASCII lines of 80 bytes). Once the editor is ready (rustode: its first frame; the App: the first line shown) the script runs on the editor's own text model. The script is 10,000 single-letter inserts (`a` to `z`) at offsets from mulberry32 with seed 1: `offset = next % (length + 1)`, `letter = 'a' + next % 26`, the length growing by one per insert; each insert is its own undo step; then every step is undone. rustode: `rustode-gui` reads `RUSTODE_BENCH_SCRIPT` (`rustode_workspace::bench`), the window runs one step per frame, `Workspace::replace` for each insert and `Workspace::undo` until it returns false. The App: `bench/app_internals.cjs` finds the open file's `TextModel` and runs, in the page, `pushStackElement()` then `pushEditOperations` at `getPositionAt(offset)` for each insert, and `undo()` while `canUndo()`. "Applying" is the loop alone; "to next frame" runs from before the first insert (or undo) to the end of the next frame: rustode's return from `RedrawRequested` of the frame that follows the step, the App's second `requestAnimationFrame` callback. `VmHWM` is the kernel's peak resident size, read from `/proc/<pid>/status` before and after the script, summed over the editor's processes (an upper bound of their simultaneous peak).

**Regex search over a monorepo** (`search-monorepo`). The runner generates the tree (`fixture.rs`, seed 1): 5,000 source files of 16 KiB in `src/pkgNN/modNN/` (extensions in the rotation ts, rs, md, ts, rs; every tenth in `src/legacy/`), plus files with matching tokens in `node_modules/`, `generated/` and `logs/*.log`, and a `.gitignore` with `generated/` and `*.log`. Both editors open the folder and run the same search: the regex `\bhandle_[a-z]+_[0-9]{3}\b`, include `**/*.ts,**/*.rs`, exclude `**/legacy/**`, default excludes and ignore files on, case-insensitive. rustode: the window starts the search through the Search view's own code path (`Workspace::start_search`) after its first frame, results stream in and are polled once per frame; "first result" and "complete" are the presents of the frames that first showed a result and the end. The App: the command `workbench.action.findInFiles` with the query as arguments, executed on its command service; "first result" is the first `.monaco-list-row` of the search view, "complete" the sidebar's progress bar going from `active` to `done` with the result message no longer changing, both seen by a `MutationObserver` in the page on `performance.now()`. The generator plants the matches and counts them from the text it wrote; a decoy set (two digits, four digits, no word boundary before) keeps the pattern honest.

**The App's first presented frame** (G22). With `trace = true` in the scenario, each App launch records a Chromium trace (categories `viz` and `blink.user_timing`) from right after the CDP connection; a `MutationObserver` in the page marks (`performance.mark`) the moment the fixture's first line is in the DOM. "Launch to first swap" is the end of the first `Display::DrawAndSwap` that began after the mark, minus the spawn time; "second swap" is the next one. The trace's timestamps and `process.hrtime` are the same clock (the table above shows the offset is the CDP round trip). The trace is stopped 400 ms after the first line, so a launch ends about 0.4 s later than without it.

## Limitations

- **The edit scenario is not like for like.** The App's `pushEditOperations` takes a line and column (the loop calls `getPositionAt` for each insert) and fires content-change events that the open editor, the minimap, bracket matching and tokenization listen to on every edit; rustode's `Workspace::replace` takes a char offset and its GUI redraws once after the whole script. How much of the App's 9,338 ms is the model and how much the view is not separated here (G25). The ratio says what a programmatic bulk edit costs through each editor's own model API with its window open, not how fast either feels when typing. Typing latency is not measured.
- **The two "to next frame" times end at different events.** rustode's is the return from `RedrawRequested` after the present; the App's is the second `requestAnimationFrame` callback, paced by the compositor's frame callbacks on the virtual display. A frame time is quantized by that pacing on the App's side.
- **The search compares two engines and two views.** rustode searches with its own file walker and the `regex` crate in background threads; the App runs ripgrep through its search service and renders a tree. The pattern uses ASCII classes so both mean the same, and the counts match the oracle, but the file walk, the ignore-file handling and the result rendering differ. rustode's first result and complete times are equal here (96.7 ms against 96.7 ms) because the first frame that drew any result was also the first frame after the search ended (both were noted in the same frame), so its streaming latency is not seen and would need a finer clock (G26). The tree is in the page cache from the generation, so the walk is warm for both (a cold-cache search is not measured).
- **The first swap may be one frame early.** The App's `launch to first swap` is the end of the first frame that started drawing after the DOM mark; a frame already in flight with older content would count. The second swap (1,947 ms warm) bounds it from above; the gap between the two is 7.7 ms at p50. rustode's first frame is a present that has returned (`wgpu::Queue::present`), the App's a swap that has returned (`SoftwareRenderer::SwapBuffers`): both end at the editor's own presentation call, neither at the pixels reaching the compositor's output.
- **The trace costs the App some milliseconds.** One control run (App only, `--no-app-trace`, n = 20) against the traced run (n = 20): first paint 1,798 against 1,835 ms, first line 1,891 against 1,928 ms at p50. That is one pair of runs, so the difference includes run-to-run noise (finding 0003's untraced warm run had a first-line p50 of 1,893 ms, close to the control's). The startup rows that carry a swap time are traced, the control is not.
- **The launches differ in more than the editor**, as in finding 0003: the App is started through the QA launcher's `systemd-run --user --scope` (about 5 ms, measured in finding 0003), the GUI directly. The App ran with default settings plus the QA `settings.json`, with its built-in extensions, terminal and source control; rustode has none of them yet, so a leaner idle footprint is partly "does less".
- **Software rendering on a virtual display** for both, as ADR 0006 and ADR 0024 require; absolute times do not hold for a real GPU session.
- **Small samples**: 20 launches per side for startup, 5 for idle, open-file, edits and search, 3 for 1 GB; one machine, one afternoon, other processes of the desktop session ran throughout (two runs were refused by the memory guards for that reason and repeated).
- **In-process `Workspace::open`** numbers have no process start, window or rendering and are not comparable with any App number.
- **Typing latency is not measured** (ADR 0028).

## Next steps

- Typing latency: the owner decides [ADR 0028](../docs/adr/0028-virtual-display-input.md); the scenario then joins list version 4.
- G25: split the App's edit cost between the model and the view (the same edits on a model with no editor attached, and as one batched `pushEditOperations` against rustode's `replace_ranges`).
- G26: the search scenario on a cold cache and on a larger tree, and a finer clock for rustode's streaming (the first result and the end here fall in the same frame).
- G22: check by a screenshot that the first swap already shows the first line, and measure the trace's cost on more than one pair of runs.
- G23 is still open: re-run a QA startup benchmark with the software-rendering environment.
