# Changelog

Notable changes to sprawlr. Versions follow [Semantic Versioning](https://semver.org/).

## Unreleased

- Export as code: the whole account or one app as a Terraform project (awscc provider) and CloudFormation templates, with the documentation, from each resource's live configuration. Resources refer to each other in the code, and every resource has an import, so `terraform plan` checks the code against the account: it should import everything and change nothing. Values that could be secrets become inputs you fill in, never written. Resources a CloudFormation stack already manages are left out with the stack's name. In the desktop app and with `sprawlr export`; it needs the new `OptionalExport` statement in `iam-policy.json`.
- An Architecture tab, next to Map and Documentation, draws any place the way an AWS architecture diagram does: the internet and edge services, each region, its VPCs by availability zone and subnet tier, and what runs outside any VPC. Each resource says how its zones are configured ("Multi-AZ", "Subnets in 2 AZs"), and each subnet tier says how it was read. Health and findings show on every resource, and the connections of the one you point at are drawn with their evidence. The resource panel names the availability zones a resource runs in.
- Opening a resource answers what it does, who uses it, what it may do and how it is set up. An IAM role shows who can take it on (from its trust policy, read in the same IAM call), what in the scan does, and what it may do by service, with the resources it reaches. Functions and services name the role they run as; tables, buckets and queues list the roles IAM lets in. Settings read as words, with the JSON one click away, and the panel shows what changed on the resource since the last scan. Scan again to see trust policies; older scans say so.
- Back and Forward buttons (⌘[ and ⌘] in the Mac app) and a trail from the whole account to what is open. In the web demo every place has a URL, so the browser's Back and links work too. A link to something the snapshot on screen doesn't have opens the whole account.
- The environment switch appears only when some app runs in more than one environment, instead of hiding resources in accounts that have none.
- "Load" is now "Capacity", and maps without load figures don't offer it. The "breaks first" card opens it on the whole account, in every environment.
- The demo account's map, briefing and tray always show a recent read, instead of aging with the date the demo was generated.
- A domain's extension is no longer read as an environment: a site at `example.dev` is not "dev", and neither is everything it calls.
- Resources named after an app that tags or a CloudFormation stack already name join it (`aws-quiz-guide-progress` joins quiz-guide). Names that start with a verb are grouped by what follows (`get-quotes-*`, not `get`).
- KMS keys are named by their alias, and AWS managed keys (`aws/s3`) say so and belong to no app. This needs `kms:ListAliases`, now in `iam-policy.json`; without it keys show their description or id, and the scan says what to add.
- When AWS limits how fast resources can be read (KMS keys, for example), a scan slows down and tries again instead of leaving them half-read. If it still can't, the warning says so in AWS's words and what to do, without the SDK's internals.
- A tour GIF, a "why not the AWS console or the AWS CLI" section, SECURITY.md and the license file.
- `iam-policy.json` also denies batch and history reads of secrets, parameters, object versions and table items.
- The install steps say which installer fits your system, and how to open it the first time while the builds are not signed with a developer certificate.

## 0.1.0 (2026-11-25)

The first release, with installers for macOS (Apple silicon and Intel), Windows and Linux.

- **Map:** apps grouped from tags, CloudFormation stacks, naming and connections. VPCs are drawn as territories, and environments work as a filter. Every connection carries its evidence: seen in traffic, configured, or only allowed by IAM.
- **Flows:** request and event paths, found from their entry points. Each flow shows its runs, errors and wait time, and where it breaks first.
- **What needs attention:** where errors start, what saturates first, queues that can lose messages, and shared dependencies.
- **Bottlenecks and what-if:** each resource's busiest hour against its limit, and a traffic slider.
- **Why it fails:** opt-in error patterns from CloudWatch Logs Insights, masked and never stored raw.
- **IAM mapping:** workers you can't see inside are mapped from their roles' policies.
- **Probably not needed:** idle, orphaned and forgotten resources, with what they cost a month.
- **What changed since the last scan:** a comparison with the previous scan of the same account.
- **Tray:** status from the menu bar or the system tray. Background refresh and notifications are off by default.
- **Documentation:** a Markdown page per app, with Mermaid diagrams.
- **CLI and MCP server:** the same answers for scripts and AI assistants. The MCP server has 11 tools, including `what_changed`.
- **Web demo:** a fictional account at <https://gonzaloan.github.io/sprawlr/>.
