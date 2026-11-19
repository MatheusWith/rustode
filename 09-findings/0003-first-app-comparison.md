# 0003: How does rustode compare with the App on startup, idle memory and large files? (first comparison, M3)

> **Date:** 2026-11-19 · **Confidence:** measured (every number below comes from a run of `rustode bench`, with its repetitions; the interpretation of "first frame" is approximate, see Limitations) · **Gap:** G15 (partly), opens G22, G23, G24 · **Components:** `rustode bench`, `bench/launch.cjs`, `rustode-gui`, the App (a verified copy), `kwin_wayland --virtual`

## Question

[ADR 0024](../docs/adr/0024-comparison-with-the-app.md) asks for one comparison with the App per milestone, on the same virtual display, from a versioned list of scenarios. M3 brings the GUI, so this is the first run where both editors show a window. What are rustode's and the App's startup to first frame (cold and warm), idle memory, and the cost of opening 1 MB, 100 MB and 1 GB files, and where does each number stop being comparable?

## Scope

Measured: cold and warm startup to first frame, idle memory, opening 1 MB, 100 MB and 1 GB files.

**Not measured: typing latency.** It waits for the owner to accept [ADR 0028](../docs/adr/0028-virtual-display-input.md); until then [AGENTS.md](../AGENTS.md) §11.5.5 forbids injecting a keystroke into either editor. The scenario list (`bench/scenarios.toml`, version 2) has no typing scenario. Also not measured here: the 10k-edit scenario, regex search over a monorepo (ADR 0024's list), which need scenario kinds that do not exist yet.

## Short answer

All numbers are medians (p50) of the runs listed under Evidence, on the virtual display with software rendering for both editors (Mesa llvmpipe), release build of `rustode-gui`, the App from a verified copy. **No ratio of first-frame times is claimed**: the two editors have no common first-frame event (Limitations, G22).

- **Startup.** With a warm file cache, `rustode-gui` presented its first frame 147 ms after the spawn (n = 20, p95 149 ms, max 153 ms). The App's Chromium first contentful paint came at 1,806 ms, the workbench's editor part at 1,796 ms and the fixture's first line at 1,893 ms (n = 20, p95 1,911 ms). With the editor's own files dropped from the page cache: rustode 180 ms (p95 183 ms, one launch at 286 ms), App 2,059 ms to first paint and 2,162 ms to first line (p95 2,215 ms). Dropping the cache costs rustode about 33 ms and the App about 269 ms to the first line (computed from the p50s).
- **Idle memory**, 20 s after the editor is ready (n = 5): rustode, one process, 137 MB `Rss` and 104 MB `Pss`; the App, 7 processes, 1,339 MB `Rss` and 769 MB `Pss` (p95 1,440 and 868 MB). The `Pss` figure is the one that adds up; the App's `Rss` over-counts shared pages. The App also loads built-in extensions, a terminal and source control that rustode does not have (Limitations).
- **Large files.** Both editors opened 1 MB, 100 MB and 1 GB files without error. Compared with their own startup, opening costs rustode about +8 ms (1 MB), +163 ms (100 MB) and +2.9 s (1 GB) to the first frame, and the App about +17 ms, +524 ms and +4.3 s to the first line shown (computed: each scenario's p50 minus the warm startup's p50). Memory with the file open, at the first frame or first line (`Rss`/`Pss`): 1 MB 140/111 MB against 1,312/604 MB; 100 MB 252/222 MB against 1,319/767 MB; 1 GB 1,309/1,276 MB against 2,308/1,739 MB.
- **In-process `Workspace::open`**, no window: 1.2 ms (1 MB, n = 20), 165 ms (100 MB, n = 10), 1.64 s (1 GB, n = 3); not comparable with any App number.
- **Not measured: typing latency**, waiting on ADR 0028.
- **Method findings that matter beyond this run:** without extra settings the App on the virtual display rendered on the machine's NVIDIA GPU, not in software (G23), and `rustode-gui` exited with status 0 and no message when it found no usable graphics adapter (G24).

## Evidence

Every run is one `rustode bench <scenario>` from the clone at `<clone>` (cwd), run on 2026-11-19 (UTC stamps are in the run ids). The five scenarios other than `startup-cold` ran with `rustode` at `82e0786c4741` (`git` clean); `startup-cold` was run again at `e5c8f2b8d561`, which only retries the cache eviction when part of the main file stays cached (the first cold run left 28 to 29% of `codium` resident in 6 of its 20 App launches, and its folder was deleted); the later commits of this branch change documents only. Release builds (`cargo build --release --locked -p rustode-cli -p rustode-gui`); `rustode-gui` sha256 `a9423e64d49551815958efd80c29bf3972d0c2ea89ce43d1452ae5390a01b4a4`; list version 2 (`bench/scenarios.toml`). Machine: 11th Gen Intel Core i5-11400H, 12 CPUs, 7.7 GB RAM, Linux 7.3.4-arch1-2, KWin 6.8.2 virtual, Node 24.22.1, playwright-core 1.61.0-alpha-2026-06-04. App: VSCodium 1.147.0 (`22d8dccf0f29`), intact before, in the copy and after each run ("ok: 3008 listed files match, 0 files outside the manifest" in every report), 0 files added to the copy in every run (`app/copy-writes.txt`).

Raw outputs (small files only: `report.txt`, `report.json`, each driver's `results.json`, `driver-config.json` and `driver.log`, the first launch's `app.log`, the battery log) are in [`0003-attachments/runs/`](0003-attachments/runs/) (the home folder is abbreviated to `~` in the copies). The run folders with fixtures, screenshots and watchdog series stay outside the repository in `<runs>/` (fixtures deleted; the 1 GiB one is regenerated by the command).

Times are in ms and memory in MB. `n` is the number of recorded launches (warm-ups excluded); p95 is nearest-rank, so with n ≤ 20 it is close to the maximum.

### Startup, first frame

Cold file cache (`startup-cold`):

**2026-11-19-174544-startup-cold** (status ok)

| Side | Metric | n | min | p50 | p95 | max |
|---|---|---|---|---|---|---|
| rustode | launch to first frame (ms) | 20 | 179 | 180 | 183 | 286 |
| rustode | main to first frame (ms) | 20 | 172 | 174 | 177 | 279 |
| App | launch to first paint (ms) | 20 | 2,029 | 2,059 | 2,118 | 2,125 |
| App | launch to workbench (ms) | 20 | 2,020 | 2,046 | 2,109 | 2,114 |
| App | launch to first line shown (ms) | 20 | 2,121 | 2,162 | 2,215 | 2,221 |
| App | shutdown after SIGTERM (ms) | 20 | 247 | 324 | 10,588 | 10,588 |
| rustode | RSS at first frame / first line (MB) | 20 | 130 | 131 | 131 | 131 |
| App | RSS at first frame / first line (MB) | 20 | 1,082 | 1,097 | 1,122 | 1,124 |
| rustode | PSS at first frame / first line (MB) | 20 | 102 | 103 | 103 | 103 |
| App | PSS at first frame / first line (MB) | 20 | 564 | 578 | 593 | 595 |

Warm file cache (`startup-warm`):

**2026-11-19-170717-startup-warm** (status ok)

| Side | Metric | n | min | p50 | p95 | max |
|---|---|---|---|---|---|---|
| rustode | launch to first frame (ms) | 20 | 146 | 147 | 149 | 153 |
| rustode | main to first frame (ms) | 20 | 144 | 145 | 147 | 150 |
| App | launch to first paint (ms) | 20 | 1,776 | 1,806 | 1,820 | 1,828 |
| App | launch to workbench (ms) | 20 | 1,767 | 1,796 | 1,808 | 1,818 |
| App | launch to first line shown (ms) | 20 | 1,865 | 1,893 | 1,911 | 1,923 |
| App | shutdown after SIGTERM (ms) | 20 | 181 | 308 | 330 | 330 |
| rustode | RSS at first frame / first line (MB) | 20 | 132 | 134 | 135 | 135 |
| App | RSS at first frame / first line (MB) | 20 | 1,201 | 1,245 | 1,284 | 1,293 |
| rustode | PSS at first frame / first line (MB) | 20 | 102 | 104 | 104 | 104 |
| App | PSS at first frame / first line (MB) | 20 | 585 | 598 | 615 | 619 |

The App did not exit within 10 s of SIGTERM, and was killed, in 10 of its 20 cold launches (its shutdown p95 and max are that wait, plus 0.5 s); in none of the 20 warm launches, and in 1 of 20 in the first cold run that was deleted. Shutdown is therefore not a stable metric of the App here (the QA harness's finding P12 is about the same hang); rustode shut down in about 60 ms every time. The shutdown does not affect the start-up times above, which are read before it.

### Idle memory

**2026-11-19-171021-idle-rss** (status ok)

| Side | Metric | n | min | p50 | p95 | max |
|---|---|---|---|---|---|---|
| rustode | idle RSS, summed (MB) | 5 | 135.3 | 136.9 | 137.2 | 137.2 |
| rustode | idle PSS, summed (MB) | 5 | 102.0 | 103.7 | 104.0 | 104.0 |
| App | idle RSS, summed (MB) | 5 | 1328.7 | 1338.9 | 1439.8 | 1439.8 |
| App | idle PSS, summed (MB) | 5 | 759.7 | 769.1 | 867.6 | 867.6 |
| rustode | peak RSS during launch (MB) | 5 | 135 | 137 | 137 | 137 |
| App | peak RSS during launch (MB) | 5 | 1,372 | 1,399 | 1,479 | 1,479 |

The App had 7 processes in every launch, rustode 1 (`idle_procs` in `results.json`).

### Opening files

**2026-11-19-171524-open-file-1mb** (status ok)

| Side | Metric | n | min | p50 | p95 | max |
|---|---|---|---|---|---|---|
| rustode | Workspace::open in-process (ms) | 20 | 1.066 | 1.212 | 1.295 | 1.317 |
| rustode | GUI launch to first frame (ms) | 5 | 154 | 155 | 156 | 156 |
| App | launch to first paint (ms) | 5 | 1,770 | 1,798 | 1,805 | 1,805 |
| App | launch to first line shown (ms) | 5 | 1,884 | 1,910 | 1,928 | 1,928 |
| rustode | GUI RSS at first frame (MB) | 5 | 138 | 140 | 141 | 141 |
| App | RSS at first line (MB) | 5 | 1,297 | 1,312 | 1,331 | 1,331 |
| rustode | GUI PSS at first frame (MB) | 5 | 109 | 111 | 111 | 111 |
| App | PSS at first line (MB) | 5 | 592 | 604 | 618 | 618 |

**2026-11-19-171725-open-file-100mb** (status ok)

| Side | Metric | n | min | p50 | p95 | max |
|---|---|---|---|---|---|---|
| rustode | Workspace::open in-process (ms) | 10 | 128.308 | 165.248 | 174.559 | 174.559 |
| rustode | GUI launch to first frame (ms) | 5 | 309 | 310 | 316 | 316 |
| App | launch to first paint (ms) | 5 | 1,773 | 1,827 | 1,886 | 1,886 |
| App | launch to first line shown (ms) | 5 | 2,361 | 2,417 | 2,503 | 2,503 |
| rustode | GUI RSS at first frame (MB) | 5 | 252 | 252 | 254 | 254 |
| App | RSS at first line (MB) | 5 | 1,266 | 1,319 | 1,352 | 1,352 |
| rustode | GUI PSS at first frame (MB) | 5 | 222 | 222 | 224 | 224 |
| App | PSS at first line (MB) | 5 | 714 | 767 | 796 | 796 |

**2026-11-19-173928-open-file-1gb** (status ok)

| Side | Metric | n | min | p50 | p95 | max |
|---|---|---|---|---|---|---|
| rustode | Workspace::open in-process (ms) | 3 | 1498.853 | 1641.266 | 1729.158 | 1729.158 |
| rustode | GUI launch to first frame (ms) | 3 | 2,090 | 3,023 | 3,143 | 3,143 |
| App | launch to first paint (ms) | 3 | 1,772 | 1,792 | 2,086 | 2,086 |
| App | launch to first line shown (ms) | 3 | 6,199 | 6,232 | 6,744 | 6,744 |
| rustode | GUI RSS at first frame (MB) | 3 | 1,293 | 1,309 | 1,310 | 1,310 |
| App | RSS at first line (MB) | 3 | 2,264 | 2,308 | 2,324 | 2,324 |
| rustode | GUI PSS at first frame (MB) | 3 | 1,260 | 1,276 | 1,278 | 1,278 |
| App | PSS at first line (MB) | 3 | 1,698 | 1,739 | 1,757 | 1,757 |

note [open-file-1mb]: 5 of 6 launches did not exit within 10 s of SIGTERM and were killed with SIGKILL: warmup-1, rep-1, rep-2, rep-3, rep-5; their shutdown times include the 10 s wait.

note [open-file-100mb]: 6 of 6 launches did not exit within 10 s of SIGTERM and were killed with SIGKILL: warmup-1, rep-1, rep-2, rep-3, rep-4, rep-5; their shutdown times include the 10 s wait.

note [open-file-1gb]: 3 of 3 launches did not exit within 10 s of SIGTERM and were killed with SIGKILL: rep-1, rep-2, rep-3; their shutdown times include the 10 s wait.

note [startup-cold]: 10 of 20 launches did not exit within 10 s of SIGTERM and were killed with SIGKILL: rep-1, rep-2, rep-3, rep-4, rep-5, rep-6, rep-7, rep-12, rep-13, rep-20; their shutdown times include the 10 s wait.

In the three open-file runs the driver stops the App as soon as it has seen the first line, so most App launches needed SIGKILL (5 of 6, 6 of 6 and 3 of 3 `needed_sigkill`; ROADMAP "Next steps" item 4.2); their shutdown times are not reported here. The "peak RSS" rows of the reports come from the watchdog's 1 s samples and are 1 to 3 samples per launch, so this finding uses the RSS read right after the end point instead.

Commands, from `<clone>` (`<clone>` is the agent's clone, `<project>` the project root that holds `01-app/` and `02-package-analysis/`; `<target>`, `<runs>`, `<copies>` and `<marker>` are scratch folders outside the project):

```sh
export CARGO_TARGET_DIR=<target> CARGO_BUILD_JOBS=3
cargo build --release --locked -p rustode-cli -p rustode-gui
for sc in startup-cold startup-warm idle-rss open-file-1mb open-file-100mb open-file-1gb; do
  $CARGO_TARGET_DIR/release/rustode bench $sc \
    --scenarios bench/scenarios.toml --project <project> \
    --out <runs> --app-copy-dir <copies> \
    --gui-bin $CARGO_TARGET_DIR/release/rustode-gui
done
# integrity of 01-app/, before and after (nothing printed = intact):
cd <project>/01-app/VSCode-linux-x64 && sha256sum -c --quiet ../../02-package-analysis/manifest-sha256.txt
# nothing in folders 01 to 08 changed during the work:
cd <project> && find 0[1-8]* -newer <marker>
# the scope's overhead:
hyperfine -N --warmup 3 --runs 30 'systemd-run --user --scope --quiet -p MemoryMax=3G -p MemorySwapMax=1G -- /usr/bin/true' '/usr/bin/true'
```

| What | Where | Note |
|---|---|---|
| The App chose the GPU without extra settings | first trial run (folder deleted): `SystemInfo.getInfo` over CDP, `glRenderer: ANGLE (NVIDIA Corporation, NVIDIA GeForce RTX 3050 Laptop GPU…)`, `gpu_compositing: enabled` | measured once, in a trial; not in the attachments, so it is a lead for G23, not a recorded result |
| The App rendered in software in every recorded run | `0003-attachments/runs/<scenario>/app/results.json`, key `gpu`: `glRenderer: ANGLE (Mesa, llvmpipe (LLVM 23.1.4 256 bits), OpenGL ES 3.2 Mesa 26.2.7-arch1.1)`, `gpu_compositing: disabled_software` | measured |
| The GUI rendered in software | `0003-attachments/runs/<scenario>/gui/rep-1-app.log`: `Software rasterizer detected … llvmpipe (LLVM 23.1.4, 256 bits)` | measured |
| The GUI exits silently without a usable adapter | `RUSTODE_LOG=debug` with the machine's EGL driver: `ERROR eframe::native::run: Exiting because of error: WGPU error: No suitable graphics adapter found … gl not compatible with provided surface`, exit status 0, 70 ms after start | measured once, interactively (not in the attachments); G24 |
| First frame definition | `crates/gui/src/run.rs` (`Timed::window_event`), `crates/trace/src/first_frame.rs`, `crates/gui/src/main.rs` (`open` before `run`) | code |
| Cold files really left the cache | `cache_reference_resident_pct` and `evict_attempts` in `0003-attachments/runs/startup-cold/{app,gui}/results.json`: 0% for all 40 launches; 2 App launches needed 4 attempts | measured |
| Memory floor | the driver waits up to 60 s for `MemAvailable` ≥ 2,000 MB before each launch; no launch was refused or aborted by the watchdog in the six recorded runs (`status ok`) | measured; `battery.log` lists `avail=` after each run |

## How it was measured

`rustode bench <scenario>` ran each scenario once, from a clone of the repository at the commit named below, one scenario at a time, each through the machine-exclusive wrapper (holds both build slots) (it holds both build slots, so no agent build ran, and waits for 1,800 MB of `MemAvailable`). A run:

1. checks `01-app/VSCode-linux-x64` against the manifest (`sha256sum -c --quiet`, and no file outside it), copies it to `<copies>/rustode-bench-app.<random>/` with `cp -a`, checks the copy the same way, runs the App only from the copy, deletes the copy and checks `01-app/` again (AGENTS.md §0.6, §11.5.3);
2. generates the fixture (numbered ASCII lines of 80 bytes, deterministic; 4 KiB for the startup and idle scenarios);
3. launches `rustode-gui` (release build) with the fixture on its command line, then the App (`codium`, from the copy) with the same fixture, each launch with a fresh isolated profile (`--user-data-dir`, `--extensions-dir`, `--shared-data-dir` for the App; `RUSTODE_USER_DIR` and the `XDG_*` folders for the GUI), inside `run_virtual.sh bwrap --dev-bind / / --unshare-net --die-with-parent node bench/launch.cjs <config>`: the `kwin_wayland --virtual` display, a private D-Bus and no network. The App has `NODE_DISABLE_COMPILE_CACHE=1`. The QA watchdog (`sampler.py`, abort below 700 MB `MemAvailable`) watches every launch. One editor instance at a time;
4. for the open-file scenarios, last, calls `Workspace::open` in the `rustode` process itself (an in-process figure with no window, kept from ticket 14).

**Software rendering for both.** The virtual display has no GPU, but without extra settings the App chose the machine's NVIDIA GPU (`SystemInfo.getInfo` over CDP: `gpu_compositing: enabled`, `glRenderer: ANGLE (NVIDIA Corporation, NVIDIA GeForce RTX 3050 Laptop GPU/PCIe/SSE2, …)`, in a first trial run whose folder was deleted), and rustode's wgpu found only the NVIDIA GL adapter and then could not use it with the Wayland surface. So the runner sets the same variables for both editors (`plan.rs: software_rendering_env`): `LIBGL_ALWAYS_SOFTWARE=1`, `GALLIUM_DRIVER=llvmpipe`, `__EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json`, `__GLX_VENDOR_LIBRARY_NAME=mesa`, `VK_ICD_FILENAMES=/nonexistent`; the GUI also gets `WGPU_BACKEND=gl`. This machine has no Vulkan software driver (lavapipe). With them, the App reports `glRenderer: ANGLE (Mesa, llvmpipe (LLVM 23.1.4 256 bits), …)` and `gpu_compositing: disabled_software`; the GUI reports `Software rasterizer detected … llvmpipe` (`gui/rep-1/app.log` of each run). The report prints both.

**Definitions** (the same ones in every report, under "what the results measure"):

- *Launch to first frame* (rustode): from spawning `rustode-gui` to the arrival, in the driver, of the line `rustode-gui: first frame presented N us after main`, which the GUI prints (when `RUSTODE_REPORT_FIRST_FRAME` is set) right after eframe returns from its first `RedrawRequested`, that is, after it painted and called `wgpu::Queue::present` (`crates/gui/src/run.rs`, `Timed::window_event`; `crates/trace/src/first_frame.rs`). The fixture is already open in that frame: `main` opens its arguments before it creates the window (`crates/gui/src/main.rs`, `open`). `main to first frame` is the GUI's own `Instant` from `rustode_trace::init_from_env`. The two differ by 2 to 3 ms in every run: exec, dynamic linking and the pipe.
- *Launch to first paint* (App): from the spawn to Chromium's `first-contentful-paint` entry in the workbench page, converted to a wall-clock time (`performance.timeOrigin + startTime`) and subtracted from the wall-clock time of the spawn.
- *Launch to workbench* (App): the QA startup benchmark's "workbench ready": `.monaco-workbench .part.editor` exists, polled every 25 ms.
- *Launch to first line shown* (App): the fixture's first line rendered in an editor `.view-line`, polled every 10 ms over CDP (50 ms for 1 GB). No input is sent to the App.
- *Cold*: before each launch, every file of the App copy (3,008 files, 694 MiB) or the GUI binary (38 MiB) is dropped from the page cache with `posix_fadvise(DONTNEED)` after a `sync` (`bench/evict.py`), and `fincore` on `codium` or `rustode-gui` reads 0% resident afterwards (field `cache_reference_resident_pct` in each launch of `results.json`). Libraries shared with other programs (libc, Mesa, Wayland) stay cached, and so does the profile-independent state of the OS. *Warm*: nothing is dropped; the previous launch has just read the files. The profile is fresh in both.
- *Idle memory*: `settle_s = 20` seconds after the end point, with nothing typed, the `Rss` and `Pss` of every process of the editor, summed, from `/proc/<pid>/smaps_rollup` (the App has several processes, rustode one). `Rss` counts a shared page once per process, so it over-counts the App; `Pss` splits shared pages among their users and adds up.
- *Memory at the end point* (open-file scenarios): the same sums read right after the first frame (rustode) or the first line (App).
- Percentiles are nearest-rank, the definition of `LatencyRecorder` (`crates/cli/src/bench/report.rs`, test `percentiles_match_the_trace_crate`). With n = 10 or 5 the p95 is the maximum.

## Limitations

- **"First frame" is not one event.** rustode's is a wgpu present returning; the App has no such event. The App's two nearest events are Chromium's first contentful paint and the first fixture line seen in the DOM by polling, and they bracket rustode's, they do not equal it. No ratio is claimed anywhere; the report says so in its notes (G22).
- **The two launches differ in more than the editor.** The App is started through the QA launcher's `systemd-run --user --scope -p MemoryMax=3G -p MemorySwapMax=1G`; `rustode-gui` is started directly by the driver. The scope's own cost is not subtracted. Measured on 2026-11-19: `hyperfine -N --warmup 3 --runs 30` of `systemd-run --user --scope --quiet -p MemoryMax=3G -p MemorySwapMax=1G -- /usr/bin/true` took 5.5 ms ± 0.7 ms (4.4 to 7.3 ms), against 0.3 ms for `/usr/bin/true` alone (run through `exclusive`, release build irrelevant). About 5 ms is added to every App time and to none of rustode's.
- **Cold means "the editor's own files are out of the page cache"**, not a reboot: libc, Mesa, the Wayland client libraries, the kernel's dentry caches and the CPU caches are warm for both. A file-cache-cold start of the real session would be slower for both.
- **Software rendering on a virtual display** (llvmpipe on 12 CPUs), the same for both, as ADR 0006 and ADR 0024 require. Absolute times do not hold for a real GPU session, and the ratio between the editors may not either: Chromium and wgpu use the GPU differently.
- **The App ran with default settings plus the QA `settings.json`** (`bench/app/settings.json`): built-in extensions, the workbench's services and the extension host all start. rustode has none of them yet (the Gap Analysis in the ROADMAP lists what it lacks), so a leaner idle footprint is partly "does less", not only "is better built". The App also holds a window with a terminal, search, source control and the explorer; the GUI shows an editor, tabs, explorer and status bar.
- **Small samples.** 20 launches per side for startup, 5 for idle and open-file at 1 MB and 100 MB, 3 for 1 GB; 20 in-process opens at 1 MB and 10 at 100 MB. With n = 5 the p95 is the maximum. One machine, one day, no repetition of the whole battery on another day.
- **Other load on the machine.** Two other agents built and tested in parallel before and after each run; each run held both build slots and started only with 1,800 MB available, but background daemons (the desktop session, `opencode`) ran throughout.
- **The in-process `Workspace::open`** numbers have no process start, window or rendering and are not comparable with any App number.
- **Typing latency, the 10k-edit scenario and the regex-search scenario are not measured** (see Scope).
- **A GUI whose first frame arrives after its 1 GB document is loaded still has not drawn text for a document of that size on any path other than the visible lines**; this run does not check how fast scrolling or editing is in such a file.

## Next steps

- Typing latency: the owner decides [ADR 0028](../docs/adr/0028-virtual-display-input.md); the scenario then joins list version 3.
- A common first-frame event for the App: G22.
- Re-run one QA startup benchmark with software rendering to see whether the 2026-11-13 and 2026-11-16 QA numbers used the GPU: G23.
- `rustode-gui` exits with status 0 and no message when it finds no graphics adapter: G24.
- The 10k-edit and regex-search scenarios of ADR 0024, and a scenario for scrolling a large file, need new scenario kinds in `rustode bench`.
