# 0001: A build without a recorded commit changes where the app looks for modules

> In VS Code, `product.commit` is not just informational: the code uses its presence to decide whether it is running packaged. Without a commit, the agent host looks for modules in `node_modules`, which barely exists in the package, and fails to load `tree-sitter.wasm`.

**Sources:**
- `02-package-analysis/source/tree/src/vs/platform/agentHost/node/appNodeModules.ts`;
- `…/agentHost/node/commandAutoApprover.ts`;
- `02-package-analysis/source/tree/build/lib/git.ts` and `build/lib/getVersion.ts`;
- `06-port/PORT-NOTES.md` ("`build.sh`: `BUILD_SOURCEVERSION`").

## Context

The first built app showed `1.147.0 Unknown commit x64` in `--version`. The agent host then logged this error while the compile cache was being generated: `failed to asynchronously prepare wasm: ENOENT … resources/app/node_modules/@vscode/tree-sitter-wasm/wasm/tree-sitter.wasm`.

## What I observed

- The build reads the commit straight from the `.git/HEAD` file (`build/lib/git.ts`, `getVersion`). In a worktree, `.git` is a one-line file (`gitdir: …`), so the read fails and the commit stays undefined. Only the `BUILD_SOURCEVERSION` variable (`build/lib/getVersion.ts`) works around this.
- `appNodeModules.ts` decides the modules path like this:

  ```ts
  function hasUnpackedNodeModulesArchive(): boolean {
  	return !!process.versions['electron'] && !!product.commit && !process.env['VSCODE_DEV'];
  }
  ```

  Without a commit, the result is `false`, and the path becomes `node_modules` instead of `node_modules.asar.unpacked`. `tree-sitter-wasm` is only in the second one (measured: `ls resources/app/node_modules.asar.unpacked/@vscode/`).
- With `BUILD_SOURCEVERSION` in `build.sh`, the app started showing the commit, and the error went away.

> **Transferable concept:** build metadata (commit, date, quality) is often used as an "I am packaged" signal. A local build that doesn't fill it in can behave like a development version in unexpected places, without any compile error.

## Taking it to other projects

- After producing a build outside CI, check the metadata (`--version`, `product.json`) before testing behavior.
- Builds in a worktree or in copies without `.git` need the commit through an environment variable.
- When studying an app, look for where it tests `commit`, `quality` or `isBuilt`: those tests change paths and features.
