# 0003. Never store values that could be secrets or personal data

Date: 2026-11-24 · Status: Accepted

## Context

sprawlr runs with read access to accounts that hold secrets, customer data and logs. Snapshots get saved, shared and committed.

## Decision

- Never read secret values, S3 objects, DynamoDB items or queue messages. The IAM policy denies these explicitly.
- Environment variable values are inspected in memory to find references and to flag plaintext secrets. Only keys and resolved references are stored.
- Log lines are reduced to templates that mask ids, numbers, emails, addresses, tokens and quoted values. Raw lines are never stored.

## Consequences

- Snapshots are safe to share.
- Some precision is lost: a masked message can merge two errors that differed only in a value.
