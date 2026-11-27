# Open a resource and know what it is, who uses it and what it may do

Status: Built · Owner: gonzaloan · Date: 2026-11-27

## Problem

Someone opens an IAM role from search to find out what it is for. The panel shows its policy statements as raw text, and nothing about who can take the role on: the trust policy is never read. For other resources the configuration is a JSON dump behind a button, and nothing says which roles IAM lets into a table or a bucket. Answering "who uses this, what can it do, how is it set up" means going back to the AWS console, which is the job sprawlr is meant to save.

## Outcome

Opening any resource answers, in this order and without leaving the panel:
- what it is and what this kind of resource does;
- what needs attention on it;
- which flows it is part of, what uses it and what it depends on;
- what it is allowed to do, or who is allowed in (IAM);
- how it behaved in the last 24 hours;
- how it is set up, in words, and what changed since the last scan.

For an IAM role, the panel names who can take it on (services, accounts, identity providers) and who does, and what it allows by service, with the resources in the scan it reaches.

## Scope

**In:**
- The role's trust policy, from the same IAM call the scan already makes (`GetAccountAuthorizationDetails` returns it). Its principals are kept; its condition values are not, only their keys (an `sts:ExternalId` value is a secret).
- What a role allows, by service: access level, actions, and the resources of this scan they cover. Computed once in the domain from the policies already read, and kept on the role.
- Who IAM lets into a resource: the roles whose policies reach it, by name or because they reach every resource of that service.
- A compute resource's panel shows the role it runs as and what that role allows.
- Readable configuration for every kind sprawlr extracts, with the JSON still one click away.
- The changes of the last comparison that touch this resource.

**Out (and why):**
- AWS managed policy documents: thousands of them, and their names already say what they are. The panel names the common ones in plain words.
- Evaluating conditions, permission boundaries and SCPs: sprawlr reports them, it does not simulate IAM. The panel says when a grant has conditions.
- Users and groups: sprawlr maps workloads, not people.
- CloudTrail history of the resource: phase 4 (architecture view and its overlays).

## Experience

The panel keeps its place on the right. Sections appear only when they have something to say.

1. **Header:** name, kind, and one line on what that kind does ("Gives permissions to whatever takes it on").
2. **Needs attention:** findings and "probably not needed", as today.
3. **Facts:** app, environment, where it runs, what it runs. "Show what breaks if this fails".
4. **Part of:** the flows through it.
5. **Used by / Depends on:** connections with their evidence, as today. For a role, "Who can take it on" lists the trust policy in words ("Lambda functions", "account 444455556666", "GitHub Actions, when its conditions hold"), and "Taken on by" the resources of this scan that do. A role nothing in the scan takes on says so, and how long ago it was last used.
6. **Permissions:**
   - role, under "What it may do": one row per service ("DynamoDB: read and write, on orders and invoices", "every SQS queue", "when its conditions hold"), then denies, then the AWS managed policies no row counts, with a plain description when known;
   - compute, under "Permissions": "Runs as checkout-api-task-role", with the same rows;
   - anything IAM can reach: "Allowed in by IAM", one row per role with its access level, and "every table" when the grant is service-wide. A deny of every action takes the resource out.
7. **Headroom, Last 24 hours, Errors in logs:** as today. They come before the configuration: they are what someone checks first.
8. **Configuration:** labelled settings ("Point-in-time recovery: On"). "Show as JSON" for the attributes as read.
9. **Since the last scan:** the rows of the comparison about this resource.
10. **Tags, Identifier.**

Empty states: a role with no policies says "No policies attached". A scan from before this change has no trust policy and no summary of the policies; the sections say to scan again.

## Design

- **Domain (Rust, `sprawlr-domain/src/iam.rs`):**
  - `trusted_by(doc) -> Vec<Principal>`: kind (service, account, role, user, federated, deleted, anyone), id, and condition keys. Users, federated users and sessions keep only their account or role: their names can name people.
  - `access_of(graph, policies, aws_managed) -> Vec<ServiceAccess>`: per service, effect, conditions and resources, its access levels (full, read, write, permissions, read from the action's name), actions as written, the graph nodes it reaches (service-wide grants included, minus full denies and `NotAction`/`NotResource` exclusions), whether it covers every resource, and patterns outside the scan. A resource pattern counts only for the service it names. Reuses the existing glob and resource forms.
  - `apply` also stores `attrs.access` on every role it can read. Same input, same output.
- **Adapter (`sprawlr-aws/src/iam.rs`):** reads `AssumeRolePolicyDocument`, the permissions boundary's name and the region of last use from the response it already gets. No new call and no new permission.
- **Snapshot contract:** new role attributes `trust`, `access`, `permissions_boundary`, `last_used_region`. All optional: older snapshots still open.
- **Frontend (`app/src/domain/resource.ts`):** pure functions for the panel (`trustOf`, `accessOf`, `allowedIn`, `runsAs`, `settingsOf`, `changesAbout`, `purposeOf`), each with vitest cases. `features/details` splits into one component per section.
- **Costs, permissions, privacy:** no new AWS call, no new cost, no new permission. Trust policy condition values are never stored.

## Acceptance criteria

- [x] Given a role trusted by `lambda.amazonaws.com`, when it is opened, then "Who can take it on" says "Lambda functions" and "Taken on by" lists the functions of this scan that use it.
- [x] Given a trust policy with an `sts:ExternalId` condition, when it is scanned, then the snapshot holds the key `sts:ExternalId` and not its value.
- [x] Given a role allowed `dynamodb:GetItem` and `dynamodb:PutItem` on one table, when it is opened, then it shows "DynamoDB: read and write" on that table, as a link.
- [x] Given a role allowed `dynamodb:*` on `*`, when a table is opened, then "Allowed in by IAM" lists that role with "every table".
- [x] Given a Lambda function, when it is opened, then the panel names the role it runs as and what that role allows.
- [x] Given a DynamoDB table, when it is opened, then its settings read as labelled values, and the JSON is one click away.
- [x] Given a snapshot without `trust` (scanned before this change), when a role is opened, then the panel says to scan again, and nothing breaks.
- [x] Given the demo account, when its roles are opened, then each shows who can take it on and what it allows.

## Open questions

- Whether the MCP server's `describe` should answer "what can this role do" with the same rows. Its settings already carry `access` and `trust`; a dedicated answer can follow.
