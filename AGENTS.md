# Working on sprawlr

Instructions for anyone changing this repo, people and coding agents alike. Read this first, then `docs/architecture.md`.

## What sprawlr is

A local, read-only desktop app (and CLI) that scans an AWS account and explains it:
- which apps exist and how their parts talk;
- the flows requests follow;
- what breaks first if traffic grows;
- what fails, and why;
- what is probably not needed.

Every statement it makes carries the evidence behind it.

## Non-negotiables

1. **Read-only.** Never call an AWS API that changes anything. New calls go in `iam-policy.json`, under a `Describe/List/Get` action or a separate optional statement.
2. **Never store values that could be secrets or personal data.**
   - Keep environment variable *keys*, never values.
   - Never read secret values, objects, items or messages.
   - Logs become masked templates (`sprawlr_domain::logs`), never raw lines.
3. **Say how you know.** Every edge has evidence (`Permitted`, `Declared`, `Observed`), and every inferred app or environment has a method. Never draw a line without evidence, and never show IAM-only reach as configured.
4. **Costly reads are opt-in.**
   - Telemetry can be turned off.
   - Error logs are off by default.
   - The UI says what each one costs.
5. **Deterministic core.** The same snapshot gives the same findings, flows and docs. There is no randomness, and the only clock reads are evidence timestamps.

## Architecture rules

Hexagonal; see `docs/architecture.md` for the diagram.

- `crates/sprawlr-domain` is pure: no I/O, no async, no AWS. Business rules live here and nowhere else.
- `crates/sprawlr-app` holds ports (traits) and use cases. It does not know AWS, Tauri or the file system.
- Adapters implement ports:
  - `sprawlr-aws` for AWS;
  - `sprawlr-fs` for files;
  - `sprawlr_app::demo` for the demo account.
  
  `sprawlr-docs` renders Markdown.
- Drivers (`sprawlr-cli`, `app/src-tauri`) parse input, pick adapters, call a use case and present the result. They contain no rules.
- Frontend:
  - `app/src/domain` is pure view logic.
  - `app/src/platform` is the only place that knows Tauri.
  - Features use `useBackend()`.

When in doubt where code goes, ask: "would this still be true for a GCP adapter or a test double?" If yes, it belongs in the domain or app layer.

## Commands

```bash
# Rust (use --no-default-features on machines with < 10 GB RAM: skips the EC2 SDK)
cargo test --workspace --no-default-features
cargo clippy --workspace --no-default-features -- -D warnings
cargo fmt --all

# Frontend
cd app && npm ci
npm run typecheck && npm test && npm run build

# Demo data (regenerate after any change to domain output)
cargo run -p sprawlr-cli --no-default-features -- demo --out app/src/demo/demo-snapshot.json
cargo run -p sprawlr-cli --no-default-features -- docs --snapshot app/src/demo/demo-snapshot.json \
  --out examples/demo-architecture --json app/src/demo/demo-docs.json

# Desktop app
cd app && npx tauri dev
```

## Recipes

**Support a new AWS resource type.**
1. Add the Cloud Control type to `TYPES` in `sprawlr-aws/src/cloudcontrol.rs`.
2. Write a pure extractor in `sprawlr-aws/src/extract.rs` with a fixture test.
3. Map its kind and add its read actions to `iam-policy.json`.
4. Add it to the "What it reads" table in the README.

**Add a finding or recommendation.**
1. Write the rule in `sprawlr-domain/src/analysis.rs` (per resource) or `insights/rules.rs` (needs flows or capacity).
2. Write the title as a statement, the detail as the evidence and the action as a verb.
3. Add a demo case to `fixtures.rs` and a test that fails without the rule.
4. Regenerate the demo data.

**Add a field to the snapshot.**
1. Add it in Rust with `#[serde(default)]`.
2. Mirror it in `app/src/domain/types.ts`.
3. Use it through a function in `app/src/domain`, with a vitest case.

**Add a UI feature.**
1. Create a folder in `app/src/features/<name>`.
2. Read data through `app/src/domain` and reach Rust only through `useBackend()`.
3. Use the existing CSS tokens in `styles.css`; never use raw colours.

**Add an outside capability** (a new cloud, an MCP server, a store).
1. Define or reuse a port in `sprawlr-app`.
2. Implement it in a new adapter crate.
3. Wire it in a driver.

## Code style

- Rust:
  - small functions with doc comments that say *why*;
  - no `unwrap()` outside tests and provably safe spots;
  - errors with `anyhow` and context in adapters;
  - domain functions return values, not `Result`, unless input can be invalid.
- TypeScript: strict mode, no `any`, pure functions in `domain/`, one component per file, hooks before render logic.
- Names describe what something is for a reader, not how it is built (`flowsThrough`, not `filterFlowsByNodeId`).
- Tests describe behaviour (`upload_flow_reaches_an_ec2_worker_known_only_through_iam`).

## Product writing

The UI and docs speak to someone running a system, not to an engineer reading the code.
- Use sentence case and plain verbs, with no filler or marketing words.
- Say what happened and what to do: "Errors start at InvoiceProcessor", then "Start with that message: it is 73% of the error lines."
- Name AWS things the way AWS does (SQS queue, Lambda function). Name evidence plainly: "Configured", "Seen in traffic", "Allowed by IAM".
- Empty and error states say how to move on.

## Definition of done

- [ ] Tests for the behaviour, failing without the change.
- [ ] `cargo test`, `clippy -D warnings`, `cargo fmt --check`, `npm run typecheck`, `npm test` and `npm run build` all pass.
- [ ] Demo data regenerated if domain output changed, and the screenshots in `docs/images/` updated if the UI changed visibly.
- [ ] README and `docs/` updated for anything a user or contributor would notice.
- [ ] An ADR in `docs/adr/` for any decision that changes a rule above.
- [ ] One topic per PR. The description says what changed for the person using sprawlr.

## Specs and decisions

- Feature specs live in `docs/specs/`, using the template there. Write the spec before building anything that adds a screen, a port or a cost.
- Architecture decisions live in `docs/adr/`, numbered.

## Agents

Focused agents live in `.claude/agents/`:
- `architecture-reviewer` checks a change against the rules above.
- `aws-adapter` adds AWS resource types.
- `insight-author` adds findings and recommendations.
- `ui-builder` builds frontend features.
- `spec-writer` drafts specs.

Use them for those jobs; they carry the checklists.
