# 0005. Reads that cost money are visible and optional

Date: 2026-11-24 · Status: Accepted

## Context

Discovery calls are free, but CloudWatch `GetMetricData` and Logs Insights are billed per request or per GB scanned. Logs also contain content.

## Decision

- Telemetry is on by default because it costs cents per scan, and it can be turned off.
- Error logs are off by default, and the IAM policy grants them in a separate optional statement.
- The UI and CLI help say what each one costs.

## Consequences

- No surprise bills. Error patterns are missing until the user opts in.
