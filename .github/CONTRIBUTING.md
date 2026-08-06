# Contributing to mycelium

Thanks for looking. This project is **pre-release**: the design is complete but
implementation hasn't started, so the most valuable contribution right now is
argument, not code.

## The most useful thing you can do today

Read [`docs/design/v0-design.md`](../docs/design/v0-design.md) and push back.

The design doc is written in French, as a working document. Section titles and
all identifiers, schemas and product strings are in English. If that's a barrier
for you, open an issue and say so — we'll translate it.

## Code contributions

Once implementation begins:

- `cargo fmt --all` and `cargo clippy --all-targets -- -D warnings` must pass.
- Changes to the TypeScript parser need tests covering new syntax and edge cases.
- Changes to import resolution need golden tests showing the effect on a real codebase:
  false positives and false negatives measured.
- Changes to the graph model should document the schema change and query impact.

## Commit messages

This project follows [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/).
Written in English, imperative mood, explaining *why* rather than *what* when the
diff doesn't already make it obvious.

```
<type>(<optional scope>): <description>

<optional body>
```

| Type       | Use for                                                           |
| ---------- | ----------------------------------------------------------------- |
| `feat`     | A new capability                                                  |
| `fix`      | A bug fix                                                         |
| `perf`     | A change that improves performance                                |
| `refactor` | A change that neither fixes a bug nor adds a feature              |
| `test`     | Adding or correcting tests                                        |
| `docs`     | Documentation only                                                |
| `build`    | Build system, dependencies, release tooling                       |
| `ci`       | CI configuration and workflows                                    |
| `chore`    | Anything that doesn't touch src or tests                          |
| `style`    | Formatting only, no behaviour change                              |

Scopes follow the crate layout: `graph`, `cli`, or a module such as
`parser`, `import-resolver`, `graph`, `renderer`.

```
feat(parser): support TypeScript enums
fix(import-resolver): resolve star imports correctly
docs: explain the graph model
```

A breaking change is marked with `!` before the colon — `feat(graph)!: …` — and
explained in the body.

## Scope

The design doc has an explicit non-goals section (§3). It's short, and it's there
so the tool stays one sharp thing instead of a broad TypeScript tooling suite. A PR
adding features beyond file/import graph extraction will be declined for v0 — but the
discussion about what v1 should cover is open, and welcome in an issue.
