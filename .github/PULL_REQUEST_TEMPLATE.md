<!--
Never include a real credential in a diff, a fixture or a test. If a change
touches redaction, say so explicitly below.
-->

## What and why

<!-- What changes, and what problem it solves. Link the issue if there is one. -->

## Checks

- [ ] `cargo fmt --all --check` passes
- [ ] `cargo clippy --all-targets --all-features` passes with no warnings
- [ ] `cargo test --all-features` passes

## If this touches sensitive areas

<!-- Delete the sections that don't apply. -->

**Adapter** — fixtures added, with the expected normalized `Session` output.

**Stage-1 scan** — golden tests updated. Report the effect on a real corpus:
windows detected before and after.

**Redaction** — positive cases (the secret is removed) *and* negative cases
(legitimate high-entropy code survives intact) both added.

**Distillation prompt** — reject rate on the hand-labelled evaluation set,
before and after:

| | before | after |
| --- | --- | --- |
| accepted | | |
| rejected | | |
