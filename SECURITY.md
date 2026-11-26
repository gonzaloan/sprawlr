# Security

sprawlr runs on your machine with your AWS credentials. It only reads from AWS, and it keeps what it reads on your machine.

## What it reads

- **Configuration**, through list and describe calls.
  - Lambda functions and ECS task definitions come back with their environment variable values. sprawlr reads them in memory and keeps only the variable names.
  - [`iam-policy.json`](iam-policy.json) grants only these calls. It explicitly denies reading:
    - secret values (Secrets Manager and SSM parameters);
    - KMS decryption;
    - S3 object contents;
    - DynamoDB items;
    - SQS messages.

  The deny protects you only when you scan with that policy. If you scan with your usual admin profile, nothing stops those calls, although sprawlr never makes them.
- **Metrics.** sprawlr reads CloudWatch alarms and metrics (`GetMetricData`, which CloudWatch bills) and the X-Ray service graph. This is on by default.
  - In the app, turn it off with the "Include alarms, metrics and traces" checkbox. In the CLI, use `--no-telemetry`.
  - Background refresh, off by default, reads metrics again on every run.
- **Error logs.** These are off by default, and turning them on needs the separate `OptionalErrorLogs` statement in the policy. Delete that statement to forbid them.
  - The Logs Insights query returns up to 5,000 distinct error lines, each cut to 400 characters.
  - sprawlr masks numbers, ids, emails, addresses, tokens and quoted values, turning the lines into patterns in memory.
  - Only the patterns are stored, never the raw lines ([ADR 0003](docs/adr/0003-never-store-values.md)).

sprawlr never calls an API that changes your resources or their settings. With error logs on, it starts Logs Insights queries and stops any that run longer than 90 seconds.

## What it stores

- **The desktop app** keeps these files in its data folder:
  - the last snapshot;
  - the last 30 scans of each account;
  - your app settings.

  A snapshot holds:
  - resource names, configuration, tags and IAM policies;
  - connections;
  - metric figures;
  - error patterns.

  It holds no secret values, object contents, table items or queue messages. The app also writes exported docs to the folder you pick, and adds a login item if you turn on "Launch at login".
- **The CLI** writes `snapshot.json` or `architecture/` in the current folder unless you pass another path.
- **The MCP server** writes nothing.
- **Credentials** are read by the AWS SDK from your usual profile. sprawlr never stores them.

## What leaves your machine

- **To AWS:** the API calls of the scans you run. sprawlr collects no usage analytics and calls no other server.
- **To your AI assistant:** the MCP server answers from your snapshot. Whatever it tells your assistant may be sent to that assistant's model provider.

## Installers

The installers on [Releases](https://github.com/gonzaloan/sprawlr/releases) are built by [`release.yml`](.github/workflows/release.yml) from the tagged commit, without a build cache. `SHA256SUMS` lists every file's checksum. To check yours:

```bash
shasum -a 256 -c SHA256SUMS --ignore-missing   # macOS
sha256sum -c --ignore-missing SHA256SUMS        # Linux
```

They are not yet signed with a developer certificate.

## Reporting a vulnerability

Report it privately through the repository's Security tab (**Report a vulnerability**), not in a public issue. Include:
- what you found;
- how to reproduce it;
- the version or commit you used.

Expect an answer within a week. Only the latest release gets fixes.
