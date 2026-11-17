# ADR 0029: Extensions are managed through Open VSX's REST API, stored in VS Code's layout in rustode's own folder, and never loaded

- **Status:** proposed (2026-11-17, by the ticket 40 agent; applied in `crates/workspace/src/extensions.rs` and `crates/workspace/src/extensions/`). Two parts are owner decisions, listed under "Owner decisions"
- **Build:** rustode at `f56cd87` (branch `feat/extensions-open-vsx`); studied: the source tree of the App (local VSCodium 1.147.0)

## Context

M8 of Phase 1 is extension management: search Open VSX, install, uninstall and list, with nothing loaded ([ADR 0018](0018-phase-1-milestones.md), rules 3 and 4). The installed extensions must be stored in VS Code's format, so a later extension host needs no migration (spec story 114), and the Microsoft Marketplace and Microsoft's proprietary extensions are out of scope (AGENTS.md §1.2).

What the App does:

- Its gallery is Open VSX through the VS Code-compatible gallery API: `extensionsGallery.serviceUrl` is `https://open-vsx.org/vscode/gallery` (`02-engenharia-reversa/product.json`). Searches are POSTs of a filter, flag and sort body to `{serviceUrl}/extensionquery` with `Accept: application/json;api-version=3.0-preview.1` (`src/vs/platform/extensionManagement/common/extensionGalleryManifestService.ts:48-52`, `:88-200`; `src/vs/platform/extensionManagement/common/extensionGalleryService.ts:1452-1512`). It contacts `open-vsx.org` and `openvsx.eclipsecontent.org` when the Extensions view opens ([finding 0001](../../09-analises/0001-trafego-de-rede.md)).
- It checks downloads with a signature verified by the `@vscodium/vsce-sign` module (`src/vs/platform/extensionManagement/node/extensionSignatureVerificationService.ts:70`), not by a checksum.
- It stores each extension in `<extensions folder>/<id>-<version>[-<platform>]/` with an `extensions.json` index, and deletes uninstalled extensions at its next start (the files are cited in `crates/workspace/src/extensions/storage.rs` and in `ATTRIBUTION.md`).

## Decision

1. **Open VSX's REST API, not the gallery API.** Search is `GET /api/-/search?query=…&size=50&offset=0&sortBy=relevance&sortOrder=desc`; an install reads `GET /api/{namespace}/{name}` (and `/api/{namespace}/{name}/linux-x64` when the latest release is another platform's build), then downloads `files.download` and checks it against `files.sha256`. Reasons:
   - the REST API publishes a SHA-256 for every file, so a download can be checked without the signature module, which rustode does not have;
   - the gallery API is the Microsoft Marketplace's protocol; a client that speaks only Open VSX's own API cannot be pointed at the Marketplace by changing a URL;
   - the answers are plain JSON, without flag bitmasks or asset-type tables.

   The search behavior is VS Code's: the text is trimmed and cut at 200 chars, 50 results a page (`extensionGalleryService.ts:1173-1203`). Hypothesis, not measured (rustode was not run against the real Open VSX): the hosts contacted are the App's, the registry and the storage host its download links redirect to.
2. **The Marketplace is refused before any request.** Every URL is checked before it is fetched, redirects included (redirects are followed by hand, at most 5): hosts under `visualstudio.com`, `vsassets.io` and `vscode-unpkg.net` are refused, ignoring case and a trailing dot; only `https` is fetched, and `http` only on this machine (`localhost`, `127.*`, `[::1]`).
3. **No unchecked install.** A release without a published SHA-256, a download whose SHA-256 differs, a VSIX whose `package.json` names another extension or version (as VS Code checks, `src/vs/platform/extensionManagement/node/extensionManagementService.ts:309-311`), and a VSIX entry whose path leaves the extension folder all fail the install and leave nothing behind. rustode also caps a download at 512 MiB, an extraction at 100,000 entries and 1 GiB (estimated as generous for real VSIX files; VS Code sets no cap).
4. **VS Code's storage layout, in rustode's own folder.** The folder is `RUSTODE_EXTENSIONS_DIR`, else `$XDG_DATA_HOME/rustode/extensions`, else `~/.local/share/rustode/extensions` (`rustode_workspace::default_extensions_dir`); never VS Code's or VSCodium's folders. Inside it, VS Code's layout: `<id>-<version>-<platform>/` with the VSIX's `extension/` contents and `__metadata` in its `package.json`; `extensions.json`; `.obsolete` and `.vsctmp` during deletions. Uninstalling deletes at once, because nothing holds the files open in Phase 1.
5. **HTTP stack.** `ureq` 3 with rustls on ring and the system's root certificates (`rustls-native-certs`); not ureq's `rustls` feature, which pulls `webpki-roots` (CDLA-Permissive-2.0, not allowed by `deny.toml`). `zip` 9 with the zlib-rs backend reads VSIX files; `sha2` checks them.
6. **Never on the frontend's thread.** Searches run on threads of their own (a newer search replaces an older one's results); installs, uninstalls and listings run one at a time on one worker thread, the folder's only writer. Results arrive through `Workspace::poll_extensions`, and `Workspace::set_extensions_waker` lets a frontend that sleeps until input repaint when they arrive.
7. **Nothing is loaded.** `InstalledExtension::is_loaded` is always false, and both frontends show `NOT_LOADED` ("Not loaded in this phase") on every installed extension.

Containment rules:

1. All network and extension-folder code lives in `crates/workspace/src/extensions/`; the frontends call the workspace (`search_extensions`, `install_extension`, `uninstall_extension`, `refresh_extensions`, `execute_with`) and render its state ([ADR 0013](0013-crates-and-workspace-api.md), [ADR 0014](0014-in-process-frontends.md)).
2. The Marketplace domains are written as domains in one constant (`MARKETPLACE_DOMAINS`, `registry.rs`), never as full hosts, so the semgrep rule `rustode.microsoft-marketplace-or-telemetry` keeps flagging any literal Marketplace host elsewhere. The tests write such hosts in mixed case, which also checks that the refusal ignores case.
3. Tests use the local stand-in `rustode_workspace::extensions::stand_in` (an HTTP server on 127.0.0.1 that also builds VSIX files). It is compiled only with the workspace crate's `stand-in` feature, which the workspace (through a dev-dependency on itself), the TUI and the GUI enable in their dev-dependencies, so it never ships in a binary. No test touches the network or a real profile ([ADR 0019](0019-tests-at-three-seams.md)).
4. Ticket 41 (enable and disable) builds on this layout and must not change it without a new decision.

## The guarantee

rustode never sends a request to a Microsoft Marketplace host, and never installs a file it could not check against Open VSX's published SHA-256.

Checks, from the project root: `cargo nextest run -p rustode-workspace -E 'binary(extensions)'` — the tests `the_microsoft_marketplace_is_never_contacted`, `registries_must_be_https_or_on_this_machine`, `a_download_whose_sha256_differs_is_refused_and_leaves_nothing`, `a_download_without_a_published_sha256_is_refused` and `an_entry_that_leaves_the_extension_folder_is_refused`. Measured at `f56cd87` on 2026-11-17 (debug build): the whole suite, `cargo nextest run --workspace --locked`, ran 341 tests, all passed (4 skipped).

## Consequences

- The workspace crate now links an HTTPS client: 32 more packages in `Cargo.lock` (478 to 510, measured at `f56cd87`), including ring, which compiles C code.
- Gaps against the App: no signature check (only the SHA-256); no search as you type, no `category:`/`tag:` filters, no paging beyond the first 50 results, no extension details page, no pre-release choice, no automatic updates; the `extensions.json` metadata has no gallery `id` (UUID), which the REST API does not give; JSON keys are written sorted, not in VS Code's order.
- A platform-specific extension without a linux-x64 build cannot be installed (the message says so).
- Anything that loads or runs an extension needs a new decision (ADR 0018, rule 2).

## Owner decisions

1. **The extensions folder** (a data location): rustode's own `$XDG_DATA_HOME/rustode/extensions`, overridable with `RUSTODE_EXTENSIONS_DIR`. Reversible: nothing else depends on the path.
2. **Refusing releases without a published SHA-256**, instead of installing them unchecked. Reversible in `Registry::release_at`.
