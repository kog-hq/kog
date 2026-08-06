# Contributing to mycelium

Thanks for looking. This project is **pre-release**: the design is complete but
implementation hasn't started, so the most valuable contribution right now is
argument, not code.

## The most useful thing you can do today

Read [`docs/design/v1-design.md`](../docs/design/v1-design.md) and push back.

The design doc is written in French, as a working document. Section titles and
all identifiers, schemas and product strings are in English. If that's a barrier
for you, open an issue and say so — we'll translate it.

Two areas genuinely need scrutiny:

**Stage-1 detection (§6).** Precision and recall are unmeasured. The heuristic is
"a tool fails, then an equivalent command later succeeds." It will produce false
positives (flaky tests, environment hiccups) and false negatives (problems solved
without ever running a command). If you know a better structural signal, that's
the highest-leverage thing you can tell us.

**Redaction (§6.2).** The tool reads your entire history and sends windows to an
LLM. If you can find a secret format that slips through the rules described
there, that's a security issue — see [SECURITY.md](SECURITY.md).

## Reporting a transcript format

Adapters are the part most likely to break when upstream tools change their
format, and we can only support tools we have samples for. If you use an
assistant we don't cover, an **anonymized** sample transcript is a great
contribution.

Scrub it before sending. We will not accept a sample containing credentials.

## Code contributions

Once implementation begins:

- `cargo fmt --all` and `cargo clippy --all-targets -- -D warnings` must pass.
- New adapters need fixtures with the expected normalized `Session` output.
- Changes to stage-1 scanning need golden tests covering the edge cases listed
  in the design doc: failure with no resolution, nested failures, agent-mechanics
  noise, sessions exceeding the window cap.
- Changes to redaction need both positive cases (secrets are removed) and
  negative cases (legitimate high-entropy code isn't mangled).
- Changes to the distillation prompt should be checked against the hand-labelled
  evaluation set before and after, and the reject rate reported in the PR.

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

Scopes follow the crate layout: `core`, `cli`, `mcp`, `app`, or a module such as
`adapters`, `scan`, `distill`, `redact`, `vault`, `index`.

```
feat(adapters): read Codex CLI rollout transcripts
fix(scan): stop treating "File has not been read yet" as a real failure
docs: state that the cli distiller sends windows to its provider
```

A breaking change is marked with `!` before the colon — `feat(vault)!: …` — and
explained in the body.

## Scope

The design doc has an explicit non-goals section (§3). It's short, and it's there
so the tool stays one sharp thing instead of a knowledge-management suite. A PR
adding note types beyond "solved problem" will be declined for v1 — but the
discussion about whether v2 should broaden is open, and welcome in an issue.
