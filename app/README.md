# mycelium — graph viewer

This is the rendering half of mycelium: a Vite + TypeScript page that loads
`/graph.json` and draws it with [sigma.js](https://www.sigma.js.org/) —
nodes are source files, edges are static imports, laid out with ForceAtlas2
and coloured by top-level directory. See `docs/design/v0-design.md` at the
repository root for the full design.

`app/src/main.tsx` is plain DOM/TypeScript; there is no framework in this
page (no React, despite the Vite template this was scaffolded from).

**This page ships embedded in the `mycelium` binary.** `bun run dev` below
is the *contributor* workflow for iterating on the page itself — it is not
how anyone using `mycelium` sees a graph. The end-user path is `mycelium`
(equivalently `mycelium view <dir>`): `app/dist` is embedded into the CLI at
compile time via `rust-embed` (`crates/mycelium-cli/build.rs` and
`src/server.rs`), and at runtime the CLI scans the requested project and
serves that page over an in-memory `tiny_http` server — no Vite, no `bun`,
and no `public/graph.json` involved. The live scan always wins over
whatever `graph.json` happened to be sitting in `public/` when `app/dist`
was built (`src/server.rs` intercepts that route before falling back to the
embedded files), but building with a stale `graph.json` still bakes it,
uselessly, into the binary — clear `public/graph*.json` before `bun run
build` if you want a lean one.

## Iterating on the page

The dev server needs a `graph.json` to load, and `public/graph*.json` is
**not committed** (see `.gitignore`) — it is scan output specific to
whatever codebase you point the CLI at. A fresh clone has none, so
`bun run dev` alone will show `graph.json: 404` in the page and in the
browser console. Generate one first, from the repository root:

```bash
export PATH="$HOME/.cargo/bin:$PATH"   # if ~/.cargo/bin isn't already on PATH
cargo build --release
./target/release/mycelium scan <path-to-a-typescript-project> -o app/public/graph.json
```

Then, from this directory:

```bash
bun install
bun run dev
```

Before committing a change to this page, run `bun run build` (or
`just build` from the repository root) and rebuild the CLI so `app/dist`
gets re-embedded — a stale `dist` silently serves the old page even after
`app/src` changes. `crates/mycelium-cli/build.rs` fails loudly if
`app/dist` doesn't exist at all, but it can't tell a stale build from a
fresh one; only a rebuild does that.

## Available scripts

- `bun run dev` — start the Vite dev server.
- `bun run build` — type-check (`tsc -b`) and produce a production build in
  `dist/`, which `cargo build` then embeds into the `mycelium` binary
  wholesale, including whatever `public/graph*.json` happens to exist at
  build time (see above).
- `bun run preview` — serve the `dist/` build locally.
- `bun run lint` — run oxlint.
