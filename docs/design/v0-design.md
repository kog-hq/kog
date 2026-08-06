# KOG — design v0

- **Date**: 2026-08-06
- **Status**: in review
- **v0 scope**: Rust parser from a TypeScript project to a file/import graph,
  CLI, WebGL rendering. No Tauri, no clustering, no AI.

> Documentation is in English; the design, plan and measurement documents were
> originally written in French and translated when the project went public.

---

## 1. Problem

A developer who accumulates codebases has no overview of their own code.
Existing tools split into two camps that don't talk to each other:

| Camp | Examples | What they do | What's missing |
| --- | --- | --- | --- |
| Cartographers | Graphify-Labs/graphify | Understand the code, produce a graph | No steering, single-project, static HTML export |
| Pilots | Open Cowork, CloudCLI, agents-ui, OpenHands | Launch AI agents | No understanding of the code |

KOG targets the intersection: **seeing your code and steering your agents over it, on
the same map**. v0 only handles the first half — and not even all of it.

### Market reference

`Graphify-Labs/graphify`, measured on 2026-08-06 via the GitHub API:

- 103,112 stars, 10,020 forks, 819 open issues
- created 2026-04-03 → **4 months**
- Python, Apache-2.0 (relicensed from MIT), Y Combinator S26

This is proof the market exists, and the competitor to beat.

---

## 2. What Graphify measures — and doesn't measure

Recorded in `BENCHMARKS.md` (updated 2026-07-05) and `docs/how-it-works.md`:

| Benchmark | Metric | graphify | Competitors |
| --- | --- | --- | --- |
| LOCOMO (n=300) | recall@10 | 0.497 | mem0 0.048 · supermemory 0.149 |
| LOCOMO (n=300) | QA accuracy | 45.3 % | supermemory 49.7 % · mem0 27.3 % |
| LongMemEval-S (n=50) | QA accuracy | 76 % | tied with dense RAG |
| ERPNext (~1M LOC) | key-fact coverage | **82.0 %** vs 70.8 % (baseline grep+read) | — |

The "code" figure (82.0 %) measures a **downstream effect** — whether an agent answers
better — on **n = 6 questions**, at ~140K tokens per query. It says nothing about the
correctness of the graph itself.

Yet their code pipeline is the same family as ours:

> "Tree-sitter parses your code files […] This runs locally with no LLM involved.
> 25 languages supported."
> "Code files are **not** sent to the LLM semantic extractor in the normal pipeline."

Each relation is tagged `EXTRACTED` (confidence 1.0), `INFERRED` (Claude, 0.55–0.95)
or `AMBIGUOUS`. For code, everything is `EXTRACTED`: deterministic, like ours.

**What they never publish: what fraction of imports was resolved**, neither overall
nor per language. A failed import surfaces nowhere — it doesn't exist as a failure,
it disappears from the graph.

That's KOG's opening. The resolution rate costs nothing to produce, is verifiable
repo by repo, and becomes a public argument.

---

## 3. Decisions, and the measurements that dictated them

### 3.1 Node = file, edge = import

Ruled out: node = symbol, edge = call. Call resolution in TypeScript (aliases,
re-exports, dynamic dispatch, methods) falls below 70 % accuracy without a full
type-checker, and **a wrong graph is worse than a coarse graph**.

On the largest project on the reference machine, the file model gives ~727 nodes
and ~2,100 edges: readable, verifiable, enough to prove the Rust → WebGL chain.

### 3.2 TypeScript only in v0

Census of the reference machine, 2,209 code files:

| Extension | Files | Share |
| --- | ---: | ---: |
| `.tsx` `.ts` `.js` `.jsx` | 2,097 | **94.9 %** |
| `.go` | 62 | 2.8 % |
| `.swift` | 34 | 1.5 % |
| `.rs` | 10 | 0.5 % |
| `.sh` | 6 | 0.3 % |

But share isn't the main argument. The file+imports model **doesn't apply equally
to every language**:

- **Swift — the model produces nothing.** The 34 files in the reference project import
  exclusively system frameworks: `SwiftUI` ×23, `Foundation` ×12, `Combine` ×9,
  `WidgetKit` ×5, `StoreKit` ×3, `AVFoundation` ×2, `UserNotifications`, `Supabase`.
  **Zero internal imports.** This is structural: in Swift, files within the same
  module see each other without importing. The graph would be 34 nodes and 0 edges.
  Swift is waiting on the symbol level.
- **Go — applicable, but the node changes nature.** 73 internal imports out of 298
  total, pointing to **packages** (directories), not files:
  `ClientServer/internal/model` designates 15 files. A second node model, a second
  resolver, for ~18 nodes.
- **Rust — 10 files**, no demonstration value.

### 3.3 A language's entry rule

> **A language enters KOG when it passes its own resolution gate — not
> when its grammar compiles.**

Every supported language publishes its own rate. A language whose model produces no
edges (Swift today) is documented as such rather than listed empty.

This is the direct answer to Graphify's 25 unmeasured languages: fewer languages,
each with a number.

### 3.4 Alias resolution: mandatory

Breakdown of the 4,355 `from '…'` specifiers in the reference project:

| Category | Count |
| --- | ---: |
| Internal aliases (`@/` `@common/` `@modules/` `@scope/` `@lib/`) | 2,651 |
| Relative (`./` `../`) | 558 |
| **Total internal** | **3,209** (73.7 %) |
| External (`@nestjs` 282, `react` 261, `lucide-react` 196, `next` 98, `@tanstack` 41…) | 1,146 |

These 1,146 external specifiers spread across **77 distinct packages**.

A resolver that ignores tsconfig `paths` loses **82.6 % of internal edges**. Reading
tsconfig is therefore not a side option, it's the core of the parser.

The reference project is moreover a private 727-file Turborepo monorepo:
`workspaces: ["apps/*", "packages/*"]`, a root `tsconfig.base.json` and five
nested tsconfigs, with cross-package `@scope/*` imports between packages. It's
the hardest case, so it's the right test bench.

### 3.5 External dependencies: ignored, but counted

Nodes are exclusively the project's files — the topology stays that of the code.
Every node carries `external_deps: ["react", "next"]`, which will later allow
filtering ("which files depend on Prisma?") without re-running the parser or
polluting the layout with 257-edge hubs.

### 3.6 Renderer: sigma.js

| Library | Version | License |
| --- | --- | --- |
| `sigma` + `graphology` | 3.0.3 / 0.26.0 | **MIT** |
| `@cosmograph/cosmos` | 3.4.1 | **CC-BY-NC-4.0** |

Cosmograph is under a non-commercial, non-OSI license, incompatible with MIT+Apache.
Ruling it out is a licensing constraint, not a technical preference.

### 3.7 Prototype shape: CLI before Tauri

The crate and the CLI produce `graph.json`; a Vite+sigma page loads it. Tauri only
shows up after the gate, to wrap a crate that's already been tested. If the graph
turns out to be useless, that's learned in hours rather than days.

---

## 4. Architecture

```
kog/
├── Cargo.toml                 workspace
├── crates/
│   ├── kog-graph/        lib — extraction, resolution, assembly
│   └── kog-cli/          bin — kog scan <dir> -o graph.json
├── app/                       Vite + React + TS + sigma
└── docs/design/, docs/plans/
```

`kog-graph`, one module per role:

| Module | Responsibility | Depends on |
| --- | --- | --- |
| `model.rs` | `Graph` / `Node` / `Edge` / `Stats`, serde. Language-agnostic, zero logic | — |
| `extractor.rs` | `Extractor` trait: `extensions()`, `extract(source) -> Vec<Specifier>`, `resolve(...)` | tsconfig |
| `discover.rs` | Traversal, respects `.gitignore`, filtering by declared extensions | — |
| `tsconfig.rs` | Reading and merging tsconfig (`extends`, `paths`, `baseUrl`) — the core of the parser, 719 lines | discover |
| `extractors/typescript.rs` | tree-sitter TS/TSX grammar, TS resolution rules | extractor, tsconfig |
| `graph.rs` | Assembly, deduplication, statistics | all |

The `Extractor` trait exists from day one. Adding Go must be **one file**,
not a refactor — and that's precisely what v0.2 will verify.

### Rust dependencies

`tree-sitter` 0.26.11 and `tree-sitter-typescript` 0.23.2. The latter depends only on
`tree-sitter-language ^0.1`, the stable-ABI shim: the two versions line up without
intervention.

---

## 5. Data model

```jsonc
{
  "nodes": [
    {
      "id": "apps/frontend/src/lib/api.ts",   // path relative to the scanned root
      "path": "apps/frontend/src/lib/api.ts",
      "lang": "typescript",
      "loc": 143,
      "external_deps": ["react", "@tanstack/react-query"]
    }
  ],
  "edges": [
    { "source": "apps/frontend/src/app/page.tsx", "target": "apps/frontend/src/lib/api.ts", "kind": "import" }
  ],
  "stats": {
    "files_discovered": 727,
    "files_parsed": 727,
    "specifiers_total": 4375,
    "specifiers_internal": 3211,
    "resolved": 3160,
    "unresolved": 0,
    "excluded": 51,
    "resolution_rate": 1.0,
    "external_specifiers": 1164,
    "external_packages_distinct": 77,
    "failures": [],
    "diagnostics": [
      {
        "path": "apps/backend/prisma/seed-travaux.ts",
        "line": 2,
        "specifier": "../src/generated/prisma/client",
        "kind": "excluded"
      }
    ]
  }
}
```

Figures as measured on the acceptance target, see
`docs/measurements/2026-08-06-v0-gate.md`.

`resolution_rate` = `resolved / (specifiers_internal - excluded)`. Externals are
outside the calculation: an `import react` doesn't have to point to a file. `excluded`
is removed from the denominator for the same reason: a specifier that resolves to a
real file deliberately out of scope (gitignored, a directory always excluded, or an
extension the extractor doesn't claim) is not a failure of the resolver — counting it
against the rate would understate the resolver's quality instead of measuring it.
Conversely, a file the tool itself failed to read or parse is never `excluded`: that's
our failure, not an out-of-scope target, so it stays `unresolved` and keeps weighing
on the rate.

`diagnostics` (capped at `MAX_DIAGNOSTICS`, cf. `model.rs`) identifies, with file and
line, every `unresolved` or `excluded` specifier — a count alone isn't auditable (§7).
In exchange, it only records the fact that a specifier was excluded, never *why*
(gitignored? a directory always excluded? an unclaimed extension?): that distinction
only exists by checking it by hand on disk (see the corresponding limitation in the
measurement document, §12).

---

## 6. TypeScript resolution rules

Applied in order, first match wins:

1. **Relative** — `./x`, `../x` resolved from the importer's directory.
2. **tsconfig alias** — table built by following `extends` chains, `paths`
   interpreted relative to `baseUrl` (or to the tsconfig's directory if absent). The
   applicable tsconfig is the closest one walking up the tree.
3. **Workspace package** — root `package.json`, `workspaces` field; a specifier
   `@scope/pkg` matching a local package is resolved to its `main`/`exports`,
   failing that to its `index.ts`.
4. **External** — everything else. Recorded in `external_deps`, never as an edge.

For any resolved target, the order tried is: exact path, then `.ts`, `.tsx`,
`.js`, `.jsx`, then `<dir>/index.{ts,tsx,js,jsx}`. A `.js` specifier is also
tried as `.ts` (ESM/NodeNext convention).

### Cases measured on the reference project

| Case | Count | Treatment |
| --- | ---: | --- |
| `import type` | 303 | Resolved normally — points to real files |
| `export … from` re-exports | 24 | Normal edge; no barrel traversal in v0 |
| `index.ts` files | 5 | Directory resolution |
| Asset imports (`.png`) | 1 | Resolved (real file found on disk), then excluded — extension outside the scope claimed by the TypeScript extractor |
| Dynamic `import()` | 0 | Out of scope for v0 |

These figures show that the real difficulty comes down to aliases and the missing
extension. The rest is marginal.

---

## 7. Errors — never silent

| Situation | Behaviour |
| --- | --- |
| Root missing or unreadable | **Immediate failure**, non-zero exit code |
| File not parsable | Skipped, logged in `stats.failures`, the scan continues |
| Import not resolved | Counted in `stats.unresolved`, never dropped silently |
| tsconfig unreadable or invalid | Warning, relative resolution only on this subtree |

No filter should *fail open*: a filter that cannot apply excludes rather than
including at random.

---

## 8. Tests

Tiny synthetic fixtures, one per resolution rule, on the model set by dejavu
(1,777 LOC, 51 tests):

- relative import, simple and upward-traversing
- tsconfig alias, with and without `baseUrl`
- `extends` chain two levels deep
- monorepo workspace package
- directory resolution to `index.ts`
- missing extension and a `.js` specifier → `.ts` file
- unresolvable specifier → counted, not fatal
- unparsable file → logged, scan continues

The surface under test is `extractors/typescript.rs`, not file traversal.

---

## 9. Acceptance gate

v0 is "done" when, and only when:

1. `kog scan` on the reference monorepo project (727 files) shows a
   **`resolution_rate` ≥ 0.95**, measured and printed, never estimated.
2. `kog scan` on a simple project (93 files, `@/*` alias) produces a coherent
   graph.
3. The sigma page displays the monorepo graph and stays smooth on pan/zoom.
4. Green CI: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, gitleaks.

Until these four points are met, nothing else begins.

---

## 10. Out of scope for v0

A stopping rule, not abandonment. No line of code on: Tauri and `src-tauri/`,
Leiden clustering, multi-project, AI chat and permissions, overlays (dead code,
unreviewed AI code, recent activity), **session graph** — even though dejavu's
transcript parser is available and ready —, quota gauge, MCP manager, diff checker,
CLAUDE.md sync.

Every one of these pieces is more exciting than alias resolution, which is exactly
why they would derail v0. If one becomes necessary along the way, the question gets
asked before it gets settled.

---

## 11. After the gate

| Version | Content | What it proves |
| --- | --- | --- |
| v0.2 | Go extractor (node = package) | That the `Extractor` trait holds up on a second node model |
| v0.3 | Tauri shell around the crate | That the core distributes as a single binary |
| next | Multi-project, Leiden, overlays, steering | The full vision |

Swift will only enter with the symbol level, and will be documented as such until then.

---

## 12. Reuse from dejavu

To pull from `~/apps/dejavu`: MIT + Apache-2.0 licenses, `.gitleaks.toml`,
`rust-toolchain.toml`, CI workflow, `CODE_OF_CONDUCT.md`, `CONTRIBUTING.md`,
`SECURITY.md`, issue and PR templates.

The transcript parser (`adapters/claude_code.rs`, 278 lines, 9 tests) will feed the
session graph — **after** v0.

---

## 13. v0.1 — `kog` with no argument

First feedback on v0 once the gate was passed: seeing a graph required three
commands, a repo clone and `bun` — `kog scan ~/project -o
app/public/graph.json`, then `cd app && bun run dev`. That directly contradicts the
differentiator claimed against the competitor (§3.7, §11): "one binary, zero
dependencies", when the Python competitor measured in §1 has issues full of
install pain. Requiring a repo checkout and a JS toolchain to see the result
concedes exactly what KOG claims to avoid. This feature is therefore on the
project's trajectory, not an added convenience.

Decisions:

- `ROOT` defaults to `.` on both `scan` and `view` — typing a path becomes
  optional everywhere.
- `kog` with no subcommand explicitly means `kog view .`, wired into the clap
  setup rather than left to implicit behaviour.
- `--stats-only` goes away: `scan` only writes a file if `-o` is given, rather
  than writing by default and offering a flag to opt out. Same behaviour, less
  surface.
- `view` never touches disk: the graph is kept in memory and served as-is.
  Running `kog` in your project must never leave a `graph.json` behind.

The page (`app/dist`, produced by `bun run build`) is embedded in the binary via
`rust-embed`, served by `tiny_http` — synchronous, so no async runtime enters the
binary — and the browser is opened via `open`. The server listens on `127.0.0.1`
only (it's the structure of the user's own source code being served; never
`0.0.0.0`), on a port chosen by the OS (bind on port 0, read back afterwards) rather
than a fixed port like 4173/5173, which might already be taken. The URL is printed
before the browser opens, to stay useful over SSH or if opening fails.

`crates/kog-cli/build.rs` checks that `app/dist/index.html` exists before
compiling and fails with the exact command to produce it (`cd app && bun install &&
bun run build`) rather than letting the `rust-embed` macro fail with a "file not
found" that points nowhere. It also emits `cargo:rerun-if-changed` on `app/dist`:
without that, a rebuilt page would stay embedded, stale, in the next compiled binary
— the most expensive failure mode to discover late.

---

## Appendix — reference environment

Development machine, verified on 2026-08-06:

- cargo 1.97.1, toolchain `stable-aarch64-apple-darwin`.
  `~/.cargo/bin` **missing from the non-interactive PATH** — fix before the first build.
- node v22.23.2, bun 1.3.8
- Xcode CLT present
- `gh` authenticated on the target account
