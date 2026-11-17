'use strict';
/**
 * rustode bench: the App side of the "open-file" scenario.
 *
 * Usage: node open_file.cjs <driver-config.json>
 *
 * `rustode bench` writes the config and runs this script inside the QA
 * harness's virtual display and a network namespace without network:
 *
 *   run_virtual.sh bwrap --dev-bind / / --unshare-net --die-with-parent node open_file.cjs <config>
 *
 * so the App and this driver share a loopback interface (for CDP) and nothing
 * else. rustode builds every command this script runs (`app_command`,
 * `sampler_command`); this script fills in the per-launch placeholders
 * {profile}, {profile_re} and {rep_dir}, times the launch and cleans up.
 *
 * Each launch (warm-ups first, not recorded) gets a fresh isolated profile:
 *   1. refuse below `min_available_mb` MemAvailable;
 *   2. spawn the App with the fixture on its command line, then the QA
 *      harness's watchdog (collectors/sampler.py), which kills the App below
 *      700 MB MemAvailable or under sustained memory pressure;
 *   3. time, from the spawn: the CDP endpoint answering, the workbench's editor
 *      part existing, and the fixture's first line rendered in an editor
 *      `.view-line`, each polled over CDP (no input is sent to the App);
 *   4. screenshot, SIGTERM the main process (SIGKILL after 10 s, as the QA
 *      startup benchmark does), wait for the watchdog, delete the profile.
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
const { chromium } = require(cfg.playwright_core);

const EXIT = { ok: 0, error: 1, refused: 4, aborted: 5 };
const round1 = ms => Math.round(ms * 10) / 10;
const reEscape = s => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
const profilePattern = profile => `[-]-user-data-dir=${reEscape(profile)}/ud`;
const alive = profile => spawnSync('pgrep', ['-f', '--', profilePattern(profile)]).status === 0;
const memAvailableMb = () =>
	Math.floor(+fs.readFileSync('/proc/meminfo', 'utf8').match(/MemAvailable:\s+(\d+)/)[1] / 1024);

class Aborted extends Error { }

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

/** The environment without variables that would change how the App starts. */
function appEnv() {
	const env = { ...process.env };
	for (const key of Object.keys(env)) {
		if (key.startsWith('ELECTRON_') || key.startsWith('VSCODE_') || key === 'NODE_OPTIONS') { delete env[key]; }
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
		if (launch.appExit !== null) { throw new Error(`the App exited (${launch.appExit}) before ${what}`); }
		return fn();
	}, Math.max(remaining, 0), everyMs);
	if (!r.v) { throw new Error(`timeout: no ${what} within ${cfg.timeout_ms} ms of the launch`); }
	return { v: r.v, ms: round1(now() - launch.t0) };
}

async function measure(launch, rec) {
	const endpoint = `http://127.0.0.1:${cfg.cdp_port}`;
	rec.spawn_to_cdp_ms = (await until(launch, 'CDP endpoint', async () => {
		try { return (await fetch(`${endpoint}/json/version`)).ok; } catch { return false; }
	}, 25)).ms;
	const browser = await chromium.connectOverCDP(endpoint);
	try {
		const pages = () => browser.contexts().flatMap(c => c.pages());
		const page = (await until(launch, 'workbench page', () => pages().find(p => /workbench/.test(p.url())), 25)).v;
		rec.spawn_to_workbench_ms = (await until(launch, 'editor part', () => page.evaluate(
			() => !!document.querySelector('.monaco-workbench .part.editor')).catch(() => false), 25)).ms;
		rec.spawn_to_first_line_ms = (await until(launch, "fixture's first line", () => page.evaluate(
			firstLineShown, cfg.first_line).catch(() => false), cfg.poll_ms)).ms;
		await page.screenshot({ path: path.join(launch.dir, 'first-line.png') }).catch(() => { });
	} finally {
		await browser.close().catch(() => { }); // disconnects; the App keeps running
	}
}

/** The App's main process: the one with the profile and no `--type=` (QA startup benchmark). */
function mainPid(launch) {
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
	while (alive(launch.profile) && now() - t < 10000) { await sleep(50); }
	rec.needed_sigkill = alive(launch.profile);
	if (rec.needed_sigkill) {
		spawnSync('pkill', ['-KILL', '-f', '--', profilePattern(launch.profile)]);
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

/** Starts the App and its watchdog with a fresh profile; returns the launch state. */
function start(label) {
	const dir = path.join(cfg.out_dir, label);
	fs.mkdirSync(dir, { recursive: true });
	const profile = fs.mkdtempSync(path.join(cfg.profiles_root, `${label}-`));
	fs.mkdirSync(path.join(profile, 'ud', 'User'), { recursive: true });
	fs.copyFileSync(cfg.settings, path.join(profile, 'ud', 'User', 'settings.json'));
	const fill = a => a.replaceAll('{profile_re}', reEscape(profile)).replaceAll('{profile}', profile)
		.replaceAll('{rep_dir}', dir);
	const [cmd, ...args] = cfg.app_command.map(fill);
	const [samplerCmd, ...samplerArgs] = cfg.sampler_command.map(fill);
	const launch = { dir, profile, appExit: null, samplerExit: null };
	const log = fs.openSync(path.join(dir, 'app.log'), 'w');
	launch.t0 = now();
	launch.app = spawn(cmd, args, { env: appEnv(), stdio: ['ignore', log, log] });
	launch.app.on('exit', (code, signal) => { launch.appExit = signal || code; });
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
	const mem = memAvailableMb();
	if (mem < cfg.min_available_mb) {
		rec.refused = `MemAvailable ${mem} MB is below ${cfg.min_available_mb} MB`;
		return rec;
	}
	const launch = start(label);
	try {
		await measure(launch, rec);
	} catch (e) {
		if (!(e instanceof Aborted)) { rec.error = String((e && e.message) || e).split('\n')[0]; }
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
	schema: 'rustode-bench-app-results/1',
	status: 'ok',
	node: process.version,
	playwright_core: require(path.join(cfg.playwright_core, 'package.json')).version,
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
