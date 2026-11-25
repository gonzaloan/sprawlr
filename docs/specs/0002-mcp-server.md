# Ask sprawlr from an agent (MCP server)

Status: Built · Owner: gonzaloan · Date: 2026-11-24

## Problem

Someone asking an agent "why is checkout failing?" or "what breaks if orders-db goes down?" gets an answer built from raw AWS CLI calls. The agent lists resources one API at a time and rebuilds relationships it often gets wrong: env vars, IAM, event sources, notifications. Every question costs dozens of calls and minutes, and it needs live credentials that can usually write. sprawlr has already worked all of this out, with evidence for each connection.

## Outcome

- An agent answers architecture questions in one tool call each, from the last scan, citing how each connection is known.
- No credentials, no AWS calls, nothing that can change the account.

## Scope

**In:** a stdio MCP server (`sprawlr-mcp`) with ten read-only tools over the latest snapshot:
- the desktop app's last scan (default);
- a snapshot file (`--snapshot`);
- the demo (`--demo`).

The file is read again when it changes.

**Out:**
- **Live AWS calls.** The point is a safe, fast, consistent view. For a live value, the agent can use the AWS CLI alongside.
- **Writing anything.**
- **MCP resources and prompts.** No client needs them yet.
- **HTTP transport.** Local stdio covers Claude Code and Claude Desktop.

## Experience

```
claude mcp add sprawlr -- sprawlr-mcp
> why is checkout failing?
  → why_failing → "Errors start at InvoiceProcessor … Most common in its logs: “Timeout calling
    https://api.stripe.com/v1/invoices after <n> ms” (19 of 26)"
```

Names are matched by id, name, displayed name or part of it. "checkout" resolves to the production flow when only one production flow matches. An unknown or ambiguous name is a tool error that lists the candidates or points to `find_resources`.

## Design

- **App layer:** `sprawlr_app::queries` holds pure functions over a `Snapshot` that return serialisable answers, tested on the demo. Blast radius separates what breaks through configured or observed connections from what IAM alone would allow.
- **Driver:** `crates/sprawlr-mcp` contains:
  - `source` (snapshot file, reload on change, or demo);
  - `tools` (schemas and dispatch);
  - `protocol` (JSON-RPC 2.0 per line: initialize with version negotiation, ping, tools/list, tools/call).
- **Tool results:** text (pretty JSON) plus `structuredContent`. Mistakes are `isError` results, not protocol errors.
- **The protocol is written by hand** rather than with an SDK. Four methods over stdio is about a hundred lines with tests, and it keeps the driver free of an async runtime and of a fast-moving dependency.
- **Privacy:** answers come from the snapshot, which by ADR 0003 holds no secret values or raw log lines.

## Acceptance criteria

- [x] `initialize` returns the client's protocol version when supported, else the newest; notifications get no reply.
- [x] `tools/list` returns ten tools, each read-only with an object input schema.
- [x] `account_status` on the demo reads `act_now`; `blast_radius` of checkout-orders-db includes checkout-api.
- [x] An unknown resource or a missing argument is a tool error with a next step. A missing scan says how to make one.
- [x] Malformed JSON gets -32700; unknown methods get -32601; a request without a method or an empty batch gets -32600; an unknown tool gets -32602.
- [x] A batch (2025-03-26) gets one reply per request, and none if it holds only notifications.
- [x] An unknown app in `why_failing` or `recommendations` is a tool error listing the apps, never an empty "nothing failing".
- [x] Every tool returns an object as `structuredContent` on the demo.

## As built

- Connections and flow steps carry their evidence sources (`env:ORDERS_QUEUE_URL`, `xray`, `iam:role/…`), so an agent can cite how it knows.
- Error patterns come with when the logs were read (`read_at`): background refreshes skip logs.
- Recommendations put flow insights and resource findings on one scale and sort by it.
- Blast radius uses `ArchGraph::dependents` in the domain, which separates confirmed dependents from IAM-only ones.
- Resource types without an extractor now keep only safe scalar properties (`sprawlr_aws::cloudcontrol::safe_props`), so nothing raw can reach an agent or a snapshot.
