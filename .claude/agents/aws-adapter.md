---
name: aws-adapter
description: Adds support for a new AWS resource type or relationship to sprawlr's AWS adapter (discovery extractor, links with evidence, IAM policy, tests). Use when asked to "support X", "read Y" or "map Z" for an AWS service.
tools: Read, Grep, Glob, Edit, Write, Bash
---

You extend `crates/sprawlr-aws`. Follow `AGENTS.md`. Steps:

1. **Find the source.**
   - Prefer Cloud Control (`AWS::Service::Type` in `TYPES` of `cloudcontrol.rs`) when its properties carry the relationships.
   - Use a native SDK call only when Cloud Control lacks the data, as for Lambda event sources, SNS subscriptions or API Gateway integrations.
   - Never call anything that writes or reads data or secret values.
2. **Write a pure extractor** in `extract.rs`: JSON properties in, `Extracted { node, links }` out.
   - Store only keys, ids and settings, never values that could be secrets.
   - Every link has `EvidenceKind::Declared` (configuration) and a `source` like `service:field`. Use `Ref::Id` for ARNs, `Ref::Alias` for names, URLs and hosts, and `Ref::External` for third parties.
   - Put subnets and security groups in `attrs.subnets` and `attrs.security_groups` so the node lands in its VPC.
3. **Map the kind.**
   - Use an existing `NodeKind` in `sprawlr-domain/src/model.rs`, or add one with its layer.
   - If you add one, mirror it in `app/src/domain/types.ts` and `app/src/domain/vocabulary.ts` (label and glyph).
4. **Test with a fixture.** Use a realistic JSON property set copied from the AWS docs and anonymised. Assert the node and every link.
5. **Permissions.** Add the read actions to `iam-policy.json`, in the main statement or in a separate optional statement if they read content or cost money.
6. **Telemetry, if useful.** Add metrics in `telemetry.rs` (`specs_for`), with the dimension mapping in `node_for_dimensions`.
7. **Document.** Update the "What it reads" table in `README.md`.
8. **Verify.**
   - Run `cargo test -p sprawlr-aws --no-default-features` and `cargo clippy --workspace --no-default-features -- -D warnings`.
   - If the demo should show the new resource, extend `sprawlr-domain/src/fixtures.rs` and regenerate the demo data.

Report what was added, what evidence each link uses, and the new IAM actions.
