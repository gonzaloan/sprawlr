# 0001. Hexagonal architecture with a pure domain

Date: 2026-11-24 · Status: Accepted

## Context

The value of sprawlr is in its rules: how apps are inferred, how flows are found, what counts as a bottleneck or a leftover. Where the data comes from is secondary, and it already has three sources: a real AWS account, the demo account and test fixtures. More are planned, such as an MCP server for agents, other clouds and stored history. When the scanner, the rules and the drivers were mixed, every new source meant touching the rules.

## Decision

- `sprawlr-domain` holds the model and every rule, with no I/O.
- `sprawlr-app` declares ports (`InfraSource`, `HealthSource`, `ErrorLogSource`, `SnapshotStore`) and the use cases that orchestrate them.
- Adapters implement ports: `sprawlr-aws`, `sprawlr-fs`, and the demo inside `sprawlr-app`.
- Drivers (CLI, desktop) only wire adapters to use cases.
- The frontend mirrors this:
  - `domain/` holds pure view logic;
  - `platform/` holds the Backend port and its adapters;
  - `features/` holds the UI.

## Consequences

- Rules are tested without AWS or a UI. The demo runs the same use case as a real scan.
- A new source is a new adapter, with no changes to rules.
- There are more crates and some indirection; ports use `impl Future` in traits, so adapters are generic rather than trait objects.
- Drivers can't hold business logic; reviews check this (see `.claude/agents/architecture-reviewer.md`).
