# Architecture decisions

Short records of decisions that shape the code, in the format: context, decision, consequences. Add one for any change to a rule in [AGENTS.md](../../AGENTS.md). Never edit an accepted record; supersede it with a new one.

| # | Decision | Status |
|---|---|---|
| [0001](0001-hexagonal-architecture.md) | Hexagonal architecture with a pure domain | Accepted |
| [0002](0002-evidence-on-every-edge.md) | Every connection carries its evidence | Accepted |
| [0003](0003-never-store-values.md) | Never store values that could be secrets or personal data | Accepted |
| [0004](0004-snapshot-is-the-contract.md) | The snapshot is the contract between core and UI | Accepted |
| [0005](0005-costly-reads-are-opt-in.md) | Reads that cost money are visible and optional | Accepted |
| [0006](0006-exports-read-configuration-on-demand.md) | Exports read full configuration on demand and never store it | Accepted |

New records use [template.md](template.md).
