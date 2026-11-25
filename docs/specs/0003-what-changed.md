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

## Acceptance criteria

- [x] The same snapshot twice changes nothing ("Nothing changed").
- [x] A new queue, a removed function, a resized function, an alarm and a new finding are each reported once, with labels.
- [x] Snapshots of different accounts are not compared.
- [x] The first scan has no changes; the second is compared with the first; a refresh after it is still compared with the first.
- [x] History keeps 30 scans in order and prunes older ones.
- [x] The demo reads "1 new resource, 1 removed, 1 setting changed, 1 new thing to look at, 1 new error message".
