# ADR 0003: The local build does not check for updates

- **Status:** accepted (2026-11-16, approved by the owner; applied in `06-port/apply.sh`)
- **Build:** local VSCodium 1.147.0

## Context

VSCodium points `updateUrl` to `raw.githubusercontent.com/VSCodium/versions`, which announces the official binary (1.143.x in 2026-11). This build is a different version (1.147.0, from `main`). The channel would offer to replace the app with a different binary and would make a useless periodic query to GitHub.

## Decision

`apply.sh` deletes `updateUrl` from `product.json`.

Containment rules:

1. Nothing else changes. VSCodium patches 11 and 12 (updates via GitHub releases and a minimum interval) stay applied; they just have no effect.
2. Without `updateUrl`, the service is permanently disabled in `abstractUpdateService.ts` (`setDisabledPermanently(DisablementReason.MissingConfiguration)`), with no error.

## The guarantee

No update query leaves the app (measured: the network test shows no `VSCodium/versions`; [`09-findings/0001`](../../09-findings/0001-network-traffic.md)).

## Consequences

- The app never announces a new version; updating means redoing the build.
- `raw.githubusercontent.com` still appears in the traffic, but only for the Eclipse Foundation's malicious-extension list.
