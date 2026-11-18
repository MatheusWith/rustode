# Patches

> **Last reviewed:** 2026-11-16 · **Last updated:** 2026-11-16 · **Change:** minor (translated to English) · **Target:** VSCodium `ca7f7b44b5e5` ported to VS Code 1.147.0

These are all the port's patches. From VSCodium: 32 applied unchanged, 19 adapted to 1.147, 6 skipped and 2 JSON actions; plus 4 exclusive to the port. The reason for each adaptation is in [`06-port/PORT-NOTES.md`](../06-port/PORT-NOTES.md). The diff of everything applied is in [`02-package-analysis/source/port-applied.diff`](../02-package-analysis/source/port-applied.diff).

Each version lives in one of two places:
- `06-port/patches/orig/<patch>`: as it came from VSCodium;
- `06-port/patches/port/<patch>`: adapted; exists only for the adapted and the port-only patches.

`apply.sh` uses the `port/` version when it exists.

## VSCodium patches

| Patch | Area | What it does (from the name) | Status |
|---|---|---|---|
| `00-binary-fix-name.patch` | binary | fix name | applied unchanged |
| `00-brand-remove-branding.patch` | brand | remove branding | **adapted** to 1.147 |
| `00-build-download-extensions-from-gh.patch` | build | download extensions from gh | applied unchanged |
| `00-build-package-missing-dependencies.patch` | build | package missing dependencies | applied unchanged |
| `00-build-update-sourcemap-url.patch` | build | update sourcemap url | **adapted** to 1.147 |
| `00-cloud-remove.patch` | cloud | remove | applied unchanged |
| `00-community-add-announcements.patch` | community | add announcements | **adapted** to 1.147 |
| `00-copilot-disable-terminal-suggest.patch` | copilot | disable terminal suggest | applied unchanged |
| `00-copilot-fix-action-condition.patch` | copilot | fix action condition | applied unchanged |
| `00-copilot-remove-terminal-hint.patch` | copilot | remove terminal hint | applied unchanged |
| `00-ext-git-ai-coauthor-off.patch` | ext | git ai coauthor off | applied unchanged |
| `00-ext-github-authentication-use-pat.patch` | ext | github authentication use pat | applied unchanged |
| `00-ext-github-remove-vscodedev.patch` | ext | github remove vscodedev | **adapted** to 1.147 |
| `00-extension-disable-signature-verification.patch` | extension | disable signature verification | applied unchanged |
| `00-remote-add-missing-dependencies.patch` | remote | add missing dependencies | **adapted** to 1.147 |
| `00-remote-add-url.patch` | remote | add url | applied unchanged |
| `00-remote-disable-client-validation.patch` | remote | disable client validation | applied unchanged |
| `00-remote-remove-missing-vsda.patch` | remote | remove missing vsda | **adapted** to 1.147 |
| `00-security-add-command-filter.patch` | security | add command filter | applied unchanged |
| `00-settings-gallery.patch` | settings | gallery | applied unchanged |
| `00-settings-user-product.patch` | settings | user product | applied unchanged |
| `00-telemetry-disable.patch` | telemetry | disable | applied unchanged |
| `00-tunnel-disable-recommendation.patch` | tunnel | disable recommendation | applied unchanged |
| `00-ui-custom-font.patch` | ui | custom font | **adapted** to 1.147 |
| `00-ui-disable-modernui.patch` | ui | disable modernui | applied unchanged |
| `00-ui-improve-eol-banner.patch` | ui | improve eol banner | applied unchanged |
| `00-ui-report-issue.patch` | ui | report issue | applied unchanged |
| `00-update-rename-cache-path.patch` | update | rename cache path | applied unchanged |
| `00-vsce-use-custom-lib.patch` | vsce | use custom lib | **adapted** to 1.147 |
| `10-version-add-release.patch` | version | add release | applied unchanged |
| `11-update-use-github-release.patch` | update | use github release | **adapted** to 1.147 |
| `12-update-add-cooldown.patch` | update | add cooldown | applied unchanged |
| `20-keymap-use-custom-lib.patch` | keymap | use custom lib | **adapted** to 1.147 |
| `21-policy-use-custom-lib.patch` | policy | use custom lib | **adapted** to 1.147 |
| `22-osproxy-use-custom-lib.patch` | osproxy | use custom lib | **adapted** to 1.147 |
| `30-build-add-missing-dependencies.patch` | build | add missing dependencies | **adapted** to 1.147 |
| `40-cli-use-reh-archive.patch` | cli | use reh archive | applied unchanged |
| `50-build-improve-gulp-tasks.patch` | build | improve gulp tasks | applied unchanged |
| `52-ext-copilot-remove-it.json` | ext | copilot remove it | JSON action (missing paths become warnings) |
| `53-ext-copilot-remove-it.patch` | ext | copilot remove it | **adapted** to 1.147 |
| `60-security-add-option-for-malicious-ext.patch` | security | add option for malicious ext | **adapted** to 1.147 |
| `61-extension-close-connection.patch` | extension | close connection | **adapted** to 1.147 |
| `80-ui-disable-onboarding.json` | ui | disable onboarding | JSON action (missing paths become warnings) |
| `81-ui-disable-onboarding.patch` | ui | disable onboarding | applied unchanged |
| `linux/00-asset-fix-rpm-version.patch` | asset | fix rpm version | applied unchanged |
| `linux/00-build-improve-qemu.patch` | build | improve qemu | **adapted** to 1.147 |
| `linux/00-cli-fix-unused.patch` | cli | fix unused | applied unchanged |
| `linux/00-env-improve-xdg-path.patch` | env | improve xdg path | applied unchanged |
| `linux/00-log-use-state-home.patch` | log | use state home | applied unchanged |
| `linux/00-policy-use-custom-path.patch` | policy | use custom path | applied unchanged |
| `linux/00-terminal-cmd-in-shell.patch` | terminal | cmd in shell | applied unchanged |
| `linux/31-build-fix-dependencies.patch` | build | fix dependencies | **adapted** to 1.147 |
| `linux/41-arch-add-support.patch` | arch | add support | skipped (other architectures only) |
| `linux/42-arch-add-ppc64le.patch` | arch | add ppc64le | skipped (other architectures only) |
| `linux/43-arch-add-riscv64.patch` | arch | add riscv64 | skipped (other architectures only) |
| `linux/44-arch-add-loong64.patch` | arch | add loong64 | skipped (other architectures only) |
| `linux/45-arch-add-s390x.patch` | arch | add s390x | skipped (other architectures only) |
| `linux/46-arch-add-arm32.patch` | arch | add arm32 | skipped (other architectures only) |
| `linux/46-deb-use-libasound2t64-compat.patch` | deb | use libasound2t64 compat | **adapted** to 1.147 |

## Port-only patches

| Patch | Area | What it does |
|---|---|---|
| `96-port-compile-cache-uid-suffix.patch` | build | the compile-cache validation accepts Node's -<uid> suffix |
| `97-port-bundle-no-sourcemaps.patch` | build | VSCODE_BUNDLE_SOURCEMAPS=0 turns off the bundle's source maps (ADR 0001) |
| `98-port-sequential-bundle.patch` | build | VSCODE_BUNDLE_SEQUENTIAL=1 builds one entry point at a time |
| `99-port-fixups.patch` | agentHost | patch 53 rejects resolved in 12 files; restricted telemetry emptied out; --agent-host-ca unavailable |

## Prefixes

| Prefix | Family |
|---|---|
| `00-` | branding, UI, built-in extensions, remote, telemetry, security (general patches) |
| `1x-` | version and update |
| `2x-` | libraries replaced by VSCodium forks (keymap, policies, proxy) |
| `3x-` | build dependencies |
| `5x-` | Copilot removal |
| `6x-` | security and extension gallery |
| `9x-` | port-only (they don't exist in VSCodium) |
| `linux/` | Linux packaging and build |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-16 | minor | Translated to English |
| 2026-11-16 | major | Generated from `06-port/patches/` and the `apply.sh` skip list |
