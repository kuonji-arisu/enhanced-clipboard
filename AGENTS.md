# Enhanced Clipboard — AI Guide

This file tells coding agents how to change this repo safely.
Prefer small, local changes that preserve the existing architecture and behavior.
If a request conflicts with these rules, call out the conflict explicitly before making a risky change.

## 1. Highest-Priority Rules
- Windows only. Do not spend effort on cross-platform compatibility unless explicitly asked.
- Preserve layering: UI -> Store -> API (`clipboardApi.ts`) -> Tauri command -> Rust service -> DB.
- Keep `commands.rs` thin. Put validation, orchestration, rollback, pruning, and business rules in `services/`.
- Components and stores must not call Tauri `invoke()` directly. IPC belongs in `clipboardApi.ts`.
- Commands return `Result<T, String>`. Do not introduce `unwrap()` / `expect()` / `panic!` on normal runtime paths.
- Prefer existing event-driven flows over ad hoc refreshes when updating clipboard state.

## 2. Architecture Boundaries

### Frontend
- Frontend owns rendering, view state, transient UI state, and user interaction flow.
- Use Tailwind for layout/spacing only. Use CSS variables for colors. Use `<Icon />` for icons.
- User-visible failures should go through a shared notice/dialog path instead of per-component alerts.
- Async UI actions should use a shared error-handling path when the action is user-triggered.
- Background or auto-triggered work such as pagination should avoid blocking modal error UX and should prefer a local inline error/retry state.
- `globalNow` is the source for frontend TTL-based hiding.

### Backend
- Rust owns system access, clipboard integration, persistence, validation, pruning, and recovery decisions.
- Backend logs stay in English.
- Frontend-visible strings returned from backend must use i18n.
- Runtime degradation should surface via events or status commands, not by assuming Rust can show UI directly.

## 3. Data, DB, and Persistence Invariants
- `clipboard.db` uses SQLCipher-backed `rusqlite`.
- `settings.db` remains plain SQLite.
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
- Max pinned entries: 3.
- Pinned entries never expire and are never auto-deleted.
- Pinned entries appear only on the default unfiltered first page.
- Search and date-filtered results must be strict matches and must not inject pinned entries automatically.
- `get_active_dates` and `get_earliest_month` must use the same TTL visibility rules as list queries, while still treating pinned entries as visible.

## 5. Prune Rules
- Prune runs before insert and after settings changes that affect retention.
- Prune order is fixed:
  1. Remove expired non-pinned entries by TTL.
  2. Trim remaining non-pinned history to `max_history`.
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
  5. Emit `entry_updated`.
- If `thumbnail_path == image_path`, the original is small enough to display directly.
- There is no startup thumbnail repair flow right now. Do not assume one exists.
- Copying an image entry intentionally writes a file list to the clipboard, not a raw bitmap.
- While an image entry is still processing (`thumbnail_path == null`), the UI should treat copy as unavailable instead of relying on a predictable backend failure.
- Image display load failure should remove the broken entry from the store.

## 7. Event Contracts
| Event | Payload | Meaning |
|---|---|---|
| `entry_added` | `ClipboardEntry` | Insert item into the UI immediately |
| `entry_updated` | `{ id, image_path, thumbnail_path }` | Async image pipeline finished |
| `entries_removed` | `string[]` | Remove entries after delete, clear, prune, or rollback |
| `runtime_status_changed` | `RuntimeStatus` | Clipboard capture/runtime status changed |

- Keep event payloads stable unless the change is intentional and all consumers are updated together.
- Clipboard watcher failures must update runtime status, not just logs.

## 8. Settings Rules
- `get_settings` / `save_settings` are the only source of truth for autostart state.
- Frontend must not talk to the autostart plugin directly.
- `save_settings` must fail if hotkey re-registration fails.
- `save_settings` must fail if autostart synchronization fails.
- Do not silently log-and-continue for those cases.
- Preserve the explicit "follow system language" option in settings UX.

## 9. I18n and Text
- Frontend-visible text, tray labels, and backend error strings shown to the frontend must use i18n.
- Backend logs must not depend on i18n.
- Treat repo files as UTF-8 unless proven otherwise.
- When reading Chinese text in the terminal, use UTF-8-safe reads such as `Get-Content -Encoding utf8`.
- If terminal output looks garbled, re-read safely before claiming the file is corrupted.

## 10. Security and Config
- Keep CSP defined.
- Keep `assetProtocol.scope` restricted. Do not widen it to `["**"]`.

## 11. What Good Changes Look Like
- Reuse the existing store / service / event architecture.
- Add the rule in the layer that owns it.
- Keep backend behavior deterministic and explicit.
- Preserve user-visible consistency across list, search, date filtering, prune, and runtime error states.
- Prefer durable rules over patch-specific hacks.

## 12. Before Finishing a Change
Sanity-check these when relevant:
- Did business logic stay in Rust services instead of leaking into commands or Vue components?
- Did new IPC stay inside `clipboardApi.ts`?
- Did you preserve cursor pagination and TTL semantics?
- Did delete/clear/prune keep DB-first, file-cleanup-second ordering?
- Did frontend-visible errors go through the shared UX path?
- Did background failures avoid noisy blocking UX?
- Did tray/i18n/runtime-status behavior stay consistent?
