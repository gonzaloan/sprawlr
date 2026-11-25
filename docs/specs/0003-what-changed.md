# What changed since the last scan

Status: Built · Owner: gonzaloan · Date: 2026-11-25

## Problem

A scan says how the account is, not how it got there. Someone opening sprawlr on Monday can't tell what changed since Friday: a new queue, a function whose timeout went from 5 s to 60 s, a security group someone deleted, a database that started failing. Those are usually the cause of what needs attention now. Today they compare two snapshots by eye, or not at all.

## Outcome

Every scan and refresh says what changed since the previous scan of the same account, in the same words everywhere: the briefing, the tray popover, the generated docs, the MCP server and the CLI.

## Scope

**In:**
- Resources added and removed.
- Watched settings: capacity, runtime, durability, exposure and environment variable names.
- Runtime connections known from configuration or traffic.
- Findings and attention items opened and resolved.
- Health changes and new error messages.
- The level change and the cleanup cost before and after.
- A history of the last 30 full scans in the desktop app.

**Out:**
- **Trends and charts over many scans.** One comparison answers "what happened", and history makes trends possible later.
- **Who made a change.** That is CloudTrail, which sprawlr does not read.
- **IAM-only reach.** Nobody changes that on purpose; it moves with policies.

## Design

- **Domain:** `changes::compare(before, after)` is pure. It returns `None` across accounts, ignores placeholders, and gives settings their human labels. It is stored as `Snapshot.changes`, with its `summary` line (ADR 0004).
- **App:** `SnapshotStore` gets `record_scan`, `scans` and `scan`, with default no-op implementations, so stores without history keep working.
  - A scan compares with the newest kept scan.
  - A refresh compares with the scan before the one it refreshes, so a 15-minute background refresh does not wipe the story.
  - Only full scans are recorded.
- **FS:** `FileSnapshotStore` keeps `history/scan-<time>.json` and prunes to 30.
- **Demo:** `sprawlr_app::demo::earlier()` builds the same account two days earlier: no reports bucket, a 5 s InvoiceProcessor timeout, a healthy connection pool and an old security group. The demo therefore tells a consistent story: raising the timeout to 60 s is what made the queue able to deliver twice.
- **Surfaces:**
  - `features/changes/ChangesCard` at the top of the briefing (rows about existing resources open them);
  - one line in the tray popover;
  - a section in the docs README;
  - the MCP tool `what_changed`;
  - `sprawlr changes`.

## As built (after review)

- **Only what both reads could see is compared.** That means resources in regions both scanned, and health and telemetry-based items only when both reads measured them. Error messages are compared only where logs were read both times. Connections count only as configured: traffic-only connections and third parties seen in traces come and go with traffic.
- **Matched by id.** Connections are matched by (resource id, kind, resource id) and findings by (resource, rule), so copies with the same name in two environments stay apart. A placeholder that later gets scanned counts as new, not changed.
- **Baselines.** A scan is compared with the newest kept scan of the same account. A refresh is compared with the scan its snapshot was already compared with, if any, so refreshing an older file never tells the story backwards.
- **History** is kept per account (`history/<account>/scan-<time with nanoseconds>.json`), 30 scans each. A history error is a warning, never a failed scan.
- **One list.** `Changes.rows` holds the rows in display order; the app, the tray, the docs and the CLI all print that one list.

## Acceptance criteria

- [x] The same snapshot twice changes nothing ("Nothing changed").
- [x] A new queue, a removed function, a resized function, an alarm and a new finding are each reported once, with labels.
- [x] Snapshots of different accounts are not compared.
- [x] The first scan has no changes; the second is compared with the first; a refresh after it is still compared with the first.
- [x] History keeps 30 scans per account, in order, and gives back exact times.
- [x] Telemetry off, logs read for the first time, or another region added: none of these is reported as a change.
- [x] Removing one of two same-named resources reports it once, with no stray connection changes.
- [x] Another account scanned in between does not hide changes; refreshing an older file does not invent any.
- [x] The demo reads "1 new resource, 1 removed, 1 setting changed, 1 new thing to look at, 1 new error message".
