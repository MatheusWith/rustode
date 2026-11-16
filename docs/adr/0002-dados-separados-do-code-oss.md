# ADR 0002: The app's data lives in `.vscodium` and `.vscodium-shared`, separate from the system's Code - OSS

- **Status:** accepted (2026-11-16, at the owner's request: "sim, separe o diretório", i.e. "yes, separate the directory"; applied in `06-port/apply.sh`)
- **Build:** local VSCodium 1.147.0

## Context

Official VSCodium uses `dataFolderName` = `.vscode-oss` and `sharedDataFolderName` = `.vscode-oss-shared`. These are the same as Arch's `code` 1.138 installed on this machine (measured: `/usr/lib/code/product.json`). With the same directories, the two apps would share two things:
- `~/.vscode-oss/extensions` and `extensions.json`. An extension updated by 1.147 to `engines.vscode: ^1.147` would become incompatible on 1.138.
- The recents and folder trust in `~/.vscode-oss-shared` (QA finding P1).

## Decision

`apply.sh` writes `dataFolderName` = `.vscodium` and `sharedDataFolderName` = `.vscodium-shared` into `product.json`, after merging with VSCodium's `product.json`.

Containment rules:

1. Only these two keys change; `serverDataFolderName` stays `.vscodium-server`, as in VSCodium.
2. The change is recorded in `PORT-NOTES.md`, "Decisões locais".

## The guarantee

The built app never reads or writes the system `code`'s directories. The settings profile (`~/.config/VSCodium`) was already separate, through `nameShort`.

## Consequences

- Measured: `product.json` packaged with `.vscodium` and `.vscodium-shared` (`smoke.sh`).
- Difference from official VSCodium: someone migrating from the official build to this one won't see their installed extensions, which are in `~/.vscode-oss/extensions`.
