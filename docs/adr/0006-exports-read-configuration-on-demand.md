# 0006. Exports read full configuration on demand and never store it

Date: 2026-11-27 · Status: Accepted

## Context

Code that recreates a resource needs its whole configuration: IAM policy documents with their condition values, state machine definitions, every setting. The snapshot keeps far less on purpose (ADR 0003). It keeps:
- the keys of environment variables, not their values;
- the condition keys of trust policies, not their values;
- only safe scalars of types without an extractor.

Snapshots get saved, kept in a history and shared. Export files are different: the person asks for them, picks where they go, and usually commits them to their own repository.

## Decision

- An export reads each resource's configuration again, at the moment it is asked for, through Cloud Control. The configuration is kept in memory and written only to the folder the person chose. It never enters the snapshot, its history, logs or warnings.
- Configuration documents (IAM policies with their conditions, key and bucket policies, state machine definitions) are written as AWS returns them: without them the code would not recreate the resource. A value in them that looks like personal data or is named like a secret is still left out.
- Values that could be secrets or personal data are never written, even in an export. They become inputs the person fills in: every value under environment variables (unless it names another resource in the export), values named like secrets (properties, `{ Key, Value }` pairs, header values), email addresses and phone numbers, EC2 user data, SSM parameter values, and properties AWS never returns.
- The deny on reading secrets, objects, items and messages in `iam-policy.json` stays. A resource whose read needs one of those actions is left out, with AWS's message.

## Consequences

- An exported folder can contain account details a snapshot does not: identifiers, policy conditions, network ranges. The app and the README say so before and after the export.
- Exports cost one read per resource and one per type. Cloud Control adds no charge; Secrets Manager and KMS bill their own reads by the request, a fraction of a cent for an account. They depend on credentials at the time of the export, not on the last scan.
- The person has inputs to fill in before the code can deploy. That is the price of never writing a secret.
