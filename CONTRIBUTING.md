# Contributing

Thanks for helping. The rules every change follows are in [AGENTS.md](AGENTS.md); this page is about getting set up.

## Setup

- Rust stable (1.94.1 or newer; the AWS SDK needs it).
- Node 22 or newer.
- For the desktop app, the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS.

```bash
git clone https://github.com/gonzaloan/sprawlr && cd sprawlr
cargo test --workspace --no-default-features   # core, adapters, drivers
cd app && npm ci && npm test && npm run dev    # web demo at http://localhost:5173
npx tauri dev                                  # desktop app
```

`aws-sdk-ec2` takes about 10 GB of RAM to compile. `--no-default-features` skips it, along with VPC and security group scanning.

## Before a pull request

1. Read the definition of done in [AGENTS.md](AGENTS.md#definition-of-done).
2. For a new screen, port or anything that costs the user money, write a spec in [docs/specs](docs/specs/) first.
3. For a change to a rule in AGENTS.md, add an ADR in [docs/adr](docs/adr/).
4. Keep one topic per pull request.

## Scanning your own account

Use the read-only policy in [iam-policy.json](iam-policy.json). Its `OptionalErrorLogs` statement is only needed for error logs; delete it if you don't want sprawlr reading log content.
