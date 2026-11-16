# ADR 0001: The production bundle is built without source maps

- **Status:** accepted (2026-11-16, approved by the owner after the proposal; applied in `06-port/build.sh` and in the `97-port-bundle-no-sourcemaps.patch` patch)
- **Build:** local VSCodium 1.147.0, VS Code `22d8dccf0f29`

## Context

The production bundle (`gulp vscode-linux-x64-min-packing` → `build/next/index.ts bundle`) did not fit in the machine's 7.5 GiB:

- 1st attempt: esbuild with all entry points in parallel led to a global OOM at 08:29 (the kernel killed esbuild, `oom_score_adj` 800).
- 2nd attempt, with the sequential bundle (patch 98): the bundler's **Node** process reached 3.1 GB of RAM plus 673 MB of swap. It then spent 30 minutes swapping, with the machine's memory pressure at about 85 %, until the owner rebooted the computer.

There were three causes (`06-port/logs/build-*.log`; `PORT-NOTES.md`, "Decisões locais"):
- `build/next/bundle.ts` generates `linked` maps with `sourcesContent: true`;
- the NLS plugin adds an inline map per module;
- all outputs, with their maps, stay in memory until post-processing.

## Decision

The production bundle is built with `VSCODE_BUNDLE_SOURCEMAPS=0`. This turns off three things: the bundle's maps, the inline NLS map and the pre-NLS copy of each output.

Containment rules:

1. The option lives only in patch 97 (`build/next/bundle.ts`, `nls-plugin.ts`, `index.ts`); the code's default still generates maps.
2. This port's `build.sh` turns the option on, and nothing else does.
3. The post-processing syntax check still runs (now one file at a time, with `VSCODE_BUNDLE_SEQUENTIAL=1`).
4. The small separate builds (preloads, the agent host's crypto) still have maps.

## The guarantee

The generated JavaScript is the same with or without the option. The option only stops producing the `.map` files and keeping the copies used to adjust them. The app works the same (measured: smoke, network and the CDP battery in `08-testes-do-build/`).

## Consequences

- Measured: the bundle finished in 65 s with a 2.7 GB peak in the cgroup (before, it did not finish).
- Stack traces and minified snippets don't point straight to the source. Reverse engineering now depends on the source tree and on anchors ([ADR 0004](0004-arvore-fonte-e-a-referencia.md), the `find-minified-symbol` skill, gap G2).
- Getting maps requires a build on a machine with more RAM. Any other change to the bundle needs a new decision, not an extension of this one.
