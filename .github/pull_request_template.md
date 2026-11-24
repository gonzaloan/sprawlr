## What changes for the person using sprawlr

<!-- One or two sentences. Screenshots if the UI changed. -->

## How

<!-- The approach, and anything a reviewer should look at first. -->

## Checklist

- [ ] Tests that fail without this change
- [ ] `cargo test`, `cargo clippy -D warnings`, `cargo fmt --check`
- [ ] `npm run typecheck`, `npm test`, `npm run build`
- [ ] Demo data regenerated (if domain output changed)
- [ ] README / docs / ADR / spec updated where needed
- [ ] Still read-only; no new stored values that could be secrets or personal data
