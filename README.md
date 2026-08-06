<p align="center">
  <a href="https://github.com/kog-hq/kog">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="./assets/logo-dark.svg" />
      <source media="(prefers-color-scheme: light)" srcset="./assets/logo.svg" />
      <img src="./assets/logo.svg" width="100px" alt="KOG logo" />
    </picture>
  </a>
</p>

<h2 align="center">Every tool draws your codebase. KOG tells you what it missed.</h2>

<p align="center"><a href="./docs/design/v0-design.md"><img src="./assets/icons/book.svg" width="12" height="12"/> Design</a> · <a href="./ROADMAP.md"><img src="./assets/icons/map.svg" width="12" height="12"/> Roadmap</a> · <a href="./docs/measurements/"><img src="./assets/icons/star.svg" width="12" height="12"/> Measurements</a></p>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="./assets/cover-dark.svg" />
    <source media="(prefers-color-scheme: light)" srcset="./assets/cover-light.svg" />
    <img src="./assets/cover-light.svg" alt="KOG banner" />
  </picture>
</p>

<br />

> [!NOTE]
> **v0.** TypeScript only, file-level granularity. The graph is a proof of the pipeline,
> not an interface yet — no search, no filters, no clustering. What is finished is the
> part that has to be right: the resolution, and the number that reports it.

<br />

# Why KOG

A dependency graph is easy to draw and easy to get wrong, and a wrong one is worse than
none — it looks authoritative while quietly omitting edges.

Take the most-depended-upon file in [documenso](https://github.com/documenso/documenso),
`packages/prisma/index.ts`. Ask an agent, or yourself, *what depends on this?* The obvious
move is to grep for its path:

```bash
rg -l "packages/prisma/index" -g '*.ts' -g '*.tsx'   # 0 files
rg -l "@documenso/prisma"     -g '*.ts' -g '*.tsx'   # 476 files
```

Not one import names the path. All 476 go through the workspace package name, and grep
cannot connect the two. The graph finds 484 edges into that file; the obvious answer finds
none, and nothing says so.

Resolving it means reading the root `package.json` `workspaces`, finding the package that
declares that name, and following its `types` entry to source rather than its `main` entry
into `dist/`.

KOG does that, along with `tsconfig` `paths` across `extends` chains — and then reports,
specifier by specifier, everything it still could not resolve.

<br />

# Installation

### <img src="./assets/icons/rocket.svg" width="14" height="14"/> Build and install

```bash
git clone https://github.com/kog-hq/kog
cd kog
just install
```

Builds the page, then the CLI that embeds it, then installs `kog` to `~/.cargo/bin`.
No `just`? The [`justfile`](justfile) shows the two commands it runs.

### <img src="./assets/icons/code.svg" width="14" height="14"/> Run it

```bash
cd ~/your-project
kog
```

Scans the current directory, serves the graph from a page embedded in the binary, opens
your browser. Nothing is written to your project.

```bash
kog view ~/other      # view a different project
kog scan              # stats to stdout, writes nothing
kog scan -o g.json    # write the graph as JSON
```

The server binds to `127.0.0.1` on an OS-assigned port and prints the URL before opening
anything, so `kog` over SSH still tells you where to look.

<br />

# The number, and how to check it

Every scan reports a resolution rate. Externals leave the denominator — `import react` has
no file to point at. So do **excluded** specifiers: those resolved to a real path outside
the scanned set, which is a policy decision, not a parser failure.

```
resolution rate = resolved / (internal − excluded)
```

Measured on two public repositories, at a commit you can check out yourself:

| Repository | Files | Internal | Resolved | Unresolved | Rate |
| --- | ---: | ---: | ---: | ---: | ---: |
| [documenso/documenso](https://github.com/documenso/documenso) | 2,073 | 10,346 | 10,101 | 229 | **0.9778** |
| [TanStack/query](https://github.com/TanStack/query) | 1,027 | 1,588 | 1,462 | 8 | **0.9946** |

The remainder is not hidden. Every specifier that did not become an edge is listed in
`stats.diagnostics` with its file, its line and its category — on documenso, 225 of the 229
are imports into code that `prisma generate` and `react-router typegen` produce, absent from
a fresh clone. Run those generators and they resolve.

Reproduce it:

```bash
git clone --depth 1 https://github.com/documenso/documenso
kog scan documenso
```

Full breakdown, with the raw output and every gap categorised, in
[`docs/measurements/`](docs/measurements/).

<br />

# <img src="./assets/icons/shield.svg" width="20" height="20"/> What it does not do

Stated plainly, because a tool that only advertises its strengths is not measuring anything.

- **One language.** TypeScript and TSX. Go is next, and it will publish its own rate — a
  language ships when it passes its own gate, not when its grammar compiles.
- **Files, not symbols.** Nodes are files, edges are static imports. Call graphs need a type
  checker to be right, and a wrong call graph is worse than a coarse import graph.
- **No dynamic `import()`**, no `require()`.
- **The renderer is a prototype.** It proves 2,000 nodes render and stay interactive. It is
  not an interface.
- **Two repositories is not a corpus.** The rates above are two data points, both TypeScript.

<br />

# Stack

Rust, [tree-sitter](https://tree-sitter.github.io) for parsing, no LLM anywhere in the
pipeline — resolution is deterministic or it is not reported. The page is
[sigma.js](https://www.sigmajs.org) and [graphology](https://graphology.github.io) on WebGL,
built with Vite and embedded in the binary at compile time.

<br />

# Project status

v0 is complete and measured. [`ROADMAP.md`](ROADMAP.md) has the order: recording *why* each
specifier was excluded, then Go, then an MCP server so an agent can ask the graph questions
instead of grepping, then a desktop shell.

Contributions welcome — [`.github/CONTRIBUTING.md`](.github/CONTRIBUTING.md).

<br />

# License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE), at your option.
