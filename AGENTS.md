# Enhanced Clipboard — AI Guide

This file tells coding agents how to change this repo safely.
Prefer small, local changes that preserve the existing architecture and behavior.
If a request conflicts with these rules, call out the conflict explicitly before making a risky change.

## 1. Highest-Priority Rules
- Windows only. Do not spend effort on cross-platform compatibility unless explicitly asked.
- Preserve layering: UI -> Store -> API (`src/composables/*Api.ts`) -> Tauri command -> Rust service -> DB.
- Keep `commands.rs` thin. Put validation, orchestration, pruning, and business rules in `services/`.
- Components and stores must not call Tauri `invoke()` or `listen()` directly. Tauri IPC/event binding belongs in focused wrappers under `src/composables/*Api.ts`.
- Commands return `Result<T, String>`. Do not introduce `unwrap()` / `expect()` / `panic!` on normal runtime paths.
- Prefer existing event-driven flows over ad hoc refreshes when updating clipboard state.
- Rust `AppInfo` is the authority for shared read-only environment info and cross-layer constants. Do not re-define those values in the frontend.
- `RuntimeStatus` is separate from `AppInfo`. Keep changing health/runtime signals in `RuntimeStatus` instead of mixing them into the read-only startup payload.
- `RuntimeStatus` is a read-only in-memory runtime snapshot. Do not persist it or mix saved user intent into it.
- Runtime updates must flow through the shared runtime patch/update path. Do not directly lock and mutate `RuntimeStatusState` outside the runtime service.
- Theme intent and theme facts must stay split: store the user preference as `AppSettings.theme_mode`, store the current OS theme as `RuntimeStatus.system_theme`, and derive the UI's final `effectiveTheme` on the frontend.

## 2. Architecture Boundaries

### Frontend
- Frontend owns rendering, view state, transient UI state, and user interaction flow.
- `src/hooks/` is for reusable `use*` hooks only. Do not put Tauri IPC in hooks.
- `src/composables/` is for focused Tauri API/event wrappers only, for example `clipboardApi.ts`, `settingsApi.ts`, `persistedStateApi.ts`, `appInfoApi.ts`, `runtimeApi.ts`, and narrow lifecycle wrappers such as `uiLifecycleApi.ts`.
- List UI must consume `ClipboardListItem` read models, not raw `ClipboardEntry` domain entities.
- Read-model protocol fields such as `preview`, its typed variants, and snapshot stale reasons must stay typed/centralized on both Rust and TypeScript sides. Do not add new magic string variants in component or store code.
- Keep clipboard view coordination in focused stores/hooks such as stream, query/snapshot, actions, calendar metadata, and view coordination. Do not recreate one giant clipboard store.
- Keep clipboard view hooks split by role: `useClipboardCurrentList()` for current list display, `useClipboardSearchControls()` for query/calendar controls, and `useClipboardPageLifecycle()` for page-level initialization and rare cross-store commands.
- Keep cross-store clipboard event coordination out of individual stores. Stream/query/calendar stores should own their state; small view coordination hooks may connect them for view-facing events.
- Search membership, preview/snippet generation, and highlight planning must share one backend-owned search semantic. Frontend may render returned highlight ranges, but it must not derive match membership or guess highlight positions from the query on its own.
- Search UI uses plain text input plus committed command-filter chips. Do not reintroduce inline `type:` parsing as the primary search UX.
- The search command palette currently opens with `/` from the search input. If the root command palette is already open, pressing `/` again should fall back to inserting a literal `/` into the normal search text.
- Use Tailwind for layout/spacing only. Use CSS variables for colors. Use `<Icon />` for icons.
- User-visible failures should go through a shared notice/dialog path instead of per-component alerts.
- Async UI actions should use a shared error-handling path when the action is user-triggered.
- Background or auto-triggered work such as pagination should avoid blocking modal error UX and should prefer a local inline error/retry state.
- `globalNow` is the source for frontend TTL-based hiding.
- Frontend should consume shared runtime info and shared constants from the `AppInfo` flow instead of hardcoding duplicate values.
- Frontend runtime consumption should go through the runtime store. Do not scatter raw runtime event listeners across pages/components.
- Frontend theme application must use a single derived `effectiveTheme`. Do not bind `data-theme` directly to saved settings except through that shared derivation.
- Clipboard stream state should treat backend stream item events as the source of truth. Do not locally infer final pin/unpin list state from command return values when retention may remove items afterwards.
- Default history is a stream view with incremental updates. Search/filter/date/tag views are snapshot views; structural changes should mark snapshots stale instead of rebuilding frontend query membership logic. The current UI mode should be represented explicitly as `stream` or `snapshot`, not inferred ad hoc in components.

### Backend
- Rust owns system access, clipboard integration, persistence, validation, pruning, and recovery decisions.
- Rust owns list read-model projection. DB/repository access returns raw domain entities; projection/query services build `ClipboardListItem`.
- Search canonicalization, match planning, preview/excerpt generation, and highlight-range calculation belong in backend search/projection services, not in DB access or frontend stores.
- Rust owns the canonical `AppInfo` payload, including shared environment info and shared constants used by the frontend.
- Rust owns the canonical `RuntimeStatus` payload for changing runtime health/status.
- Backend logs stay in English.
- Frontend-visible strings returned from backend must use i18n.
- Runtime degradation should surface via events or status commands, not by assuming Rust can show UI directly.
- Watchers and other runtime sources may detect changes, but runtime merge, dedupe, and frontend notification belong to the shared runtime service.
- Watchers may observe system theme changes, but they must only report `system_theme` through the shared runtime patch path. They must not decide whether the UI should use light, dark, or system mode.

## 3. Data, DB, and Persistence Invariants
- `clipboard.db` uses SQLCipher-backed `rusqlite`.
- `settings.db` remains plain SQLite.
- Settings persistence code belongs in the Rust `db` layer. Keep settings business orchestration in `services/settings.rs`.
- Non-settings persisted UI state such as window position and `always_on_top` lives alongside settings in `settings.db`, but it is a separate `PersistedState` concept. Keep its orchestration in `services/persisted_state.rs`.
- The clipboard DB raw key is stored in Windows Credential Manager via `keyring`.
- Timestamps are Unix epoch seconds (`i64`) only. Do not introduce ISO timestamp storage.
- Pagination must use cursor pagination on `(created_at DESC, id DESC)`. Do not use `OFFSET`.
- On schema changes, rebuild the table directly. Do not add migration machinery.
- Delete order matters: DB mutation first, artifact cleanup second.
- On record removal, always remove associated artifact files.

### Recovery policy
- This project is still pre-release.
- Only recreate `clipboard.db` for confirmed unrecoverable decrypt/open cases on an existing DB, such as key mismatch or "not a database".
- Do not recreate the DB for generic open failures such as file locks or transient I/O issues.

## 4. Clipboard Domain Rules
- Text limit: 1 MB.
- Image limit: 100 MB.
- Max history: 10000. Default: 500.
- `max_history` limits non-pinned entries only. Pinned entries are excluded from that count.
- Max pinned entries: 3.
- Pinned entries never expire and are never auto-deleted.
- Pinned entries participate in first-page list results for any query, but they are fetched separately from non-pinned pagination and must not consume the non-pinned page size.
- Search, `entryType`, and date-filtered results must be strict matches. Only pinned entries that match the active query may appear; do not inject non-matching pinned entries automatically.
- Text search semantics must have one backend-owned source of truth. Membership queries, preview/snippet generation, and highlight ranges should all derive from the same canonical search text and match-planning rules, even if candidate retrieval and final projection use different implementation layers.
- `ClipboardEntry.content` is raw domain data. Never rewrite it into preview text for list APIs or events.
- Canonical searchable text may differ from raw `ClipboardEntry.content`, but it is backend-owned derived data. Do not duplicate canonicalization logic in the frontend.
- Highlight ranges, when provided, are based on the projected preview text delivered to the frontend, not raw content.
- `get_active_dates` and `get_earliest_month` must use the same TTL visibility rules as list queries, while still treating pinned entries as visible.
- `ClipboardEntriesQuery` filtering semantics must stay centralized. When adding a new query field, update the shared query-filter path used by both pinned and non-pinned lookups instead of scattering new special cases.
- Entry semantic tags are attrs, not content types. Keep `content_type` for the clipboard payload carrier such as text / image, and expose semantic labels through `ClipboardEntry.tags`.
- The attrs/tag data model supports multiple semantic tags per entry, but the current backend detector intentionally emits at most one primary tag for text entries. Do not expand current output to multi-tag detection unless explicitly requested.
- Frontend should treat `ClipboardListItem.tags` as the public list tag surface. Do not inspect or expose raw attrs tables or invent frontend-side semantic detection.
- Tag presentation is currently informational only. Do not couple tag display to filtering, command search, or new tag interactions unless explicitly requested.

## 5. Clipboard Lifecycle And Retention
- `ClipboardEntry.status` is the only persisted lifecycle state: `pending` or `ready`.
- Do not add persisted failed entries or artifact lifecycle states. Failed image ingest work deletes pending entries.
- `ClipboardEntry` must stay domain-only; list image paths are `ClipboardListItem` projection fields.
- Entry state is durable history state. Pending image entries are recoverable only through active durable `image_ingest` jobs.
- `image_ingest` is the only implemented durable job kind. Keep future job kinds as schema/enum shape only unless explicitly requested.
- Every pending image entry must have an active `image_ingest` job with a recoverable staged input. Missing input or missing active job means remove the pending entry.
- `services/image_ingest/` owns image ingest capture, staging, claim/run, retry/exhaustion, startup recovery, and cleanup planning.
- Artifacts live in `clipboard_entry_artifacts` with roles such as `original` and `display`.
- Staging files live under `staging/`; they are job inputs, not committed artifacts, and must not enter `clipboard_entry_artifacts`.
- Image ingest staging is raw `rgba8` with explicit width/height/byte-size metadata. Do not rely on implicit clipboard library layout.
- Store image originals under `images/` and display assets under `thumbnails/`; never intentionally point `thumbnail_path` at the original.
- `files/` and `previews/` are reserved artifact roots. Persistent files there need artifact rows, or maintenance may remove them as old orphans.
- Retention applies only to `is_pinned = 0 AND status = 'ready'`. It must not depend on content type, artifact role, file existence, or projection fields.
- Retention order is fixed: TTL expiration first, then `max_history` trimming by `(created_at DESC, id DESC)`.
- Ready text inserts and deferred image finalization must use the shared pipeline/retention path.
- If retention removes a just-finalized entry, emit only removal effects for that id.

## 6. Clipboard Event Flow
- Frontend list payloads are `ClipboardListItem` read models. Components must not inspect artifact rows or raw image files directly.
- Image preview modes are semantic: `pending` disables copy, `ready` shows the display asset, and `repairing` keeps copy available from the original while display is rebuilt.
- `clipboard_jobs` is the source of truth for deferred image ingest lifecycle. Do not put job lifecycle ownership back into worker memory.
- Image capture commits in this order: write staging input, atomically insert pending entry plus queued job, then emit the pending list event.
- `services/jobs.rs` is process-level worker wake/loop and polling dedup only; job claim/run/recovery policy belongs in `services/image_ingest/`.
- `services/pipeline.rs` stays shared entry/effects/retention orchestration. It must not read staging, write image artifacts, or decide image retry policy.
- Worker wake failure must not roll back an already committed pending entry/job; startup recovery can resume it.
- Post-commit event failure must not roll back DB, cancel jobs, or clear dedup by itself.
- Keep dedup split: polling dedup is process-local compare-and-clear state; in-flight dedup is enforced by active queued/running DB jobs.
- User delete/clear of pending entries must remove DB state first, schedule staging/generated cleanup second, and only compare-clear polling dedup for the current key.
- Image display load failure is repair, not deletion, when the original exists. Delete the entry only when the original is missing or unrecoverable.

## 7. Events, Effects, And Maintenance
- Keep event payloads stable unless every producer and consumer is updated together.
- Event names stay centralized in Rust constants and frontend composable wrappers.
- Clipboard list events are view-facing stream events, not canonical domain events.
- Stream events update the default history stream. Snapshot/search/date/tag views must rely on typed stale reasons and explicit refreshes.
- Use the shared `ClipboardQueryStaleReason` enum/union. Do not pass ad hoc stale strings.
- `clipboard_stream_item_updated` means the final list projection changed. If the operation ends in removal, emit removal only.
- DB mutation is the business success boundary. `PipelineEffects` / `EffectsApplier` own list events, stale events, final projection re-read, and artifact cleanup scheduling.
- DB-backed artifact cleanup must go through the shared effects path and run after DB mutation and event attempts.
- Startup recovery is lightweight: job recovery handles pending image consistency, artifact repair validates ready image paths, and neither path does heavy decode/rebuild/orphan scanning.
- On startup, previous running `image_ingest` jobs become queued; active jobs with existing input remain recoverable; missing input or pending-without-active-job removes the pending entry.
- Startup recovery events are best-effort. The initial frontend snapshot remains authoritative.
- This is a personal-tool durable job boundary, not a generic enterprise scheduler. Do not add multi-worker scheduling, long-term job history, persisted failed entries, or complex retry/backoff unless explicitly requested.
- `image_ingest` cleanup must not plan cleanup for future job-kind inputs. Future job kinds need their own owner before their files can be interpreted.
- Background artifact maintenance owns display rebuilds, broken-original cleanup, and old orphan cleanup. Keep that policy in `services/artifacts/maintenance.rs`.
- Maintenance may make repair DB writes, but normal image pending-to-ready finalization belongs to `services/image_ingest/` and the shared pipeline/effects helpers.
- Common layers such as retention, delete/clear, effects, cleanup, and startup wiring must not construct image-specific paths themselves. Ask `services/image_ingest/` or the image artifact module for staging/generated candidates.

## 8. Settings Rules
- `AppInfo` is a flat read-only startup payload. Keep it as a single object with top-level fields such as `locale`, `version`, `os`, defaults, limits, presets, and option lists.
- `RuntimeStatus` is read-only from the frontend point of view and is refreshed by commands/events rather than persisted in `settings.db`.
- Runtime patches should be merged through the runtime service and reflected in the frontend runtime store, not mirrored into settings or persisted stores.
- `AppSettings.theme_mode` is the saved user preference. `RuntimeStatus.system_theme` is the live OS fact. Do not persist `system_theme`, and do not mirror `theme_mode` into runtime.
- `get_settings` / `save_settings` are the only source of truth for the settings domain.
- `get_persisted` / `save_persisted` are the only source of truth for the persisted UI-state domain.
- Settings-related IPC belongs in `settingsApi.ts`, not in clipboard-facing API modules.
- Persisted UI state IPC belongs in `persistedStateApi.ts`, not in hooks or components.
- Frontend must not talk to the autostart plugin directly.
- Frontend `save_settings` and `save_persisted` calls should submit only changed fields; backend should merge the patch and apply side effects only for the fields that actually changed.
- Getter commands must be pure DB reads. Do not add runtime overlay, reconcile, or DB write-back behavior to getters.
- All settings/persisted save semantics must be driven by field metadata, not ad hoc field-name branches spread across services.
- `capture_images` is a watcher ingest setting. Changing it should update future capture behavior only, not trigger retention prune/reload semantics for existing history.
- Supported save strategies are:
  1. `persist_only`
  2. `persist_then_apply`
  3. `apply_then_persist`
- `persist_then_apply` means the DB value is the saved user intent. If the runtime effect fails, return an effect failure but keep the DB value.
- `apply_then_persist` means the runtime state must succeed first. If apply fails, do not write the new DB value.
- Effect reporting should stay grouped by effect key such as `autostart`, `hotkey`, `retention`, `capture_images`, `log_level`, and `always_on_top`.
- `save_settings` should return the final DB-backed `settings` plus per-effect results. The frontend should update local saved/draft state from that payload instead of refetching.
- `save_persisted` should return the final DB-backed `persisted` plus per-effect results for affected runtime fields.
- Locale selection is not a user setting. UI/backend i18n must follow `AppInfo.locale` instead of introducing a settings override.
- `AppSettings` contains only settings-page data. Do not mix window position, `always_on_top`, or other best-effort UI state into `AppSettings`.
- `PersistedState` is for non-settings restored UI/window state such as `window_x`, `window_y`, and `always_on_top`.
- Window position saves should go through `save_persisted` with a position-only patch instead of a side-channel persistence helper.
- `settingsStore` should keep a settings-page-friendly `savedSettings` / `draftSettings` split with a direct dirty check and patch save flow.
- `persistedStateStore` should keep a single persisted snapshot and update from `save_persisted` results without introducing settings-page draft semantics.
- Startup recovery belongs in explicit startup restore functions, not in getters.
- `restore_settings_effects` and `restore_persisted_effects` restore saved side effects on startup. They are not runtime snapshot APIs and should not be renamed back to `restore_runtime`.

## 9. I18n and Text
- Frontend-visible text, tray labels, and backend error strings shown to the frontend must use i18n.
- Backend logs must not depend on i18n.
- Locale matching must use full locale tags from `AppInfo.locale`; if no exact translation file exists, fall back to `en-US`, then to the string key.
- Frontend and Rust backend share the same locale JSON files. Keep dynamic messages in named placeholders such as `{count}`, `{time}`, and `{list}` instead of prefix/suffix key splitting or positional `%s`-style placeholders.
- Keep placeholder handling lightweight and deterministic. If a string needs locale-aware date/time/number formatting, format the value first with `Intl` on the frontend or the appropriate runtime formatter, then inject the final string into the i18n template.
- Do not rely on literal `{name}` text inside translations unless it is meant to be a placeholder; the current lightweight formatter does not provide an escaping syntax for literal braces.
- Treat repo files as UTF-8 unless proven otherwise.
- When reading Chinese text in the terminal, use UTF-8-safe reads such as `Get-Content -Encoding utf8`.
- If terminal output looks garbled, re-read safely before claiming the file is corrupted.

## 10. Security and Config
- Keep CSP defined.
- Keep `assetProtocol.scope` restricted. Do not widen it to `["**"]`.

## 11. Git Rules
- Before implementing a new feature, create a dedicated git branch. Do not develop new features directly on `master`.
- After the user confirms the change set is ready, create a commit with an English commit message that follows Conventional Commits.
- After committing confirmed work, open a pull request targeting the main branch (`master`) and leave it open unless the user asks for something else.

## 11.5 Testing Conventions
- Frontend tests live under `src/tests/frontend/`.
- Backend Rust tests live under Cargo integration tests in `src-tauri/tests/`.
- Do not add new file-internal implementation tests back into production source files when the test belongs in the shared test tree.
- Prefer behavior-focused tests that assert public service/store/component contracts over tests that only inspect internal implementation details.

## 12. What Good Changes Look Like
- Reuse the existing store / service / event architecture.
- Add the rule in the layer that owns it.
- Keep backend behavior deterministic and explicit.
- Preserve user-visible consistency across list, search, date filtering, prune, and runtime error states.
- Prefer durable rules over patch-specific hacks.

## 13. Before Finishing a Change
Sanity-check these when relevant:
- Did business logic stay in Rust services instead of leaking into commands or Vue components?
- Did raw `ClipboardEntry` stay raw, with list display using `ClipboardListItem` projection?
- Did stream views and snapshot views keep their distinct semantics?
- Did new IPC stay inside the appropriate `src/composables/*Api.ts` module?
- Did hooks stay in `src/hooks/` without owning Tauri command IPC?
- Did shared runtime info / shared constants come from Rust `AppInfo` instead of duplicated frontend constants?
- Did you preserve cursor pagination and TTL semantics?
- Did delete/clear/prune keep DB-first, file-cleanup-second ordering?
- Did frontend-visible errors go through the shared UX path?
- Did background failures avoid noisy blocking UX?
- Did tray/i18n/runtime-status behavior stay consistent?
