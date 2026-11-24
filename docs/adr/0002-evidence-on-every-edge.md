# 0002. Every connection carries its evidence

Date: 2026-11-24 · Status: Accepted

## Context

A map that is confidently wrong is worse than no map. Connections can be known three ways:
- an IAM policy allows them;
- configuration declares them, such as an environment variable, a trigger or an integration;
- traffic shows them, through X-Ray or flow logs.

These are not equally true.

## Decision

- Each edge holds a list of `Evidence { kind: Permitted | Declared | Observed, source, detail }`, and its strength is the strongest kind it has.
- Each inferred app or environment holds its method and confidence.
- The UI and docs always show which one applies.
- Flows follow only `Declared` or `Observed` edges, except for resources known only through IAM, where those edges are marked as inferred.

## Consequences

- Users can judge each line, and permission-only edges double as findings ("permission not seen in use").
- Every new extractor must say what its evidence is.
