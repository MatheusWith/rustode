'use strict';
/**
 * rustode bench: the launch driver, for the App and for the rustode GUI.
 *
 * Usage: node launch.cjs <driver-config.json>
 *
 * `rustode bench` writes the config and runs this script inside the QA
 * harness's virtual display and a network namespace without network:
 *
 *   run_virtual.sh bwrap --dev-bind / / --unshare-net --die-with-parent node launch.cjs <config>
 *
 * so the editor and this driver share a loopback interface (for CDP) and
 * nothing else. rustode builds every command this script runs (`app_command`,
 * `sampler_command`); this script fills in the per-launch placeholders
 * {profile}, {profile_re}, {rep_dir} and {pid}, times the launch and cleans up.
 *
 * `cfg.target` is `app` (VSCodium, driven over CDP; no input is sent) or `gui`
 * (`rustode-gui`, which prints its first presented frame on stderr when
 * RUSTODE_REPORT_FIRST_FRAME is set). `cfg.measure` is `open`, `startup` or
 * `idle`; `cfg.cache` is `cold` (the editor's own files are dropped from the
 * page cache before each launch, bench/evict.py) or `warm`.
 *
 * Each launch (warm-ups first, not recorded) gets a fresh isolated profile:
 *   1. drop the editor's files from the page cache if `cache` is `cold`;
 *   2. refuse below `min_available_mb` MemAvailable (after waiting up to 60 s for memory to settle);
 *   3. spawn the editor with the fixture on its command line, then the QA
 *      harness's watchdog (collectors/sampler.py), which kills it below
 *      700 MB MemAvailable or under sustained memory pressure;
 *   4. time, from the spawn (App): the CDP endpoint answering, Chromium's first
 *      contentful paint, the workbench's editor part existing, and the
 *      fixture's first line rendered in an editor `.view-line`, each read over
 *      CDP (no input is sent). Time, from the spawn (GUI): the line the GUI
 *      prints after its first present;
 *   5. read Rss and Pss of every process of the editor right after the end point
 *      (`ready_*`); `idle`: wait `settle_s`, then read the Rss and Pss of every process of
 *      the editor from /proc/<pid>/smaps_rollup;
 *   6. screenshot (App), SIGTERM the main process (SIGKILL after 10 s, as the
 *      QA startup benchmark does), wait for the watchdog, delete the profile.
 *
 * With `timeout_is_result`, a launch that never reaches the end point (or whose
 * editor exits first) is recorded with `outcome` and `outcome_detail` and the
 * run goes on: for a large file, "the editor cannot open it" is a result.
 *
 * Results go to <out_dir>/results.json after every launch.
 * Exit codes: 0 ok, 1 error, 4 refused by a guard, 5 aborted by the watchdog.
 */
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawn, spawnSync } = require('child_process');

const cfg = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
// The QA harness's clock and polling helpers (process.hrtime: monotonic, ms).
const { sleep, now, poll } = require(path.join(cfg.qa_root, 'suite/scenarios/lib.cjs'));
const chromium = cfg.target === 'app' ? require(cfg.playwright_core).chromium : null;

const EXIT = { ok: 0, error: 1, refused: 4, aborted: 5 };
const round1 = ms => Math.round(ms * 10) / 10;
const reEscape = s => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
const profilePattern = profile => `[-]-user-data-dir=${reEscape(profile)}/ud`;
const alive = launch => (launch.target === 'app'
	? spawnSync('pgrep', ['-f', '--', profilePattern(launch.profile)]).status === 0
	: treePids(launch.app.pid).some(pid => fs.existsSync(`/proc/${pid}`)));
const memAvailableMb = () =>
	Math.floor(+fs.readFileSync('/proc/meminfo', 'utf8').match(/MemAvailable:\s+(\d+)/)[1] / 1024);

class Aborted extends Error { }
class NoEndPoint extends Error { }

/** `rootPid` and its descendants, from the ppid of every /proc/<pid>/stat. */
function treePids(rootPid) {
	const parent = new Map();
	for (const name of fs.readdirSync('/proc')) {
		if (!/^\d+$/.test(name)) { continue; }
		try {
			const stat = fs.readFileSync(`/proc/${name}/stat`, 'utf8');
			parent.set(+name, +stat.slice(stat.lastIndexOf(')') + 2).split(' ')[1]);
		} catch { /* gone */ }
	}
	const found = [rootPid];
	for (let i = 0; i < found.length; i++) {
		for (const [pid, ppid] of parent) { if (ppid === found[i] && !found.includes(pid)) { found.push(pid); } }
	}
	return found.filter(pid => parent.has(pid));
}

/** The pids of the editor's processes: the App's by profile, the GUI's by tree. */
function editorPids(launch) {
	if (launch.target === 'gui') { return treePids(launch.app.pid); }
	return (spawnSync('pgrep', ['-f', '--', profilePattern(launch.profile)], { encoding: 'utf8' }).stdout || '')
		.split('\n').filter(Boolean).map(Number);
}

/** Rss and Pss (kB) summed over the editor's processes, from smaps_rollup. */
function memoryOf(launch) {
	let rss = 0, pss = 0, procs = 0;
	for (const pid of editorPids(launch)) {
		try {
			const text = fs.readFileSync(`/proc/${pid}/smaps_rollup`, 'utf8');
			rss += +text.match(/^Rss:\s+(\d+)/m)[1];
			pss += +text.match(/^Pss:\s+(\d+)/m)[1];
			procs++;
		} catch { /* gone, or not readable */ }
	}
	return { rss_mb: round1(rss / 1024), pss_mb: round1(pss / 1024), procs };
}

/** Resident share (percent) of the editor's main file in the page cache, from `fincore`. */
function residentPct() {
	const res = spawnSync('fincore', ['--bytes', '--noheadings', '--output', 'RES,SIZE', cfg.cache_reference],
		{ encoding: 'utf8' }).stdout || '';
	const [resident, size] = res.trim().split(/\s+/).map(Number);
	return size > 0 ? round1(100 * resident / size) : null;
}

/**
 * Drops the editor's files from the page cache. Pages of a process that is still exiting stay
 * mapped, so it tries again (up to 5 times, 300 ms apart) until the main file is under 1 %
 * resident, and records what is left.
 */
async function evict(rec) {
	for (let attempt = 1; attempt <= 5; attempt++) {
		const run = spawnSync('python3', ['-I', cfg.evict_script, ...cfg.evict_paths], { encoding: 'utf8' });
		rec.evicted = (run.stdout || run.stderr || '').trim();
		rec.evict_attempts = attempt;
		rec.cache_reference_resident_pct = residentPct();
		if (rec.cache_reference_resident_pct === null || rec.cache_reference_resident_pct < 1) { return; }
		await sleep(300);
	}
}

/** Why this process must not launch the App here (finding 0002's guards, plus no network). */
function guardProblems() {
	const problems = [];
	const display = process.env.WAYLAND_DISPLAY || '';
	const bus = process.env.DBUS_SESSION_BUS_ADDRESS || '';
	if (!display.startsWith('qa-virt-')) { problems.push(`WAYLAND_DISPLAY="${display}" is not a qa-virt-* socket`); }
	if (bus === '' || bus.includes('/run/user/')) { problems.push(`DBUS_SESSION_BUS_ADDRESS="${bus}" is not a private bus`); }
	if (process.env.DISPLAY) { problems.push('DISPLAY is set'); }
	const external = Object.entries(os.networkInterfaces())
		.filter(([, addrs]) => addrs.some(a => !a.internal)).map(([name]) => name);
	if (external.length) { problems.push(`network interfaces besides loopback: ${external.join(', ')}`); }
	return problems;
}

/** The environment without variables that would change how the editor starts, plus the config's. */
function editorEnv(profile) {
	const env = { ...process.env };
	for (const key of Object.keys(env)) {
		if (key.startsWith('ELECTRON_') || key.startsWith('VSCODE_') || key === 'NODE_OPTIONS') { delete env[key]; }
	}
	// AGENTS.md section 11.5.3: the App never writes a Node compile cache.
	if (cfg.target === 'app') { env.NODE_DISABLE_COMPILE_CACHE = '1'; }
	for (const [key, value] of Object.entries(cfg.env || {})) {
		env[key] = String(value).replaceAll('{profile}', profile);
	}
	return env;
}

/** Runs in the page: whether an editor shows `want` as one of its rendered lines. */
function firstLineShown(want) {
	// Monaco renders spaces as U+00A0 (src/vs/editor/common/viewLayout/viewLineRenderer.ts:1162-1163).
	for (const line of document.querySelectorAll('.part.editor .monaco-editor .view-lines .view-line')) {
		if (line.textContent.replace(/ /g, ' ').trimEnd() === want) { return true; }
	}
	return false;
}

/** Polls `fn` until it returns a truthy value; returns it and the ms since `t0`. */
async function until(launch, what, fn, everyMs) {
	const remaining = launch.t0 + cfg.timeout_ms - now();
	const r = await poll(async () => {
		if (launch.samplerExit === 3) { throw new Aborted('watchdog'); }
		if (launch.appExit !== null) { throw new NoEndPoint(`exited (${launch.appExit}) before ${what}`); }
		return fn();
	}, Math.max(remaining, 0), everyMs);
	if (!r.v) { throw new NoEndPoint(`timeout: no ${what} within ${cfg.timeout_ms} ms of the launch`); }
	return { v: r.v, ms: round1(now() - launch.t0) };
}

/** Runs in the page: Chromium's first contentful paint as a wall-clock time (ms since the epoch). */
function firstContentfulPaintEpochMs() {
	const entry = performance.getEntriesByType('paint').find(e => e.name === 'first-contentful-paint');
	return entry ? performance.timeOrigin + entry.startTime : null;
}

/** Runs in the page: what the workbench says when it cannot open the file. */
function workbenchComplaint() {
	const text = sel => [...document.querySelectorAll(sel)].map(e => e.textContent.trim()).filter(Boolean).join(' | ');
	return text('.notifications-toasts .notification-list-item-message, .monaco-dialog-box .dialog-message-text, .editor-placeholder, .monaco-editor-pane-placeholder')
		.slice(0, 300);
}

/** Which GPU path Chromium chose (once per run), so the report can say whether it rendered in software. */
async function recordGpu(browser) {
	try {
		const session = await browser.newBrowserCDPSession();
		const info = await session.send('SystemInfo.getInfo');
		const aux = info.gpu.auxAttributes || {};
		const status = info.gpu.featureStatus || {};
		out.gpu = {
			gl_renderer: aux.glRenderer, gl_vendor: aux.glVendor, gl_implementation: aux.glImplementationParts,
			skia_backend: aux.skiaBackendType, display_type: aux.displayType,
			gpu_compositing: status.gpu_compositing, opengl: status.opengl, vulkan: status.vulkan,
		};
		await session.detach();
	} catch (e) { out.gpu = { error: String(e.message || e).split('\n')[0] }; }
}

async function measureApp(launch, rec) {
	const endpoint = `http://127.0.0.1:${cfg.cdp_port}`;
	rec.spawn_to_cdp_ms = (await until(launch, 'CDP endpoint', async () => {
		try { return (await fetch(`${endpoint}/json/version`)).ok; } catch { return false; }
	}, 25)).ms;
	const browser = await chromium.connectOverCDP(endpoint);
	try {
		if (!out.gpu) { await recordGpu(browser); }
		const pages = () => browser.contexts().flatMap(c => c.pages());
		const page = (await until(launch, 'workbench page', () => pages().find(p => /workbench/.test(p.url())), 25)).v;
		try {
			rec.spawn_to_workbench_ms = (await until(launch, 'editor part', () => page.evaluate(
				() => !!document.querySelector('.monaco-workbench .part.editor')).catch(() => false), 25)).ms;
			rec.spawn_to_first_line_ms = (await until(launch, "fixture's first line", () => page.evaluate(
				firstLineShown, cfg.first_line).catch(() => false), cfg.poll_ms)).ms;
		} catch (e) {
			if (e instanceof NoEndPoint) { rec.complaint = await page.evaluate(workbenchComplaint).catch(() => ''); }
			await page.screenshot({ path: path.join(launch.dir, 'no-first-line.png') }).catch(() => { });
			throw e;
		} finally {
			// Chromium's paint timing is a wall-clock time; the spawn's wall-clock time is launch.wall0.
			const fcp = await page.evaluate(firstContentfulPaintEpochMs).catch(() => null);
			if (fcp !== null) { rec.spawn_to_first_paint_ms = round1(fcp - launch.wall0); }
		}
		readyMemory(launch, rec);
		if (cfg.measure === 'idle') { await settle(launch, rec); }
		await page.screenshot({ path: path.join(launch.dir, 'first-line.png') }).catch(() => { });
	} finally {
		await browser.close().catch(() => { }); // disconnects; the App keeps running
	}
}

/** The GUI prints "first frame presented N us after main" after its first present. */
async function measureGui(launch, rec) {
	await until(launch, 'the first presented frame', () => launch.firstFrame, 5);
	rec.spawn_to_first_frame_ms = launch.firstFrame.at;
	rec.first_frame_after_main_ms = round1(launch.firstFrame.us / 1000);
	readyMemory(launch, rec);
	if (cfg.measure === 'idle') { await settle(launch, rec); }
}

/** Rss and Pss right after the end point: the memory a file costs once it is open (the watchdog's 1 s samples are too coarse). */
function readyMemory(launch, rec) {
	Object.assign(rec, Object.fromEntries(Object.entries(memoryOf(launch)).map(([k, v]) => [`ready_${k}`, v])));
}

/** Waits `settle_s` after the editor is ready, then reads its memory. */
async function settle(launch, rec) {
	const end = now() + cfg.settle_s * 1000;
	while (now() < end) {
		if (launch.samplerExit === 3) { throw new Aborted('watchdog'); }
		if (launch.appExit !== null) { throw new NoEndPoint(`exited (${launch.appExit}) while idle`); }
		await sleep(250);
	}
	Object.assign(rec, Object.fromEntries(Object.entries(memoryOf(launch)).map(([k, v]) => [`idle_${k}`, v])));
}

/** The App's main process: the one with the profile and no `--type=` (QA startup benchmark). */
function mainPid(launch) {
	if (launch.target === 'gui') { return launch.app.pid; }
	const pids = (spawnSync('pgrep', ['-f', '--', profilePattern(launch.profile)], { encoding: 'utf8' }).stdout || '')
		.split('\n').filter(Boolean).map(Number);
	const main = pids.find(pid => {
		try { return !fs.readFileSync(`/proc/${pid}/cmdline`, 'utf8').includes('--type='); } catch { return false; }
	});
	return main || launch.app.pid;
}

async function shutdown(launch, rec) {
	const t = now();
	// SIGTERM to the main process only: SIGTERM to every process at once leaves
	// the main one stuck (QA finding P12, startup_bench.cjs).
	try { process.kill(mainPid(launch), 'SIGTERM'); } catch { /* already gone */ }
	while (alive(launch) && now() - t < 10000) { await sleep(50); }
	rec.needed_sigkill = alive(launch);
	if (rec.needed_sigkill) {
		if (launch.target === 'app') { spawnSync('pkill', ['-KILL', '-f', '--', profilePattern(launch.profile)]); }
		else { for (const pid of treePids(launch.app.pid)) { try { process.kill(pid, 'SIGKILL'); } catch { /* gone */ } } }
		await sleep(500);
	}
	rec.shutdown_ms = round1(now() - t);
	const code = await Promise.race([launch.samplerDone, sleep(5000).then(() => 'timeout')]);
	if (code === 'timeout') { launch.sampler.kill('SIGTERM'); }
	if (code === 3) {
		const abort = path.join(launch.dir, 'abort.json');
		rec.aborted = fs.existsSync(abort) ? JSON.parse(fs.readFileSync(abort, 'utf8')).reason : 'watchdog';
	}
}

function peakRssMb(dir) {
	const file = path.join(dir, 'sampler.jsonl');
	if (!fs.existsSync(file)) { return null; }
	const totals = fs.readFileSync(file, 'utf8').split('\n').filter(Boolean)
		.map(line => JSON.parse(line).total_rss_kb || 0);
	return totals.length ? Math.round(Math.max(...totals) / 1024) : null;
}

/** Starts the editor and its watchdog with a fresh profile; returns the launch state. */
function start(label) {
	const dir = path.join(cfg.out_dir, label);
	fs.mkdirSync(dir, { recursive: true });
	const profile = fs.mkdtempSync(path.join(cfg.profiles_root, `${label}-`));
	if (cfg.target === 'app') {
		fs.mkdirSync(path.join(profile, 'ud', 'User'), { recursive: true });
		fs.copyFileSync(cfg.settings, path.join(profile, 'ud', 'User', 'settings.json'));
	} else {
		fs.mkdirSync(path.join(profile, 'user'), { recursive: true });
	}
	const fill = (a, pid) => a.replaceAll('{profile_re}', reEscape(profile)).replaceAll('{profile}', profile)
		.replaceAll('{rep_dir}', dir).replaceAll('{pid}', String(pid));
	const [cmd, ...args] = cfg.app_command.map(a => fill(a));
	const launch = { target: cfg.target, dir, profile, appExit: null, samplerExit: null, firstFrame: null };
	const log = fs.openSync(path.join(dir, 'app.log'), 'w');
	launch.wall0 = Date.now();
	launch.t0 = now();
	launch.app = spawn(cmd, args, { env: editorEnv(profile), stdio: ['ignore', log, cfg.target === 'gui' ? 'pipe' : log] });
	launch.app.on('exit', (code, signal) => { launch.appExit = signal || code; });
	if (cfg.target === 'gui') {
		// The GUI's stderr is the channel for its first-frame line; keep a copy in app.log.
		let buffered = '';
		launch.app.stderr.on('data', chunk => {
			const at = round1(now() - launch.t0);
			fs.writeSync(log, chunk);
			buffered += chunk.toString();
			let nl;
			while ((nl = buffered.indexOf('\n')) >= 0) {
				const m = buffered.slice(0, nl).match(/first frame presented (\d+) us after main/);
				if (m && !launch.firstFrame) { launch.firstFrame = { at, us: +m[1] }; }
				buffered = buffered.slice(nl + 1);
			}
		});
	}
	const [samplerCmd, ...samplerArgs] = cfg.sampler_command.map(a => fill(a, launch.app.pid));
	const samplerLog = fs.openSync(path.join(dir, 'sampler.log'), 'w');
	launch.sampler = spawn(samplerCmd, samplerArgs, { stdio: ['ignore', samplerLog, samplerLog] });
	launch.samplerDone = new Promise(resolve => launch.sampler.on('exit', code => {
		launch.samplerExit = code;
		resolve(code);
	}));
	return launch;
}

async function once(label, warmup) {
	const rec = { label, warmup };
	// Memory freed by the previous launch or by rustode's in-process opens takes a moment to show
	// as available: wait up to a minute before refusing.
	for (let waited = 0; memAvailableMb() < cfg.min_available_mb && waited < 60; waited++) { await sleep(1000); }
	const mem = memAvailableMb();
	if (mem < cfg.min_available_mb) {
		rec.refused = `MemAvailable ${mem} MB is below ${cfg.min_available_mb} MB`;
		return rec;
	}
	if (cfg.cache === 'cold') { await evict(rec); }
	const launch = start(label);
	try {
		await (cfg.target === 'app' ? measureApp : measureGui)(launch, rec);
	} catch (e) {
		const detail = String((e && e.message) || e).split('\n')[0];
		if (e instanceof NoEndPoint && cfg.timeout_is_result) {
			rec.outcome = launch.appExit !== null ? 'exited-first' : 'timeout';
			rec.outcome_detail = detail;
		} else if (!(e instanceof Aborted)) { rec.error = detail; }
	}
	await shutdown(launch, rec);
	rec.app_exit = launch.appExit;
	rec.peak_rss_mb = peakRssMb(launch.dir);
	fs.rmSync(launch.profile, { recursive: true, force: true });
	return rec;
}

function statusOf(rec) {
	if (rec.refused) { return ['refused', rec.refused]; }
	if (rec.aborted) { return ['aborted', `${rec.label}: ${rec.aborted}`]; }
	if (rec.error) { return ['error', `${rec.label}: ${rec.error}`]; }
	return ['ok', undefined];
}

const out = {
	schema: 'rustode-bench-app-results/2',
	target: cfg.target,
	measure: cfg.measure,
	cache: cfg.cache,
	status: 'ok',
	node: process.version,
	playwright_core: cfg.target === 'app' ? require(path.join(cfg.playwright_core, 'package.json')).version : null,
	launches: [],
};
const save = () => fs.writeFileSync(path.join(cfg.out_dir, 'results.json'), `${JSON.stringify(out, null, 1)}\n`);

(async () => {
	const problems = guardProblems();
	if (problems.length) {
		out.status = 'refused';
		out.reason = `not inside the virtual display without network: ${problems.join('; ')}`;
		save();
		process.exitCode = EXIT.refused;
		return;
	}
	const plan = [];
	for (let i = 1; i <= cfg.warmup; i++) { plan.push([`warmup-${i}`, true]); }
	for (let i = 1; i <= cfg.repetitions; i++) { plan.push([`rep-${i}`, false]); }
	for (const [label, warmup] of plan) {
		const rec = await once(label, warmup);
		out.launches.push(rec);
		[out.status, out.reason] = statusOf(rec);
		save();
		console.log(JSON.stringify(rec));
		if (out.status !== 'ok') { break; }
		await sleep(2000); // the QA startup benchmark's pause between launches
	}
	process.exitCode = EXIT[out.status];
})().catch(e => {
	console.error('driver error:', e);
	out.status = 'error';
	out.reason = `driver: ${String((e && e.message) || e).split('\n')[0]}`;
	save();
	process.exitCode = EXIT.error;
});
