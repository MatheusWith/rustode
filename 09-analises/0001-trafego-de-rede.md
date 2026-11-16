# 0001: Which servers does the app connect to on its own?

> **Date:** 2026-11-16 · **Confidence:** measured · **Gap:** opens G9 · **Components:** main process, shared process, Electron

## Question

The built app runs with the default settings: no `--disable-telemetry`, nothing turned off. Which servers does it connect to during typical use, and how does that compare with the system's `code` 1.138?

## Short answer

No Microsoft service. The app contacts three places:
- Open VSX (gallery and files);
- the Eclipse Foundation's malicious-extension list on GitHub;
- Google's CDN, from which Electron downloads the spell-checker dictionary at startup.

No connection escaped the proxy. `code` 1.138 does the same, minus the malicious-extension list.

## Evidence

| What | Where | Note |
|---|---|---|
| Build hosts, with connections, bytes and the second of the first connection | `08-testes-do-build/2026-11-16-codium1.147-rede/network-report.md` and `proxy.jsonl` | measured by the logging proxy |
| 1.138 hosts | `08-testes-do-build/2026-11-16-code1.138-rede/network-report.md` | measured |
| Connections outside the proxy: 0; DNS outside the proxy: 0 | `…-rede/conn.jsonl` (empty) | sampled every 0.5 s with `ss` |
| Typical-use steps | `…-rede/steps.json` | open, 120 s idle, extensions + search for "python", a TypeScript file, welcome page, 60 s idle |
| URL of the malicious-extension list | `01-app/VSCode-linux-x64/resources/app/product.json`, `extensionsGallery.controlUrl` | `raw.githubusercontent.com/EclipseFdn/publish-extensions/…/extensions.json` |
| No update check | no `VSCodium/versions` in the proxy; `product.json` without `updateUrl` | ADR 0003 |
| Electron dictionary | 1 connection to `redirector.gvt1.com` (6 KB) and 1 to `rr1---sn-oxunxg8pjvn-uixe.gvt1.com` (447 KB) in seconds 0–1 | the size matches the `en-US-10-1.bdic` (451,968 bytes) saved by the system `code` in `~/.config/Code - OSS/Dictionaries/` |

| Host | VSCodium 1.147 build | `code` 1.138 |
|---|---|---|
| `redirector.gvt1.com`, `rr1---….gvt1.com` (Google) | 1 + 1 connections, s 0–1 | 1 + 1, s 1 |
| `raw.githubusercontent.com` | 2 connections, 31 KB, s 2 | — |
| `open-vsx.org` | 2 connections, 619 KB, **s 146** (when the extensions view opens) | 2 connections, 553 KB, **s 33** (still idle) |
| `openvsx.eclipsecontent.org` | 7 connections, 393 KB, s 149 | 6 connections, 389 KB, s 157 |

Commands (from `~/Projects/vscode/qa/suite/`):

```sh
QA_BIN=$HOME/Projects/VSCode-linux-x64/codium tools/run_virtual.sh scenarios/run_network.sh codium1.147-rede
QA_BIN=/usr/bin/code tools/run_virtual.sh scenarios/run_network.sh code1.138-rede
```

## Limitations

- The proxy sees host and volume, not contents (TLS).
- The search for connections outside the proxy is sampled every 0.5 s, so very short connections can slip through.
- It covers only the scenario's steps. Features not exercised (GitHub sign-in, installing an extension, sync) were not measured.
- Unverified hypothesis: 1.138 contacts Open VSX while still idle (s 33) because of some startup check (recommendations?) that the build doesn't do. Check the code before stating it.

## Next steps

- G9: find out whether the dictionary download can be turned off without a patch.
- Check the s 33 hypothesis on 1.138: look for gallery calls at startup in `fonte/arvore/src/vs/workbench/contrib/extensions/`.
