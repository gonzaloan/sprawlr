# Architecture

sprawlr is built as a hexagon: a pure domain in the middle, use cases around it, and adapters at the edges. AWS is one adapter among several; the demo account and the test doubles are others.

```mermaid
flowchart LR
  subgraph drivers["Drivers (how a person asks)"]
    cli["sprawlr-cli"]
    desk["sprawlr-desktop (Tauri)"]
    mcp["sprawlr-mcp (planned)"]
  end
  subgraph core["Core"]
    app["sprawlr-app\nuse cases + ports"]
    dom["sprawlr-domain\ngraph, inference, findings,\nflows, capacity, cleanup, IAM, logs"]
  end
  subgraph adapters["Driven adapters (where data comes from or goes)"]
    aws["sprawlr-aws\nInfraSource, HealthSource, ErrorLogSource"]
    fs["sprawlr-fs\nSnapshotStore, docs writer"]
    demo["app::demo\nin-memory demo account"]
    docs["sprawlr-docs\nMarkdown + Mermaid"]
  end
  cli --> app
  desk --> app
  mcp --> app
  app --> dom
  aws -. implements .-> app
  fs -. implements .-> app
  demo -. implements .-> app
  docs --> dom
  cli --> docs
  desk --> docs
```

## Dependency rule

Arrows point inwards. Each crate may depend only on what is listed:

| Crate | Layer | May depend on | Never |
|---|---|---|---|
| `sprawlr-domain` | domain | `serde`, `petgraph`, `chrono`, `regex` | I/O, async, AWS SDKs, any other sprawlr crate |
| `sprawlr-app` | application | `sprawlr-domain`, `anyhow` | AWS SDKs, file system, Tauri |
| `sprawlr-docs` | adapter (presentation) | `sprawlr-domain` | I/O |
| `sprawlr-aws` | adapter (driven) | `sprawlr-app`, `sprawlr-domain`, AWS SDKs | `sprawlr-fs`, drivers |
| `sprawlr-fs` | adapter (driven) | `sprawlr-app`, `sprawlr-docs`, `sprawlr-domain` | AWS SDKs |
| `sprawlr-cli`, `app/src-tauri` | drivers | everything above | business rules (they wire, parse and print) |

Cargo enforces most of this: a crate cannot use what it does not list. Reviews enforce the rest (no rules in drivers, no I/O in the domain).

## Ports

Defined in `crates/sprawlr-app/src/ports.rs`.

| Port | Meaning | Adapters |
|---|---|---|
| `InfraSource` | The structure of an account: resources, containment, declared connections | `sprawlr_aws::Aws`, `sprawlr_app::demo::Demo` |
| `HealthSource` | Alarms, metrics, traces for resources already in the graph | `Aws`, `Demo` (no-op) |
| `ErrorLogSource` | Error lines from logs, kept as masked patterns | `Aws`, `Demo` (no-op) |
| `SnapshotStore` | The latest snapshot between runs | `sprawlr_fs::FileSnapshotStore`, in-memory store in tests |

## Use cases

`crates/sprawlr-app/src/usecases.rs`:

- `scan_account`: discover, then add IAM reach (`domain::iam`). Read health and error logs if asked. Infer apps and environments, analyse, and save.
- `refresh_health`: same, from a snapshot, without rediscovering the structure.

Documentation is rendered by `sprawlr_docs::render(&Snapshot)` and written by `sprawlr_fs::write_docs`.

## Inside the domain

| Module | Does |
|---|---|
| `model`, `graph` | Nodes, typed edges with evidence (`Permitted < Declared < Observed`), containment, the `Snapshot` |
| `infer` | Apps and environments from tags, stacks, naming, containment and connections; each answer says how it was found |
| `analysis` | Findings (security, reliability, drift between environments) |
| `insights::{flows, capacity, rules, lifecycle, signals}` | Flows from entry points, headroom and what-if, recommendations, cleanup |
| `iam` | Policy evaluation: what a role allows, as `Permitted` edges; broad grants as findings |
| `logs` | Error lines reduced to masked templates |
| `fixtures` (feature) | The fictional demo account used by tests and the demo adapter |

## The snapshot contract

`Snapshot` (Rust, `sprawlr-domain/src/graph.rs`) is the only thing the UI receives. Its TypeScript mirror is `app/src/domain/types.ts`. A field added in Rust must be added there in the same change, with `#[serde(default)]` on the Rust side so older snapshots still load.

## Frontend

```
app/src/
  domain/      pure view logic over a snapshot (no React, no I/O), unit tested with vitest
  platform/    the Backend port and its adapters: tauri.ts (desktop), web.ts (browser demo)
  features/    one folder per feature: map, flows, details, briefing, cleanup, docs, navigation, scan
  ui/          small shared components (Spark)
  app/         the shell that composes features
```

Features read data through `domain` and reach the outside only through `useBackend()`. Nothing outside `platform/` imports `@tauri-apps/*`.

## Data flow of a scan

```mermaid
sequenceDiagram
  participant UI as Desktop UI
  participant T as Tauri command
  participant U as scan_account
  participant A as Aws adapter
  participant D as Domain
  participant S as FileSnapshotStore
  UI->>T: scan(profile, regions, options)
  T->>U: scan_account(Aws, options, store)
  U->>A: discover()
  A-->>U: graph + warnings
  U->>D: iam::apply
  U->>A: read_health(graph), read_errors(graph)
  U->>D: infer, snapshot (findings + insights)
  U->>S: save(snapshot)
  U-->>T: ScanResult
  T-->>UI: snapshot + warnings
```
