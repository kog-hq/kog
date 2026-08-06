# Roadmap

KOG maps a codebase into a graph and publishes numbers you can audit. The order below is
deliberate: usefulness first, packaging last.

Anything not listed here is not planned. If something matters to you, open an issue — the
list is short on purpose.

---

## v0 — shipped

TypeScript projects parsed into a file/import graph, with the resolution measured rather
than asserted.

- tree-sitter extraction, four resolution rules applied in order: relative, tsconfig
  `paths` alias (across `extends` chains, JSONC-tolerant), workspace package, external
- every specifier that does not become an edge is named in `stats.diagnostics` with its
  file, line and category — the number is auditable, not just published
- `kog` in a project directory: scans, serves an embedded page, opens the browser. One
  binary, no JS toolchain, no checkout

---

## v0.1 — every language, every file, one graph per project — shipped

v0 read TypeScript and dropped everything else without saying so. A tool that supports one
language and silently ignores the other nine scores a perfect resolution rate on a
polyglot repository.

- **Sixteen languages.** TypeScript, JavaScript, Vue, Svelte, Astro, Go, Python, Rust, C,
  C++, Java, C#, Ruby, PHP, HTML, CSS/Sass/Less, shell. One `Extractor` implementation
  each, dispatched by extension through a registry. Go proved the abstraction holds where
  the node stops being a file: an import names a package, one specifier resolves to a set
  — and Java's wildcard imports and C#'s namespaces then reused that same shape.
- **A rate per language.** An aggregate lets a broken resolver hide behind a majority
  language that works — publishing per language immediately turned up two real bugs that
  had moved the repository-wide number by less than a point.
- **A coverage report.** Every file the walker visits is classified: analysed, source in a
  language with no extractor yet (named, with its language), or not source at all. The gap
  is a number and a list, not an omission.
- **Every file is a node.** An image, a PDF, a lockfile and a Haskell module are all in the
  graph. Assets are measured rather than opened, so a binary is mapped at no cost.
- **The reason for every exclusion**, machine-readable, so the categorisation tables in
  `docs/measurements/` are regenerable with `jq` alone.
- **One graph per project.** Pointed at a directory that contains projects rather than
  being one, the scan produces a graph each. A monorepo stays one project: its workspace
  packages only resolve from the root.
- Graph coloured by the second path segment — on a monorepo every `apps/*` used to share
  one colour, which wasted the only visual channel that carries structure.

Measured on the same two public repositories as v0: [`documenso`](https://github.com/documenso/documenso)
at rate 0.9779 with 0.8725 source coverage, and [`TanStack/query`](https://github.com/TanStack/query)
at 0.9926 with 1.0000. Full evidence: [`docs/measurements/`](docs/measurements/).

---

## v0.2 — the languages the coverage report keeps naming

The gap list is the roadmap now: whatever the coverage report names most often across real
repositories is what gets read next. Ordered by what shows up:

- **SQL** and **MDX** — the two largest gaps on documenso, 306 files between them
- **Kotlin** — the same package-path rule as Java, on a grammar not yet wired in
- **Scala**, **Elixir**, **Dart**, **Lua**
- **Swift** — blocked, and honestly so: a Swift file imports no other file in its module,
  so a file-level graph of it is nodes and no edges. It needs symbol granularity, which is
  its own project.

A language ships when it passes its own resolution gate, not when its grammar compiles.

---

## v0.3 — MCP server

The point at which KOG becomes useful to an agent rather than only to a person.

An agent asking *"what depends on this file?"* today greps, and grep answers with text
matches. On [`documenso`](https://github.com/documenso/documenso),
`packages/prisma/index.ts` is the most-depended-upon file in the graph — **484** real
dependents, resolved through the `@documenso/prisma` workspace-package alias. Grepping the
repository for that file's own path finds **0** matches: every one of those 484 imports
goes through the alias, and grep cannot connect an alias to the file it names. The agent
concludes the file is unused, and is wrong by all 484.

The graph already knows. It just has no way to be asked. Planned queries:

- what depends on `X`
- what does `X` depend on
- blast radius of changing `X`
- which files touch package `Y`

Structure, not file contents — so an answer costs tens of tokens where reading the files
would cost hundreds of thousands, and fit in no context window at all.

---

## v0.4 — packaging

`release-plz` and `cargo-dist`: versioned releases, generated changelog, prebuilt binaries
for macOS, Linux and Windows, and a shell installer. Deliberately after usefulness.

---

## v0.5 — Tauri shell

The desktop application wraps a core that already works and is already useful from the
terminal and from an agent.

---

## Beyond

Not scheduled, listed so the direction is legible:

- Leiden clustering, so communities are separated by colour rather than by luck of the layout
- Cross-project edges: a folder of services that call each other over HTTP is one system,
  and the graphs currently say nothing about that
- Overlays: what an AI wrote and you never reviewed, dead code, where a project stopped,
  what changed this week
- Piloting agents from the map
- Session graph, linking your AI conversations to the code they touched
- Symbol-level granularity, which is also what unblocks Swift
