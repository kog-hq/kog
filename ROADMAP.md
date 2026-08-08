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

- **Twenty languages.** TypeScript, JavaScript, Vue, Svelte, Astro, Go, Python, Rust, C,
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

Still open, and deliberately overtaken by v0.3 below: a graph that cannot be asked anything
is worth less than one that reads two more languages, so the MCP server went first.

The gap list is the roadmap now: whatever the coverage report names most often across real
repositories is what gets read next. Ordered by what shows up:

- **MDX** — shipped. Measured at 0.9982 on [`withastro/docs`](https://github.com/withastro/docs),
  where it resolves 3,337 internal imports into 3,337 edges. Fenced code samples are
  excluded: documenso's embedding guides show `import … from '@documenso/embed-react'`
  inside a code block, and that package is real, so reading samples would have put
  fourteen fabricated edges into a published graph.
- **SQL** — deliberately not read, and this is the measurement rather than a plan.
  All 163 `.sql` files on documenso are Prisma migrations: 0 psql `\i`/`\ir` includes,
  0 naming another `.sql` file. An extractor would add 163 nodes with no edges and move
  source coverage five points without adding information. It becomes worth doing when a
  repository shows a real mechanism — psql includes, or dbt `{{ ref() }}` — not when the
  file count gets big enough.
- **Kotlin** — the same package-path rule as Java, on a grammar not yet wired in
- **Scala**, **Elixir**, **Dart**, **Lua**
- **Swift** — blocked, and honestly so: a Swift file imports no other file in its module,
  so a file-level graph of it is nodes and no edges. It needs symbol granularity, which is
  its own project.

A language ships when it passes its own resolution gate, not when its grammar compiles —
and as of 2026-08-07 that gate is a real repository per language rather than a unit test.
The first run of that corpus found Go at **0.1261** on `cli/cli`; see
[`docs/measurements/2026-08-07-a-real-corpus.md`](docs/measurements/2026-08-07-a-real-corpus.md).
C++ is at 0.8732 and is not yet fixed.

---

## v0.3 — MCP server — shipped

The point at which KOG became useful to an agent rather than only to a person.

An agent asking *"what depends on this file?"* greps, and grep answers with text matches.
On [`documenso`](https://github.com/documenso/documenso), `packages/prisma/index.ts` is the
most-depended-upon file in the graph — **484** real dependents, resolved through the
`@documenso/prisma` workspace-package alias. Grepping the repository for that file's own
path finds **0** matches: every one of those 484 imports goes through the alias, and grep
cannot connect an alias to the file it names. The agent concludes the file is unused, and
is wrong by all 484.

The graph already knew. It had no way to be asked. Now it does, five ways:

- `scan_summary` — the two numbers, the rate per language, the coverage gaps, and the
  files everything else points at
- `what_depends_on` — the 484
- `what_does_x_depend_on`
- `blast_radius` — transitive dependents to a depth, counted per hop
- `files_touching_package`

Structure, not file contents — so an answer costs tens of tokens where reading the files
would cost hundreds of thousands, and fit in no context window at all.

Two design rules carried over from the scan, because an answer is a published number too:

- **A list never caps its total.** It names at most `limit` files and reports exactly how
  many it held back. A blast radius stopped by its depth limit says so, so the number it
  gives reads as the floor it is.
- **A path is never guessed at.** One that names nothing returns the near names; one that
  names three files returns the three. Quietly picking one would make a wrong answer look
  like a right one.

`kog query "what depends on X"` runs the same code from a terminal — the same `Atlas`, the
same answers, the same rendering. Two implementations would drift, and only one of them
would be measured.

The transport is newline-delimited JSON-RPC 2.0 written against the specification, with no
new dependency and no async runtime. It answers both MCP eras: `server/discover` with
per-request `_meta` for `2026-07-28`, and the `initialize` handshake for `2025-11-25` and
earlier, which is what most clients still speak.

---

## v0.4 — exports — partly shipped

`--graphml` and `--cypher` on `kog scan`, each behind its own flag and never a default.
GraphML opens in Gephi, yEd and Cytoscape; the Cypher loads into Neo4j with `cypher-shell`
and uses `MERGE` throughout, so re-running a scan updates one graph rather than building a
second beside it. Ids are project-qualified, so a workspace of nine projects does not merge
nine `src/index.ts` into one node.

`--yaml` is the same record as the JSON, and `--markdown` is the odd one out: not the graph
but the *report* — the two numbers, the rate per language, what was not read, the files
everything points at, and every import that did not become an edge. It is the one export a
person reads rather than a tool.

**No image is written by the Rust side, and that is the decision rather than a gap.** SVG,
PNG and PDF all need a layout; the only layout KOG has runs in the browser. Writing a worse
one in Rust would produce a picture that looks authoritative and is arbitrary, and a second
layout would drift from the first besides. So the interface exports the PNG — exactly what
is on screen, laid out by the code that laid it out — and the formats above hand the graph
to tools that lay it out far better than either.

---

## v0.4 — packaging — wired, not yet cut

Deliberately after usefulness, and the shape is: `release-plz` decides *when* there is a
release and what version it carries, `dist` decides what a release *contains*, and they
meet at one git tag. Merging the release PR pushes `v0.2.0`; the tag builds five binaries
and creates the GitHub Release with them attached.

- **Five targets**: macOS on Apple silicon and Intel, Linux on x86-64 and ARM, Windows on
  x86-64. A shell installer and a PowerShell one, plus plain archives with SHA-256 sums
  for anyone who would rather not pipe a script into a shell.
- **The page is built before every one of them.** `kog-cli` embeds `app/dist` at compile
  time, so a release build is a bun build followed by a cargo build — expressed once, in
  `.github/workflows/build-setup.yml`, which `dist` injects into each build job.
- **Not on crates.io, and not as an oversight.** `build.rs` reads `../../app/dist`, which
  is outside the package, so what `cargo publish` uploads cannot build. `publish = false`
  in both manifests says so and says why; the prebuilt binary is the install path.

Measured rather than assumed: `dist build --artifacts=local` produces a 2.6 MB
`kog-cli-aarch64-apple-darwin.tar.xz` whose binary answers `kog 0.1.0` and serves the
embedded page. `just release-plan`, `just release-build` and `just release-ci` run the
same three steps a contributor needs.

Two things stand between this and a downloadable binary, both one-time and both requiring
a decision rather than code:

- a `RELEASE_PLZ_TOKEN` secret. A tag pushed with the built-in `GITHUB_TOKEN` starts no
  workflow, so without it the release exists and carries nothing — silently. This is
  written at the point of failure in `release-plz.yml`, because it is the one trap here
  that looks like a bug in someone else's code.
- the first tag. Nothing about the numbers above changes when it lands.

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
