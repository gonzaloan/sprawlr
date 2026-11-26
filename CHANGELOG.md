# Changelog

Notable changes to sprawlr. Versions follow [Semantic Versioning](https://semver.org/).

## Unreleased

- The demo account's map, briefing and tray always show a recent read, instead of aging with the date the demo was generated.
- A tour GIF, a "why not the AWS console or the AWS CLI" section, SECURITY.md and the license file.
- `iam-policy.json` also denies batch and history reads of secrets, parameters, object versions and table items.

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
