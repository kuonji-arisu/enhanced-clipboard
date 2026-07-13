# Enhanced Clipboard — Agent Guide

This is the canonical instruction source for coding agents in this repository. Tool-specific
files may import or link here, but must not copy these rules. Keep this guide limited to stable,
verifiable constraints; put rationale in ADRs and temporary details in tasks or pull requests.
Before editing, inspect the current code, tests, and diff, preserve unrelated user changes, and
prefer the smallest coherent change.

## Project And Decisions

- Enhanced Clipboard is a Windows-only, local-first desktop application built with Tauri 2,
  Rust, Vue 3, Pinia, and TypeScript. Do not add cloud or cross-platform behavior unless asked.
- [ADR 0001](docs/adr/0001-single-writer-clipboard-engine.md) records the clipboard architecture
  and accepted trade-offs. If a task changes an accepted decision, explain the conflict and update
  or supersede the ADR instead of silently adding a parallel path.

## System Boundaries

- Clipboard data follows UI -> `clipboardViewStore` -> `src/composables/clipboardApi.ts` -> thin
  Tauri command -> `ClipboardEngine`. The engine sequences domain work, `repository` owns
  SQL/transactions, `artifacts` owns managed image files, and `read_model` owns read-only projection,
  snippets, and highlights. No layer bypasses the engine to mutate clipboard state.
- `ClipboardEngine` is the only mutable owner of `clipboard.db`, committed clipboard artifacts,
  deduplication, retention policy, and revision. The Windows listener and engine are the only
  long-lived clipboard business threads; do not add custom workers, schedulers, or convergence
  loops.
- The listener samples source application and clipboard and reports capture availability through
  the runtime service patch path. It sends non-empty text without probing images; only absent or
  empty text falls through to owned RGBA. Limits, image policy, deduplication, and copy suppression
  belong to the engine. A full capture mailbox may drop the new payload and log it in English.
- Clipboard commands and clipboard-related settings effects use the bounded engine mailbox. An
  async handler may await the result, but its blocking mailbox wait runs through
  `tauri::async_runtime::spawn_blocking`; synchronous image encoding stays on the engine thread and
  neither encoding nor blocking `recv` runs on the IPC/UI path.
- Components, stores, and hooks do not call Tauri `invoke()` or `listen()` directly; use focused
  `src/composables/*Api.ts` wrappers. Keep commands thin, return `Result<T, String>`, avoid
  `unwrap`/`expect`/`panic!` on reachable runtime paths, and apply OS settings effects in the
  backend rather than through frontend plugins.
- Keep read-only `AppInfo`, live `RuntimeStatus`, saved `AppSettings`, and `PersistedState`
  separate. Runtime changes use the shared runtime patch path; `effectiveTheme` combines saved
  intent with the runtime system-theme fact. Shared defaults and limits remain Rust/`AppInfo`-owned.

## Clipboard And Persistence

- `clipboard.db` uses SQLCipher-backed `rusqlite`; its raw key stays in Windows Credential Manager
  and never enters logs or IPC. `settings.db` remains plain and separate.
- A destructive clipboard schema rebuild requires an explicit product decision, an updated or new
  ADR, and an upgrade/release notice. Otherwise, never delete an existing database for a lock,
  permission error, or transient I/O failure. Automatic recovery is limited to a newly created key
  with a confirmed unrecoverable decrypt/not-a-database failure. A clipboard reset never clears
  settings or persisted UI/window state.
- Store timestamps as Unix epoch seconds (`i64`) and paginate on `(created_at DESC, id DESC)`
  without `OFFSET`. Filters are strict. The first page contains matching pinned entries plus a full
  non-pinned page; pinned entries do not consume page size. `pinned_count` is global. TTL and
  `max_history` affect only non-pinned entries, with TTL applied before history trimming.
- Images enter the database only after original and preview are complete. Do not add pending/failed
  states, durable ingest jobs, staging, multi-worker scheduling, or runtime sweepers. Originals
  live under `images/`, previews under `thumbnails/`, and stored paths resolve only to validated,
  direct relative files inside those roots—never absolute, traversing, symlink/junction escapes,
  or frontend-visible original filesystem paths.
- A database transaction is the mutation success boundary. Insert, tags, and retention commit
  together and protect the new id from same-second trim; unpin and its pruning also commit together.
  Deletion is DB-first, then best-effort cleanup of only paths collected from rows—never a root
  wipe. Failed inserts remove new files; post-commit cleanup/event failures do not roll back.
  Outside schema rebuild, startup artifact scanning removes only temporary and unreferenced files.

## Frontend, Events, And Settings

- Rust owns clipboard access, validation, canonicalization, query membership, projection,
  retention, and repair. The frontend renders returned read models; it does not reconstruct
  membership, snippets, or highlights, or infer entry tags.
- Clipboard UI state lives in one `clipboardViewStore`, with one `items` array and one committed
  query. Preserve refresh/load-more serialization and generation/revision guards so stale responses
  cannot replace or append to the current view. Do not recreate parallel clipboard caches.
- `revision` is process-local and advances only when a mutation or visibility-policy change can
  alter the authoritative projection; reads and no-ops do not advance it.
  `clipboard_changed { revision }` is only an invalidation hint and may be lost, duplicated, or
  reordered. A fresh query is authoritative; actions that may mutate clipboard state actively
  invalidate, and resume forces refresh.
- `globalNow` may hide items at `visible_until` locally without mutating the cached array. TTL
  pruning and visibility semantics remain backend-owned.
- Change Rust and TypeScript clipboard wire types together. For list-page or changed-event shapes,
  also update `tests/contracts/clipboard_page.json` and both contract tests. Do not add a
  compatibility adapter unless explicitly required.
- Settings saves remain patch-based and metadata-driven. Preserve `persist_only`,
  `persist_then_apply`, and `apply_then_persist` ordering and return final DB-backed state with
  grouped effects. A failed persist-then-apply effect keeps the saved value and returns a warning;
  clipboard policy effects go through the engine.

## Quality, Review, And Delivery

- New or changed frontend-visible text, tray text, and backend errors shown to users use the shared
  locale JSON files and named placeholders. Backend logs stay in English; repository text is UTF-8.
- Keep CSP enabled and `assetProtocol.scope` restricted; never widen it to `["**"]`.
- Tests belong under `src/tests/frontend/` and `src-tauri/tests/` and exercise public store/engine
  behavior. Never touch real application data without explicit user authorization; tests and smoke
  work default to isolated temporary data.
- A review finding names the violated behavior or invariant, provides a reachable reproduction or
  failing test, and states the concrete consequence. Treat hypothetical future races, optional
  hardening, and aesthetic alternatives as non-blocking backlog, not reasons to add layers.
- Run focused checks while iterating and every current CI check before handoff. `package.json` and
  `.github/workflows/ci.yml` are authoritative; the current full suite is:

  ```text
  pnpm test:frontend
  pnpm build
  cargo test --manifest-path src-tauri/Cargo.toml
  cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
  cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
  ```

- Develop changes on a dedicated branch. Commit only after user confirmation, use English
  Conventional Commits, then open a pull request targeting `master` and leave it open unless asked.
