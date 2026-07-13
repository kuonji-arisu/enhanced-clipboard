# ADR 0001: Single-Writer Clipboard Engine

- Status: Accepted
- Date: 2026-07-11
- Target release: 0.4.0

## Context

Clipboard mutations previously crossed a DB mutex, a listener, an image worker, maintenance work, temporary cleanup threads, several event streams, and multiple frontend caches. Individual operations were guarded, but no component owned the complete ordering of database changes, artifacts, retention, deduplication, and notifications. Each additional defensive layer created new interleavings and made review less likely to converge.

The project is a personal, local Windows clipboard manager. It does not need crash-safe in-flight image ingest, exactly-once capture, multiple ingest workers, or a frontend event-replay system.

## Decision

### One mutable owner

`ClipboardEngine` owns the SQLCipher connection, committed image directories, clipboard deduplication, retention policy, and an in-memory revision. All clipboard reads and mutations reach it through a bounded mailbox. Tauri commands use request/reply messages; the Windows listener uses non-blocking capture messages.

Only two long-lived clipboard business threads are allowed: the listener and the engine. The listener samples the clipboard and source application, transfers owned text or RGBA data, and reports runtime availability. It does not read settings, access storage, perform deduplication, or encode images.

The engine executes each request to completion before the next request. Database transactions define mutation success. Removal commits database state before best-effort artifact deletion. Image capture writes original and preview files atomically before inserting a ready row and removes those files if the transaction fails.

### Ready-only images

An image becomes queryable only after both its original and preview have been encoded. There are no persisted pending or failed entries, durable ingest jobs, staging protocol, runner recovery, multi-worker scheduling, or runtime artifact maintenance.

At startup, before capture begins, the engine may remove temporary files and unreferenced files from `images/` and `thumbnails/`. It does not continuously sweep artifacts or proactively validate every referenced original.

Preview repair is an explicit synchronous request. If the original is valid, the engine rebuilds the preview and updates the row. If the original is missing or invalid, it removes the row DB-first. A preview write failure leaves the row unchanged and is exposed as a retryable UI error.

### Invalidation events

The only clipboard list event is `clipboard_changed { revision }`. It announces that cached query results may be stale; it does not describe or replay a mutation. Events may be lost, duplicated, delayed, or reordered. The current database query is always authoritative.

The frontend keeps one active query and one `items` array. It coalesces invalidations into a single-flight refresh and discards responses from old query generations or revisions. Commands also invalidate after successful mutations so correctness does not depend on event delivery. Resume forces a fresh query.

### Schema and upgrade

Clipboard schema v9 stores only ready text/image entries and their tags. Upgrading to v9 intentionally rebuilds the clipboard schema and clears all clipboard history, including pinned entries. The separate settings database, application settings, and persisted UI/window state are preserved. No compatibility facade, dual-write path, or clipboard migration framework is introduced.

## Accepted Risks And Consequences

- A process crash may lose the image currently being encoded.
- Capture uses a mailbox capacity of two; when it is full, a new background capture may be dropped and logged in English.
- Capture is not exactly-once. Duplicate suppression is best-effort and process-local.
- Synchronous image encoding can delay later engine requests; introducing another worker requires a demonstrated user-impacting problem and a new ADR.
- A crash between file creation and DB insertion can leave orphan files until the next startup cleanup.
- A crash after DB removal but before file deletion can also leave orphan files until startup cleanup.
- Mutations may reset pagination and scroll position because the frontend refreshes the authoritative first page.
- A lost event may leave the current view stale until a successful command invalidation, resume, or another event.
- Future file carriers, multiple workers, leases, outboxes, retries, and generic job registries are intentionally out of scope.

These risks are acceptable for a local personal clipboard utility and are less costly than maintaining distributed-state semantics inside one desktop process.

## Review Stop Rules

A finding blocks this refactor only when all three conditions hold:

1. It violates a product or data invariant recorded in this ADR or `AGENTS.md`.
2. It has a reachable reproduction or a failing behavior-focused test.
3. It has a clear P0/P1 user or data consequence.

Hypothetical races that require unsupported future architecture, possible refinements, and “more elegant” abstractions go to the backlog. Review is limited to two rounds; the second round verifies only behavior introduced or changed by this refactor. New concurrency infrastructure requires evidence of a current user-facing failure and a replacement ADR.
