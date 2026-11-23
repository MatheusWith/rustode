'use strict';
/**
 * rustode bench: reaching the App's own models and services over CDP, with no
 * input sent (AGENTS.md section 3.2 allows CDP, 11.5.5 forbids keystrokes).
 *
 * The workbench exposes no global handle, so this finds one. A DOM event
 * listener is a function of the workbench bundle, and DevTools shows the scopes
 * a function closes over (`[[Scopes]]`): the bundle's module scope holds every
 * top-level class that some other function refers to. Classes are found by what
 * their prototype defines (public method names, which the build keeps; private
 * `_names` are mangled), and instances by `Runtime.queryObjects`, which returns
 * every live object with a given prototype.
 *
 *  - `TextModel` (src/vs/editor/common/model/textModel.ts): `pushEditOperations`,
 *    `pushStackElement`, `undo`, `canUndo`, `getPositionAt`;
 *  - `InstantiationService` (src/vs/platform/instantiation/common/instantiationService.ts):
 *    `invokeFunction`, `createInstance`, `createChild`; with the service
 *    identifier `ICommandService` (a decorator function whose `toString()` is
 *    `commandService`) it hands out the command service, which runs
 *    `workbench.action.findInFiles`.
 *
 * Everything found this way is specific to the bundle in the App copy, which
 * `rustode bench` has checked against the manifest. If a class is not found the
 * functions here throw, and the launch is recorded as an error, never as a number.
 * Nothing here runs inside a timed region except what `runEdits` and
 * `runSearch` say they time.
 *
 * The edit generator (mulberry32) is the one of `rustode_workspace::bench`
 * (crates/workspace/src/bench.rs); `selfCheck` pins its first six edits to the
 * values that Rust test `the_edit_generator_is_deterministic_and_pinned` pins.
 */

/** The first edits of seed 1 on a 1000-char document: the same values as the Rust test. */
const PINNED = [[496, 'r'], [95, 'h'], [270, 'u'], [320, 'e'], [559, 'v'], [330, 'i']];

/** Runs in the page (it is serialized): the edit script, timed. */
function editsInPage(model, seed, count) {
	function mulberry32(a) {
		return function () {
			a |= 0; a = a + 0x6D2B79F5 | 0;
			let t = Math.imul(a ^ a >>> 15, 1 | a);
			t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t;
			return ((t ^ t >>> 14) >>> 0);
		};
	}
	function fnv1a(text) {
		let h = 0x811C9DC5;
		for (let i = 0; i < text.length; i++) { h = Math.imul(h ^ text.charCodeAt(i), 0x01000193) >>> 0; }
		return h.toString(16).padStart(8, '0');
	}
	const nextFrame = () => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
	return (async () => {
		const rng = mulberry32(seed);
		const startChars = model.getValueLength();
		let len = startChars;
		const t0 = performance.now();
		for (let i = 0; i < count; i++) {
			const at = rng() % (len + 1);
			const letter = String.fromCharCode(97 + rng() % 26);
			len++;
			const p = model.getPositionAt(at);
			model.pushStackElement();
			model.pushEditOperations(null, [{
				range: { startLineNumber: p.lineNumber, startColumn: p.column, endLineNumber: p.lineNumber, endColumn: p.column },
				text: letter,
			}], () => null);
		}
		model.pushStackElement();
		const t1 = performance.now();
		await nextFrame();
		const t2 = performance.now();
		const applied = { chars: model.getValueLength(), hash: fnv1a(model.getValue()) };
		let steps = 0;
		const u0 = performance.now();
		while (model.canUndo() && steps < count + 1) { model.undo(); steps++; }
		const u1 = performance.now();
		await nextFrame();
		const u2 = performance.now();
		return {
			start_chars: startChars,
			apply_ms: t1 - t0, apply_frame_ms: t2 - t0, applied_chars: applied.chars, applied_hash: applied.hash,
			undo_ms: u1 - u0, undo_frame_ms: u2 - u0, undo_steps: steps,
			undone_chars: model.getValueLength(), undone_hash: fnv1a(model.getValue()),
		};
	})();
}

/** Runs in the page: the search through `workbench.action.findInFiles`, timed from before the command. */
function searchInPage(instantiationServices, serviceId, args, timeoutMs) {
	return (async () => {
		let service = null;
		for (const instance of instantiationServices) {
			try {
				service = instance.invokeFunction(accessor => accessor.get(serviceId));
				if (service && service.executeCommand) { break; }
			} catch { /* a child that cannot give it */ }
		}
		if (!service) { throw new Error('no command service found'); }
		const counted = /^(\d[\d,]*) results? in (\d[\d,]*) files?\b|^No results found/;
		// The sidebar's progress bar is `active` while the search runs and `done` when it ends
		// (src/vs/workbench/contrib/search/browser/searchView.ts `onSearchComplete`, progressComplete()).
		const active = () => !!document.querySelector('.part.sidebar .monaco-progress-container.active');
		let first = null, counted_at = null, done_at = null, message = null, messageChangedAt = null;
		let sawActive = false, lastDomChange = null;
		const t0 = performance.now();
		const check = (fromObserver) => {
			const now = performance.now();
			if (fromObserver) { lastDomChange = now; }
			if (first === null && document.querySelector('.search-view .monaco-list-row')) { first = now - t0; }
			const m = document.querySelector('.search-view .messages .message');
			const text = m ? m.textContent.trim() : '';
			if (counted.test(text)) {
				if (text !== message) { messageChangedAt = now - t0; }
				message = text;
				if (counted_at === null) { counted_at = now - t0; }
			}
			if (active()) { sawActive = true; } else if (sawActive && done_at === null && message !== null) { done_at = now - t0; }
		};
		const observer = new MutationObserver(() => check(true));
		observer.observe(document.body, { subtree: true, childList: true, characterData: true, attributes: true, attributeFilter: ['class'] });
		try {
			await service.executeCommand('workbench.action.findInFiles', args);
			const deadline = performance.now() + timeoutMs;
			const settled = () => done_at !== null && performance.now() - t0 - done_at > 150;
			while (!settled() && performance.now() < deadline) {
				await new Promise(resolve => setTimeout(resolve, 20));
				check(false);
				// A search that ended before the progress bar was ever seen active: settled.
				if (!sawActive && message !== null && lastDomChange !== null && performance.now() - lastDomChange > 1500) {
					done_at = lastDomChange - t0;
				}
			}
		} finally { observer.disconnect(); }
		const complete = done_at === null ? null : Math.max(done_at, messageChangedAt === null ? 0 : messageChangedAt);
		return { first_ms: first, counted_ms: counted_at, complete_ms: complete, message, saw_progress: sawActive };
	})();
}

async function send(cdp, method, params) { return cdp.send(method, params); }

/** The bundle's module scope, found through any DOM event listener that closes over it. */
async function moduleScope(cdp) {
	const found = await send(cdp, 'Runtime.evaluate', {
		includeCommandLineAPI: true,
		expression: `(() => {
			for (const el of document.querySelectorAll('.monaco-workbench, .monaco-workbench *')) {
				const all = getEventListeners(el);
				for (const kind of Object.keys(all)) { for (const l of all[kind]) { if (typeof l.listener === 'function') { return l.listener; } } }
			}
			return null;
		})()`,
	});
	if (!found.result.objectId) { throw new Error('no DOM event listener found to reach the workbench bundle'); }
	const props = await send(cdp, 'Runtime.getProperties', { objectId: found.result.objectId, ownProperties: true });
	const scopesProp = (props.internalProperties || []).find(p => p.name === '[[Scopes]]');
	if (!scopesProp) { throw new Error('the listener has no [[Scopes]]'); }
	const scopes = await send(cdp, 'Runtime.getProperties', { objectId: scopesProp.value.objectId, ownProperties: true });
	const module = scopes.result.find(s => s.value && /^Module|^Script/.test(s.value.description || ''));
	if (!module) { throw new Error(`no module scope in ${scopes.result.map(s => s.value && s.value.description)}`); }
	return module.value.objectId;
}

/** Calls `fn` with `this` = the remote object; returns the value. */
async function callOn(cdp, objectId, fn, options = {}) {
	const r = await send(cdp, 'Runtime.callFunctionOn', {
		objectId, functionDeclaration: fn.toString(), returnByValue: !options.byRef, awaitPromise: true,
		arguments: options.args,
	});
	if (r.exceptionDetails) { throw new Error(`in the page: ${(r.exceptionDetails.exception && r.exceptionDetails.exception.description) || r.exceptionDetails.text}`.split('\n')[0]); }
	return options.byRef ? r.result : r.result.value;
}

/** The variable of `vars` that is a class whose own prototype defines every one of `methods`. */
async function findClass(cdp, vars, methods) {
	const candidates = vars.filter(v => v.value && v.value.type === 'function' && methods.every(m => (v.value.description || '').includes(m)));
	for (const v of candidates) {
		const ok = await callOn(cdp, v.value.objectId, function (names) {
			const p = this.prototype;
			return !!p && names.every(n => Object.prototype.hasOwnProperty.call(p, n));
		}, { args: [{ value: methods }] });
		if (ok) { return v; }
	}
	throw new Error(`no class defines ${methods.join(', ')}`);
}

/** Every live instance of a class, as a remote array. */
async function instancesOf(cdp, classVar) {
	const proto = await callOn(cdp, classVar.value.objectId, function () { return this.prototype; }, { byRef: true });
	const q = await send(cdp, 'Runtime.queryObjects', { prototypeObjectId: proto.objectId });
	return q.objects.objectId;
}

/**
 * Connects to the workbench page of a running App; returns what the scenarios
 * need. The discovery (about one second, 20 MB of CDP) is not timed by anyone.
 */
async function attach(page) {
	const cdp = await page.context().newCDPSession(page);
	const scope = await moduleScope(cdp);
	const vars = (await send(cdp, 'Runtime.getProperties', { objectId: scope, ownProperties: true })).result;
	return {
		cdp, vars,
		/** The TextModel of the editor that shows the file whose path ends with `suffix`. */
		async model(suffix, expectedChars) {
			const cls = await findClass(cdp, vars, ['pushEditOperations', 'getValueInRange', 'pushStackElement', 'canUndo', 'getPositionAt']);
			const models = await instancesOf(cdp, cls);
			const index = await callOn(cdp, models, function (want, chars) {
				return this.findIndex(m => !m.isDisposed() && String(m.uri).endsWith(want) && m.getValueLength() === chars);
			}, { args: [{ value: suffix }, { value: expectedChars }] });
			if (index < 0) { throw new Error(`no live text model for ${suffix} with ${expectedChars} chars`); }
			return { models, index };
		},
		/** Applies the seeded edit script to the model and undoes it; times in ms from the page's clock. */
		async runEdits({ models, index }, seed, count) {
			const model = await callOn(cdp, models, function (i) { return this[i]; }, { byRef: true, args: [{ value: index }] });
			return callOn(cdp, model.objectId, editsInPage, { args: [{ objectId: model.objectId }, { value: seed }, { value: count }] });
		},
		/** Runs the Find in Files command and times the first result row and the result message. */
		async runSearch(args, timeoutMs) {
			const cls = await findClass(cdp, vars, ['invokeFunction', 'createInstance', 'createChild']);
			const instances = await instancesOf(cdp, cls);
			let id = null;
			for (const v of vars.filter(v => v.value && v.value.type === 'function' && /-decorator can only be used to decorate a parameter/.test(v.value.description || ''))) {
				const name = await callOn(cdp, v.value.objectId, function () { return String(this); });
				if (name === 'commandService') { id = v; break; }
			}
			if (!id) { throw new Error('no service identifier "commandService"'); }
			return callOn(cdp, instances, searchInPage, {
				args: [{ objectId: instances }, { objectId: id.value.objectId }, { value: args }, { value: timeoutMs }],
			});
		},
	};
}

/** The generator, in Node, for the self-check and for tests. */
function editOffsets(seed, count, startChars) {
	let a = seed | 0;
	const next = () => {
		a = a + 0x6D2B79F5 | 0;
		let t = Math.imul(a ^ a >>> 15, 1 | a);
		t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t;
		return (t ^ t >>> 14) >>> 0;
	};
	const out = [];
	let len = startChars;
	for (let i = 0; i < count; i++) {
		const at = next() % (len + 1);
		out.push([at, String.fromCharCode(97 + next() % 26)]);
		len++;
	}
	return out;
}

/** Throws unless the generator gives the values pinned in the Rust test. */
function selfCheck() {
	const got = JSON.stringify(editOffsets(1, 6, 1000));
	if (got !== JSON.stringify(PINNED)) { throw new Error(`the edit generator drifted from the Rust one: ${got}`); }
}

module.exports = { attach, editOffsets, selfCheck, editsInPage, searchInPage, PINNED };
