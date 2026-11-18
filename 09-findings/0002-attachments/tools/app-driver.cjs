// Spike driver: runs inside the virtual session. Spawns the App with an isolated profile,
// connects over CDP, opens an untitled editor, records trusted keydown events in the renderer,
// runs the two native-path injectors, and reads the editor text back.
const { chromium } = require(process.env.HOME + '/Projects/vscode/node_modules/playwright-core');
const { spawn, execFileSync } = require('child_process');
const fs = require('fs');
const W = process.argv[2] /* out dir */, P = process.argv[3];
const sleep = ms => new Promise(r => setTimeout(r, ms));
(async () => {
  const wd = process.env.WAYLAND_DISPLAY || '';
  if (!wd.startsWith('qa-virt-')) throw new Error('not in virtual session: ' + wd);
  const app = spawn(process.env.CODIUM || process.env.HOME + '/Projects/rustode/01-app/VSCode-linux-x64/codium', [
    `--user-data-dir=${P}/ud`, `--extensions-dir=${P}/ext`, `--shared-data-dir=${P}/shared`,
    '--remote-debugging-port=9333', '--disable-workspace-trust', '--new-window', `${P}/ws`], { env: { ...process.env, NODE_DISABLE_COMPILE_CACHE: '1' }, stdio: ['ignore', fs.openSync(`${W}/s2-app.log`, 'w'), fs.openSync(`${W}/s2-app.log`, 'a')] });
  let browser;
  for (let i = 0; i < 60 && !browser; i++) { try { browser = await chromium.connectOverCDP('http://127.0.0.1:9333'); } catch { await sleep(1000); } }
  let page;
  for (let i = 0; i < 30 && !page; i++) { page = browser.contexts().flatMap(c => c.pages()).find(p => /workbench/.test(p.url())); if (!page) await sleep(1000); }
  await page.waitForSelector('.monaco-workbench .part.editor', { timeout: 30000 });
  await sleep(3000);
  console.log('ozone:', execFileSync('sh', ['-c', `ps -eo args | grep -- "--user-data-dir=${P}/ud" | grep -o -- "--ozone-platform=[a-z]*" | sort -u`]).toString().trim() || '(none on cmdline)');
  await page.keyboard.press('Control+n');            // CDP input, only to open the editor
  await sleep(1500);
  await page.evaluate(() => { window.__k = []; window.addEventListener('keydown', e => window.__k.push([performance.timeOrigin + performance.now(), e.key, e.code, e.isTrusted]), true); });
  const run = (bin, args, out) => { const o = execFileSync(bin, args, { stdio: ['ignore', 'pipe', 'inherit'] }).toString(); fs.writeFileSync(out, o); };
  run(`${process.env.BIN}/fakeinput`, ['Hello fake 1', '40'], `${W}/s2-fake.out`);
  await sleep(1000);
  run(`${process.env.BIN}/eistype`, [' eis typed 2', '40'], `${W}/s2-eis.out`);
  await sleep(1500);
  const keys = await page.evaluate(() => window.__k);
  fs.writeFileSync(`${W}/s2-app-keys.out`, keys.map(k => `KEYDOWN ${k[0].toFixed(3)} ${JSON.stringify(k[1])} ${k[2]} trusted=${k[3]}`).join('\n') + '\n');
  const text = await page.evaluate(() => document.querySelector('.editor-instance .monaco-editor .view-lines')?.innerText);
  console.log('EDITOR TEXT:', JSON.stringify(text));
  await page.screenshot({ path: `${W}/s2-app.png` });
  await browser.close();
  app.kill('SIGTERM'); await sleep(3000);
  try { execFileSync('pkill', ['-KILL', '-f', '--', `--user-data-dir=${P}/ud`]); } catch {}
})().catch(e => { console.error('DRIVER ERROR', e); process.exitCode = 1; try { execFileSync('pkill', ['-TERM', '-f', '--', `--user-data-dir=${P}/ud`]); } catch {} });
