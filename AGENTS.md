# Enhanced Clipboard — AI Guide

This file tells coding agents how to change this repo safely.
Prefer small, local changes that preserve the existing architecture and behavior.
If a request conflicts with these rules, call out the conflict explicitly before making a risky change.

## 1. Highest-Priority Rules
- Windows only. Do not spend effort on cross-platform compatibility unless explicitly asked.
- Preserve layering: UI -> Store -> API (`src/composables/*Api.ts`) -> Tauri command -> Rust service -> DB.
- Keep `commands.rs` thin. Put validation, orchestration, pruning, and business rules in `services/`.
- Components and stores must not call Tauri `invoke()` directly. IPC belongs in `src/composables/*Api.ts`.
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
- `src/composables/` is for domain API wrappers only, for example `clipboardApi.ts`, `settingsApi.ts`, `persistedStateApi.ts`, `appInfoApi.ts`, and `runtimeApi.ts`.
- Use Tailwind for layout/spacing only. Use CSS variables for colors. Use `<Icon />` for icons.
- User-visible failures should go through a shared notice/dialog path instead of per-component alerts.
- Async UI actions should use a shared error-handling path when the action is user-triggered.
- Background or auto-triggered work such as pagination should avoid blocking modal error UX and should prefer a local inline error/retry state.
- `globalNow` is the source for frontend TTL-based hiding.
- Frontend should consume shared runtime info and shared constants from the `AppInfo` flow instead of hardcoding duplicate values.
- Frontend runtime consumption should go through the runtime store. Do not scatter raw runtime event listeners across pages/components.
- Frontend theme application must use a single derived `effectiveTheme`. Do not bind `data-theme` directly to saved settings except through that shared derivation.
- Clipboard entry list state should treat backend entry events as the source of truth. Do not locally infer final pin/unpin list state from command return values when retention may remove items afterwards.

### Backend
- Rust owns system access, clipboard integration, persistence, validation, pruning, and recovery decisions.
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
- Delete order matters: DB mutation first, file cleanup second.
- On record removal, always remove associated image files.

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
- Pinned entries appear only on the default unfiltered first page.
- Search, `entryType`, and date-filtered results must be strict matches and must not inject pinned entries automatically.
- `get_active_dates` and `get_earliest_month` must use the same TTL visibility rules as list queries, while still treating pinned entries as visible.
- `ClipboardEntriesQuery` filtering semantics must stay centralized. When adding a new query field, update the shared filtered-state rule used by pinned inclusion instead of scattering new special cases.

## 5. Prune Rules
- Prune runs before insert, after unpin, and after settings changes that affect retention.
- Prune order is fixed:
  1. Remove expired non-pinned entries by TTL.
  2. Trim remaining non-pinned history to `max_history`.
- The common prune path trims strictly to `max_history`; do not rely on a buffer above the configured non-pinned limit.
- Keep the insert-time pre-prune reservation flow until insert ordering becomes strictly stable enough to guarantee a newly inserted non-pinned entry will not be immediately trimmed by the shared prune path.
- File cleanup must happen for every removed image-backed entry.

## 6. Image Pipeline Rules
- Store original PNGs in `images/`.
- Store thumbnails in `thumbnails/`.
- The UI display source is `thumbnail_path` only.
- Use `getImageSrc()` to display image files.
- Expected async flow:
  1. Emit `entry_added` with `image_path = null` and `thumbnail_path = null`.
  2. Save PNG.
  3. Generate thumbnail.
  4. Update DB.
  5. Emit `entry_updated` with the full final `ClipboardEntry`.
- If `thumbnail_path == image_path`, the original is small enough to display directly.
- There is no startup thumbnail repair flow right now. Do not assume one exists.
- Copying an image entry intentionally writes a file list to the clipboard, not a raw bitmap.
- While an image entry is still processing (`thumbnail_path == null`), the UI should treat copy as unavailable instead of relying on a predictable backend failure.
- Image display load failure should remove the broken entry from the store.

## 7. Event Contracts
| Event | Payload | Meaning |
|---|---|---|
| `entry_added` | `ClipboardEntry` | Insert item into the UI immediately |
| `entry_updated` | `ClipboardEntry` | Existing item reached its final updated state and still exists |
| `entries_removed` | `string[]` | Remove entries after delete, clear, or prune |
| `runtime_status_updated` | `RuntimeStatusPatch` | Runtime status patch changed |

- Keep event payloads stable unless the change is intentional and all consumers are updated together.
- Clipboard watcher failures must update runtime status, not just logs.
- Runtime update events are patch-only. Frontend should fetch the full snapshot once at startup and merge patches afterwards.
- `entry_updated` is a final-state event, not a step-by-step process log. If an operation ends with an item removed, emit only `entries_removed` for that id instead of `entry_updated` followed by removal.
- Frontend entry-list stores must re-check whether an `entry_updated` payload still belongs to the current filtered view. Do not blindly upsert updates that should now disappear from a filtered list.
- In filtered views, prefer a backend single-entry query resolution path for `entry_added` / `entry_updated` reconciliation. Do not rebuild frontend-side query matchers or fall back to full-list reloads for this case.

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
- Supported save strategies are:
  1. `persist_only`
  2. `persist_then_apply`
  3. `apply_then_persist`
- `persist_then_apply` means the DB value is the saved user intent. If the runtime effect fails, return an effect failure but keep the DB value.
- `apply_then_persist` means the runtime state must succeed first. If apply fails, do not write the new DB value.
- Effect reporting should stay grouped by effect key such as `autostart`, `hotkey`, `retention`, `log_level`, and `always_on_top`.
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

## 12. What Good Changes Look Like
- Reuse the existing store / service / event architecture.
- Add the rule in the layer that owns it.
- Keep backend behavior deterministic and explicit.
- Preserve user-visible consistency across list, search, date filtering, prune, and runtime error states.
- Prefer durable rules over patch-specific hacks.

## 13. Before Finishing a Change
Sanity-check these when relevant:
- Did business logic stay in Rust services instead of leaking into commands or Vue components?
- Did new IPC stay inside the appropriate `src/composables/*Api.ts` module?
- Did hooks stay in `src/hooks/` without owning Tauri command IPC?
- Did shared runtime info / shared constants come from Rust `AppInfo` instead of duplicated frontend constants?
- Did you preserve cursor pagination and TTL semantics?
- Did delete/clear/prune keep DB-first, file-cleanup-second ordering?
- Did frontend-visible errors go through the shared UX path?
- Did background failures avoid noisy blocking UX?
- Did tray/i18n/runtime-status behavior stay consistent?
