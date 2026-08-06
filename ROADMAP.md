# Roadmap

KOG maps a codebase into a graph and publishes a resolution rate you can audit. The order
below is deliberate: usefulness first, packaging last.

Anything not listed here is not planned. If something matters to you, open an issue — the
list is short on purpose.

---

## v0 — shipped

TypeScript projects parsed into a file/import graph, with the resolution measured rather than
asserted.

- tree-sitter extraction, four resolution rules applied in order: relative, tsconfig `paths`
  alias (across `extends` chains, JSONC-tolerant), workspace package, external
- every specifier that does not become an edge is named in `stats.diagnostics` with its file,
  line and category — the number is auditable, not just published
- `kog` in a project directory: scans, serves an embedded page, opens the browser. One binary,
  no JS toolchain, no checkout
- measured on two public repositories a reader can clone:
  [`documenso/documenso`](https://github.com/documenso/documenso) (2,073 files, Turborepo) at
  rate 0.9778, and [`TanStack/query`](https://github.com/TanStack/query) (1,027 files, Nx/pnpm
  library monorepo) at rate 0.9946 — every remaining gap on both is categorised by hand

Full evidence and reproduction commands: [`docs/measurements/`](docs/measurements/).

---

## v0.1 — honesty and packaging

Closing the gaps the v0 review found, and making the thing installable.

- **Record *why* each specifier was excluded.** Today `graph.json` says a specifier was
  excluded but not whether that was a gitignored target, a non-source extension, or a skipped
  directory. The categorisation table in the measurement document had to be produced by hand
  with filesystem inspection. Until this lands, "reproducible" is only half true.
- Confine `main`/`exports` and tsconfig alias targets to the scan root
- Colour the graph by the second path segment — on a monorepo every `apps/*` currently shares
  one colour, which wastes the only visual channel that carries structure
- `release-plz` and `cargo-dist`: versioned releases, generated changelog, prebuilt binaries
  for macOS, Linux and Windows, and a shell installer

---

## v0.2 — Go

A second language, chosen to test the architecture rather than to pad a count.

Go's imports resolve to **packages**, not files, so the node stops being a file. If adding it
means one new file implementing `Extractor`, the abstraction holds. If it means a refactor, it
does not, and better to learn that at 62 files than at six languages.

A language ships when it passes its own resolution gate, not when its grammar compiles. Every
supported language publishes its own rate.

---

## v0.3 — MCP server

The point at which KOG becomes useful to an agent rather than only to a person.

An agent asking *"what depends on this file?"* today greps, and grep answers with text matches.
On [`documenso/documenso`](https://github.com/documenso/documenso), `packages/prisma/index.ts`
is the most-depended-upon file in the graph — **484** real dependents, resolved through the
`@documenso/prisma` workspace-package alias. Grepping the repository for that file's own path,
`packages/prisma/index.ts`, finds **0** matches: every one of those 484 imports goes through
the alias, and grep cannot connect an alias to the file it names. The agent concludes the file
is unused, and is wrong by all 484.

The graph already knows. It just has no way to be asked. Planned queries:

- what depends on `X`
- what does `X` depend on
- blast radius of changing `X`
- which files touch package `Y`

Structure, not file contents — so an answer costs tens of tokens where reading the files would
cost hundreds of thousands, and fit in no context window at all.

---

## v0.4 — Tauri shell

Packaging, deliberately after usefulness. The desktop application wraps a core that already
works and is already useful from the terminal and from an agent.

---

## Beyond

Not scheduled, listed so the direction is legible:

- Leiden clustering, so communities are separated by colour rather than by luck of the layout
- Multiple projects in one map — the reason this exists
- Overlays: what an AI wrote and you never reviewed, dead code, where a project stopped, what
  changed this week
- Piloting agents from the map
- Session graph, linking your AI conversations to the code they touched
- Symbol-level granularity, which is also what unblocks Swift — a language whose files import
  no other files, so a file-level graph of it is 34 nodes and zero edges
