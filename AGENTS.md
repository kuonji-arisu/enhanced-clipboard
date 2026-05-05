# Enhanced Clipboard — AI Constraints

This file is a constraint set for coding agents, not a project manual.
Prefer small, local changes that preserve the existing architecture and behavior.
If a user request conflicts with these rules, call out the conflict before making a risky change.

## 1. Core Boundaries
- Windows only. Do not spend effort on cross-platform support unless explicitly asked.
- Preserve the layer order: UI -> Store -> API (`src/composables/*Api.ts`) -> Tauri command -> Rust service -> DB.
- Keep `commands.rs` thin. Validation, orchestration, pruning, recovery, and business rules belong in `services/`.
- Components, stores, and hooks must not call Tauri `invoke()` or `listen()` directly. IPC/event bindings belong in focused `src/composables/*Api.ts` wrappers.
- Tauri commands return `Result<T, String>`. Do not add `unwrap()`, `expect()`, or `panic!` on normal runtime paths.
- Prefer existing event-driven flows over ad hoc refreshes.
- Shared constants and read-only environment facts come from Rust `AppInfo`; do not duplicate them in the frontend.
- `RuntimeStatus` is live, read-only, in-memory runtime state. Do not persist it, mix it into `AppInfo`, or combine it with saved user intent.
- Runtime changes must flow through the shared runtime service patch/update path. Do not directly lock and mutate `RuntimeStatusState` elsewhere.
- Theme intent and theme facts stay split: `AppSettings.theme_mode`, `RuntimeStatus.system_theme`, and frontend-derived `effectiveTheme`.

## 2. Frontend
- Frontend owns rendering, view state, transient UI state, and user interaction flow.
- `src/hooks/` is for reusable `use*` hooks only; no Tauri IPC there.
- `src/composables/` is for focused API/event wrappers such as clipboard, settings, persisted state, app info, runtime, and UI lifecycle.
- List UI consumes `ClipboardListItem` read models, never raw `ClipboardEntry` domain entities.
- Read-model protocol fields, preview variants, and stale reasons must stay typed and centralized on both Rust and TypeScript sides. Do not invent magic strings in components or stores.
- Keep clipboard state split by role: stream, query/snapshot, actions, calendar metadata, and small view coordination. Do not rebuild a giant clipboard store.
- Keep clipboard view hooks split by purpose: current list, search/calendar controls, and page lifecycle.
- Cross-store clipboard event coordination belongs in view coordination code, not individual stores.
- Default history is `stream`; search/filter/date/tag-filter views are `snapshot`. Represent the mode explicitly.
- Backend stream item events are the source of truth for stream state. Do not infer final pin/unpin/list state from command return values.
- Snapshot views respond to typed stale reasons and explicit refreshes; do not rebuild query membership in the frontend.
- Search membership, canonicalization, snippets/previews, and highlight ranges are backend-owned. The frontend may render returned highlights only.
- Search UX is plain text plus committed command-filter chips. Do not reintroduce inline `type:` parsing as the primary search interface.
- From the search input, `/` opens the search command palette. If the root command palette is already open, `/` inserts a literal slash.
- Use Tailwind for layout/spacing only, CSS variables for colors, and `<Icon />` for icons.
- User-triggered failures go through the shared notice/dialog/error path. Background work such as pagination should prefer local inline retry/error state.
- `globalNow` remains the frontend source for TTL-based hiding.
- Runtime consumption goes through the runtime store; do not scatter raw runtime event listeners across pages/components.
- Apply theme through the shared `effectiveTheme`; do not bind `data-theme` directly to saved settings.

## 3. Backend
- Rust owns system access, clipboard integration, persistence, validation, pruning, recovery, and list read-model projection.
- DB/repository access returns raw domain entities; query/projection services build `ClipboardListItem`.
- Backend search/projection services own canonical search text, match planning, preview/excerpt generation, and highlight ranges.
- Backend logs stay in English.
- Frontend-visible backend errors must use i18n.
- Runtime degradation surfaces through events or status commands, not direct UI assumptions in Rust.
- Watchers may observe runtime facts, but merge, dedupe, and frontend notification belong to the shared runtime service.
- Theme watchers only report `system_theme` through runtime patches; they must not decide the final UI theme.

## 4. Data And Persistence
- `clipboard.db` uses SQLCipher-backed `rusqlite`; `settings.db` remains plain SQLite.
- The clipboard DB raw key is stored in Windows Credential Manager via `keyring`.
- Timestamps are Unix epoch seconds (`i64`) only; do not introduce ISO timestamp storage.
- Settings persistence belongs in the Rust DB layer; settings orchestration belongs in `services/settings.rs`.
- Non-settings UI persistence is `PersistedState`, stored with settings but orchestrated in `services/persisted_state.rs`.
- Keep `AppSettings` and `PersistedState` separate.
- Pagination uses cursors on `(created_at DESC, id DESC)`; never use `OFFSET`.
- On schema changes, rebuild tables directly. Do not add migration machinery.
- Deletion order is DB mutation first, artifact cleanup second.
- Public `Database` APIs that delete `clipboard_entries` must return `EntryJobCleanup` or `Option<EntryJobCleanup>`.
- Deletion APIs must collect raw cleanup side data in the same transaction before deleting entries: removed ids, committed artifact paths, and `image_ingest` cleanup records.
- The DB layer may collect `image_ingest` job cleanup rows, but must not interpret image cleanup semantics, generate candidate paths, or clear dedup state.
- Recreate `clipboard.db` only for confirmed unrecoverable decrypt/open failures on an existing DB, such as key mismatch or "not a database". Do not recreate it for file locks or transient I/O failures.

## 5. Clipboard Domain
- Clipboard limits and defaults are backend/AppInfo-owned. Do not duplicate numeric limits in frontend code or this document.
- Clipboard carrier probing uses explicit outcomes: accepted or ignored content means the carrier exists, while only absent/empty content may continue to lower-priority probes.
- `max_history` limits non-pinned entries only. Pinned entries are excluded from history trimming, never expire, and are never auto-deleted.
- Pinned entries are fetched separately for first-page list results and must not consume non-pinned page size.
- Search, `entryType`, date, and tag-filter results are strict filters. Only pinned entries that match the active query may appear.
- `ClipboardEntry.content` is raw domain data. Never rewrite it into preview text for list APIs or events.
- Canonical searchable text may differ from raw content, but it is backend-owned. Do not duplicate canonicalization in the frontend.
- Highlight ranges, when present, refer to the projected preview text delivered to the frontend.
- `get_active_dates` and `get_earliest_month` must share list-query visibility/TTL rules while treating pinned entries as visible.
- `ClipboardEntriesQuery` filtering semantics stay centralized; new query fields must update the shared pinned and non-pinned filter path.
- Semantic tags are attrs, not content types. Keep `content_type` for carriers such as text/image and expose tags through `ClipboardEntry.tags` / `ClipboardListItem.tags`.
- The frontend treats `ClipboardListItem.tags` as the public tag surface. Do not inspect raw attrs tables or invent frontend semantic detection.
- Tag display is informational unless the user explicitly requests tag interactions or filtering.

## 6. Lifecycle, Retention, And Image Ingest
- `ClipboardEntry.status` is the only persisted lifecycle state: `pending` or `ready`.
- Do not add persisted failed entries, terminal job state, or artifact lifecycle states.
- `ClipboardEntry` stays domain-only; list image paths are projection fields.
- Pending image entries are recoverable only through active durable `image_ingest` jobs with staged input. Missing job or input means remove the pending entry.
- `image_ingest` is the only implemented active ingest job kind. Future job kinds stay opaque until their sibling owner exists.
- `services/image_ingest/` owns image capture, staging input, jobs, generated-file lifecycle, claim/run, startup pending/job recovery, finalization, cleanup planning, and races around those resources.
- Do not reintroduce image sweepers, delayed full convergence, generic `content_ingest`, generic job-handler registries, multi-worker scheduling, long-term job history, or complex retry/backoff unless explicitly requested.
- Pending image finalization must go through durable `image_ingest` job finalization, not generic DB entry helpers.
- Artifacts live in `clipboard_entry_artifacts` with roles `original` and `preview`.
- Staging files live under `staging/`; they are job inputs, not committed artifacts.
- Store image originals under `images/` and preview assets under `thumbnails/`; never intentionally point `preview_path` at the original.
- `files/` and `previews/` are reserved for future managed file artifacts. Do not scan them for committed orphans.
- Retention applies only to non-pinned ready entries. It must not depend on content type, artifact role, file existence, or projection fields.
- Retention order is TTL expiration first, then history trimming by `(created_at DESC, id DESC)`.
- Ready text inserts and deferred image finalization use the shared pipeline/retention path.
- If retention removes a just-finalized entry, emit only removal effects for that id.
- `image_ingest::cleanup_plan_from_entry_removal` is the only layer that interprets `ImageIngestJobCleanupRecord`.
- Callers decide why entries are removed; `image_ingest` decides how image jobs, staging, generated files, and polling dedup are cleaned from deletion side data.
- Do not add ad hoc `image_ingest` cleanup logic in maintenance, prune, prepare-for-insert, retention, or common effects paths.
- User-triggered delete/clear and ready-image copy/load repair belong in `services/entry.rs`.
- Pending-image failure, stale runner handling, and startup pending/job recovery belong in `services/image_ingest/`.
- Retention-driven removal belongs in `services/prune.rs`.
- Background artifact maintenance may repair missing ready previews from originals, and may remove ready image entries only when the original is missing or broken. It must skip pending entries and must not proactively validate originals when a preview exists.
- Common layers must not construct image-specific paths themselves. Ask `services/image_ingest/` or the image artifact module.

## 7. Events, Effects, And Jobs
- Keep event names centralized in Rust constants and frontend composable wrappers.
- Keep event payloads stable unless every producer and consumer is updated together.
- Clipboard list events are view-facing stream events, not canonical domain events.
- Stream events update default history. Snapshot/search/date/tag-filter views rely on typed stale reasons and explicit refreshes.
- Use the shared `ClipboardQueryStaleReason` enum/union; do not pass ad hoc stale strings.
- `clipboard_stream_item_updated` means the final list projection changed. If an operation ends in removal, emit removal only.
- DB mutation is the business success boundary.
- `PipelineEffects` / `services/effects.rs` own list events, stale events, final projection re-read, and post-DB artifact cleanup scheduling.
- DB-backed cleanup runs after DB mutation and event attempts.
- Post-commit event failure must not roll back DB, cancel jobs, or clear dedup by itself.
- `clipboard_jobs` contains active recoverable jobs only; finalization deletes the job row.
- `services/jobs.rs` is process-level worker wake/loop and polling dedup only. Job claim/run/recovery policy belongs to the owning vertical service.
- Worker wake failure must not roll back an already committed pending entry/job; startup recovery can resume it.
- Keep dedup split: polling dedup is process-local compare-and-clear state; in-flight dedup is enforced by active DB jobs.
- Pending delete/clear must remove DB state first, schedule staging/generated cleanup second, and only compare-clear polling dedup for the current key.
- Any `image_ingest` path that may remove active jobs must own `ImageDedupState` and clear polling dedup through its `CleanupPlan`.
- Stale or duplicate ingest runners that do not own DB cleanup may clean staging inputs only. They must not delete committed `original` or `preview` artifacts.
- Image preview modes are semantic: `pending`, `ready`, and `repairing`.
- Ready image copy must verify the original artifact row/path/file. Missing originals remove the entry DB-first and emit removal/stale.
- Image preview load failure is repair, not deletion, when the original exists.
- Startup recovery is lightweight pending/job consistency repair only. Startup events are best-effort; the initial frontend snapshot remains authoritative.

## 8. Settings And Runtime Effects
- `AppInfo` is a flat read-only startup payload for locale, version, OS, defaults, limits, presets, and option lists.
- `get_settings` / `save_settings` are the only settings source of truth.
- `get_persisted` / `save_persisted` are the only persisted UI-state source of truth.
- Settings IPC belongs in `settingsApi.ts`; persisted UI-state IPC belongs in `persistedStateApi.ts`.
- The frontend must not talk to the autostart plugin directly.
- `save_settings` and `save_persisted` submit changed fields only; the backend merges patches and applies effects only for changed fields.
- Getter commands are pure DB reads. Do not add runtime overlays, reconciliation, or write-back behavior to getters.
- Save semantics are metadata-driven, not scattered field-name branches.
- Supported save strategies are `persist_only`, `persist_then_apply`, and `apply_then_persist`.
- `persist_then_apply` keeps the saved DB value even if the runtime effect fails.
- `apply_then_persist` writes the DB only after the runtime effect succeeds.
- Effect reporting stays grouped by effect key such as `autostart`, `hotkey`, `retention`, `capture_images`, `log_level`, and `always_on_top`.
- Save commands return final DB-backed state plus effect results; frontend stores update from that payload instead of refetching.
- Locale is not a user setting. UI/backend i18n follows `AppInfo.locale`.
- `AppSettings` contains settings-page data only. Window position, `always_on_top`, and similar restored UI state belong in `PersistedState`.
- Window position saves go through `save_persisted` with a position-only patch.
- `settingsStore` keeps `savedSettings` / `draftSettings`; `persistedStateStore` keeps one persisted snapshot.
- Startup recovery belongs in explicit startup restore functions, not getters.
- `restore_settings_effects` and `restore_persisted_effects` restore saved side effects on startup; do not rename them back to `restore_runtime`.

## 9. I18n And Text
- Frontend-visible text, tray labels, and backend errors shown to the frontend must use i18n.
- Backend logs must not depend on i18n.
- Locale matching uses full locale tags from `AppInfo.locale`; fall back to `en-US`, then the string key.
- Frontend and Rust share locale JSON files.
- Dynamic messages use named placeholders such as `{count}`, `{time}`, and `{list}`. Do not use prefix/suffix key splitting or positional `%s` placeholders.
- Format locale-aware date/time/number values before injecting them into i18n templates.
- Do not rely on literal `{name}` text inside translations unless it is meant to be a placeholder; the formatter has no escaping syntax for literal braces.
- Treat repo files as UTF-8 unless proven otherwise.
- When reading Chinese text in the terminal, use UTF-8-safe reads such as `Get-Content -Encoding utf8`.
- If terminal output looks garbled, re-read safely before claiming the file is corrupted.

## 10. Security
- Keep CSP defined.
- Keep `assetProtocol.scope` restricted. Do not widen it to `["**"]`.

## 11. Tests
- Frontend tests live under `src/tests/frontend/`.
- Backend Rust tests live under Cargo integration tests in `src-tauri/tests/`.
- Do not add file-internal implementation tests to production source files when the test belongs in the shared test tree.
- Prefer behavior-focused tests against public service/store/component contracts.

## 12. Git
- Before implementing a new feature, create a dedicated branch; do not develop features directly on `master`.
- Commit only after the user confirms the change set is ready.
- Use English Conventional Commit messages.
- After committing confirmed work, open a pull request targeting `master` and leave it open unless asked otherwise.

## 13. Finish Check
- Business logic stayed in Rust services.
- IPC stayed in `src/composables/*Api.ts`.
- Hooks stayed IPC-free.
- Raw domain entities stayed out of list UI.
- Stream and snapshot semantics stayed distinct.
- Search, TTL, retention, and pinned-entry semantics stayed backend-owned and centralized.
- Delete/clear/prune kept DB-first, cleanup-second ordering.
- Runtime/AppInfo/settings/persisted-state boundaries stayed separate.
- User-visible errors and background failures used the right UX paths.
- I18n, UTF-8 handling, CSP, and asset scope stayed intact.
