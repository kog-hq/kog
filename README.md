<img src="assets/logo.svg" alt="" width="72" align="left" hspace="12" vspace="4">

# KOG

**K**nowledge **O**rchestration **G**raph

<br clear="left">

Turns a codebase into a file/import graph. One binary, zero dependencies — no
JS toolchain, no repo checkout, no server to stand up by hand.

```
cd ~/your-project
kog
```

That's it. `kog` scans the current directory, holds the graph in
memory, serves it from a page embedded in the binary, and opens your
browser. Nodes are source files, edges are static imports, laid out with
ForceAtlas2 and coloured by top-level directory.

TypeScript/TSX is the only language supported in v0 — see
[`docs/design/v0-design.md`](docs/design/v0-design.md) for why, and what a
language has to prove to be added.

## Install

```
git clone https://github.com/bstcoc/kog
cd kog
just install
```

This builds the page, then the CLI that embeds it, then installs `kog`
to `~/.cargo/bin`. No `just`? See [`justfile`](justfile) for the two
commands it runs.

## Usage

```
kog                 # scan `.`, serve it, open a browser — the default
kog view ~/other    # explicit form: view a different project
kog scan            # stats to stdout, writes nothing
kog scan -o g.json  # write the graph as JSON instead of serving it
```

`view` never writes a file — nothing is left behind in the project you
pointed it at. `scan` is the non-visual path: pipe its output, write it to
disk with `-o`, or just read the numbers it prints (files parsed, edges
resolved, resolution rate — see the design doc §5 for the full shape).

The server binds to `127.0.0.1` on an OS-assigned free port and prints the
URL before opening the browser, so `kog` over SSH, or with no browser
available, still tells you where to look.

## Why this exists

The closest competitor to KOG is Python, needs a virtualenv, and its
issue tracker is full of install pain. "One binary, zero dependencies" is
the whole bet — a feature that requires cloning the renderer and running
`bun run dev` to see anything would quietly concede that bet. `kog`
alone is the measure of whether the bet is being kept.

## Development

This project is pre-release; see
[`.github/CONTRIBUTING.md`](.github/CONTRIBUTING.md) for the current state
and how to contribute. The design rationale — what's in v0, what's
deliberately out, and the measurements behind each call — lives in
[`docs/design/v0-design.md`](docs/design/v0-design.md).

```
just build    # build the page, then the release binary
just test     # cargo test --workspace
just lint     # cargo fmt --check + cargo clippy -D warnings
just dev      # Vite dev server for app/, for iterating on the page itself
```

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE), at
your option.
