# 0004. The snapshot is the contract between core and UI

Date: 2026-11-24 · Status: Accepted

## Context

The UI runs in two places: the desktop app, backed by Rust, and a browser demo with no backend. Both need the same data, and older saved snapshots must keep opening.

## Decision

- `Snapshot` (nodes, edges, findings, insights) is computed once in Rust and serialised.
- The UI only reads it and derives views; it never recomputes rules.
- New fields are additive and use `#[serde(default)]`.
- `app/src/domain/types.ts` mirrors the Rust types and changes in the same pull request.

## Consequences

- The web demo needs only a JSON file, and old snapshots load in new versions.
- Rules exist once, in Rust. The cost is keeping the TypeScript mirror in sync by hand.
