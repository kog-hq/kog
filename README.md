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
> **v0.1.** Sixteen languages, file-level granularity. The graph is a proof of the
> pipeline, not an interface yet — no search, no filters, no clustering. What is
> finished is the part that has to be right: the resolution, the coverage, and the
> two numbers that report them.

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

And it does the equivalent in every other language it reads: a Go import that names a
package directory, a Rust `use` path that ends in a struct rather than a module, a Sass
`@use "buttons"` whose file is `_buttons.scss`, a shell `source "$(dirname "$0")/lib.sh"`.

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

Point it at a directory that merely *contains* projects — `~/work`, a services folder, a
directory of clones — and you get one graph per project rather than one shape drawn out
of codebases that share no imports. A monorepo stays one project: its workspace packages
only resolve from the root.

```bash
kog view ~/other      # view a different project
kog scan              # stats to stdout, writes nothing
kog scan -o g.json    # write the graph as JSON
```

The server binds to `127.0.0.1` on an OS-assigned port and prints the URL before opening
anything, so `kog` over SSH still tells you where to look.

<br />

# Two numbers, and how to check them

A resolution rate answers *of the imports I read, how many resolved?* It says nothing
about the files never read at all — a tool that supports one language and silently drops
the other nine scores a perfect rate on a polyglot repository.

So every scan publishes both.

```
resolution rate = resolved / (internal − excluded)
source coverage = analysed / (analysed + unsupported)
```

Externals leave the resolution rate's denominator — `import react` has no file to point
at. So do **excluded** specifiers: those resolved to a real path outside the scanned set,
which is a policy decision, not a parser failure. Documentation, data and images leave
the coverage denominator for the same reason: a repository is not worse mapped for
containing a README.

Measured on two public repositories, at a commit you can check out yourself:

| Repository | Files seen | Analysed | Not read | Source coverage | Resolution rate |
| --- | ---: | ---: | ---: | ---: | ---: |
| [documenso/documenso](https://github.com/documenso/documenso) | 2,833 | 2,100 | 307 | **0.8725** | **0.9779** |
| [TanStack/query](https://github.com/TanStack/query) | 2,314 | 1,276 | 0 | **1.0000** | **0.9926** |

And a rate per language, because an aggregate lets a broken resolver hide behind a
majority language that works. On TanStack/query:

| Language | Files | Rate | | Language | Files | Rate |
| --- | ---: | ---: | --- | --- | ---: | ---: |
| typescript | 943 | 0.9948 | | html | 77 | 0.9444 |
| javascript | 138 | 0.9333 | | vue | 20 | 1.0000 |
| svelte | 77 | 1.0000 | | astro | 2 | 1.0000 |

That table is the argument for publishing per language rather than one number: writing
it turned up two real resolution bugs — `.d.ts` never probed, and SvelteKit's
`%sveltekit.assets%` placeholder treated as a path — that moved the repository-wide rate
by less than a point while breaking Vue outright.

Nothing is hidden. Every specifier that did not become an edge is listed in
`stats.diagnostics` with its file, its line, its language and **why** — `not_found`,
`gitignored`, `skipped_directory`, `outside_root`, `target_unreadable`. Every file no
extractor could read is named, by extension and language, in `stats.coverage`.

```bash
git clone --depth 1 https://github.com/documenso/documenso
kog scan documenso -o graph.json

# why did specifiers fail?
jq '[.projects[0].graph.stats.diagnostics[].reason] | group_by(.)
    | map({reason: .[0], count: length})' graph.json

# which languages were not read at all?
jq '.projects[0].graph.stats.coverage.extensions[]
    | select(.status == "unsupported_language")' graph.json
```

Full breakdown, with the raw output and every gap categorised, in
[`docs/measurements/`](docs/measurements/).

# <img src="./assets/icons/shield.svg" width="20" height="20"/> What it does not do

Stated plainly, because a tool that only advertises its strengths is not measuring anything.

- **Sixteen languages, not all of them.** TypeScript, JavaScript, Vue, Svelte, Astro, Go,
  Python, Rust, C, C++, Java, C#, Ruby, PHP, HTML, CSS/Sass/Less and shell. Everything
  else is a node in the graph and a named line in the coverage report — SQL, MDX, Swift,
  Kotlin, Scala, Elixir — but its own imports are not read. The report says which, and
  how many.
- **Files, not symbols.** Nodes are files, edges are static imports. Call graphs need a
  type checker to be right, and a wrong call graph is worse than a coarse import graph.
- **No dynamic `import()`**, no `require()`, no run-time path assembly. A shell `source
  "$CONFIG"` is reported as undecidable rather than broken.
- **A Go import is a package**, so one specifier becomes an edge to every file in that
  directory. That is what a file-level graph of Go means, not a bug.
- **The renderer is a prototype.** It proves 2,800 nodes render and stay interactive. It
  is not an interface.
- **Two repositories is not a corpus.** The rates above are two data points.

# Stack

Rust, [tree-sitter](https://tree-sitter.github.io) for parsing — one grammar per
language, one `Extractor` implementation each, dispatched by extension — and no LLM
anywhere in the pipeline: resolution is deterministic or it is not reported. The page is
[sigma.js](https://www.sigmajs.org) and [graphology](https://graphology.github.io) on WebGL,
built with Vite and embedded in the binary at compile time.

<br />

# Project status

v0.1 is complete and measured. [`ROADMAP.md`](ROADMAP.md) has the order: more languages,
then an MCP server so an agent can ask the graph questions instead of grepping, then a
desktop shell.

Contributions welcome — [`.github/CONTRIBUTING.md`](.github/CONTRIBUTING.md).

<br />

# License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE), at your option.
