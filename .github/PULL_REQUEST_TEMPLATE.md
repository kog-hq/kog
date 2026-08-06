## What and why

<!-- What changes, and what problem it solves. Link the issue if there is one. -->

## Checks

- [ ] `cargo fmt --all --check` passes
- [ ] `cargo clippy --all-targets --all-features` passes with no warnings
- [ ] `cargo test --all-features` passes

## If this touches sensitive areas

<!-- Delete the sections that don't apply. -->

**Parser / extractor** — tests added for new TypeScript syntax or edge cases.

**Import resolution** — report the effect on a test codebase: resolution accuracy before and after (false positives / false negatives).

**Graph model** — document schema changes and their impact on queries.

**Renderer** — include sample output format before and after.
