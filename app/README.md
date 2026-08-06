# mycelium — graph viewer

This is the rendering half of mycelium: a Vite + TypeScript page that loads
`public/graph.json` and draws it with [sigma.js](https://www.sigma.js.org/) —
nodes are source files, edges are static imports, laid out with ForceAtlas2
and coloured by top-level directory. See `docs/design/v0-design.md` at the
repository root for the full design.

`app/src/main.tsx` is plain DOM/TypeScript; there is no framework in this
page (no React, despite the Vite template this was scaffolded from).

## Producing the graph

The page reads `public/graph.json`, which is **not committed** (see
`.gitignore`) — it is a scan output specific to whatever codebase you point
the CLI at. A fresh clone has no graph, so `bun run dev` alone will show
`graph.json: 404` in the page and in the browser console. Generate one first,
from the repository root:

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

## Available scripts

- `bun run dev` — start the Vite dev server.
- `bun run build` — type-check (`tsc -b`) and produce a production build in
  `dist/` (which embeds whatever `public/graph.json` exists at build time).
- `bun run preview` — serve the `dist/` build locally.
- `bun run lint` — run oxlint.
