# Design — a living layout

The graph is laid out once and then frozen. Dragging a node moves that node and
nothing else: the edges attached to it stretch, its neighbours do not notice.
This document replaces that with a force simulation that runs, responds, and
stops.

The target is explicit: **the graph view in Obsidian**. Not "something like it"
— the same model, the same four dials, the same behaviour on release. Obsidian's
graph was reached from this repository's own Obsidian vault export, which is why
the comparison is available at all.

## What is being discarded, and the risk accepted

ForceAtlas2 goes. With it go `gravity: 0.02`, the `scalingRatio` ladder,
`outboundAttractionDistribution`, `slowDown: 4`, and the
`graphology-layout-forceatlas2` dependency. `parkIsolated()` goes too.

Those constants were not guesses. They are what turned a 3,000-edge disc into a
readable map, and `parkIsolated` exists because 151 unconnected files on
`acme-saas` tripled the area the camera had to cover. **Both problems are
being reopened deliberately.** The trade is that a static map, however readable,
cannot answer "what moves with this?" — and that question is worth a layout
regression risk, provided the risk is named rather than discovered.

Named, then: after this change, `acme-saas` may be less readable than it is
today. The tuning session in §9 is where that gets settled, and the acceptance
bar in §11 is what it has to clear.

**That risk was tested before this document was finished, and it did not
materialise — see §0.**

One thing is kept: **the Louvain seed**. Obsidian has no communities; this
project does. Starting from clusters that already hold together gives repulsion
clean work to do — pushing groups apart — instead of impossible work: untangling
them. It is the one place where KOG can beat Obsidian on Obsidian's own ground,
and the code already exists.

## 0. What the spike measured

Before writing the module, a throwaway (`app/src/graph/spike-d3.ts`, behind
`?layout=d3` in a dev build) laid `acme-saas` out with d3-force and nothing
else changed, so the two layouts could be photographed on the same scan. The
solver ran headlessly to freeze: the question was the *map*, not the animation.

**d3 came out better than ForceAtlas2, not worse.** FA2 produces one mass with
colour regions — `apps/backend` and `apps/frontend` occupy the same space and
are told apart only by hue. d3 separates them into two lobes with the satellite
communities (`backend/scripts`, `backend/src`, `shared-types`) as distinct
islands. The hairball this document was written to fear is what FA2 was already
producing.

It took tuning to get there. At the dials this document first proposed, clusters
came out as dense slabs and the unconnected files sprawled across more area than
the graph itself. Three findings:

| Dial | First guess | Measured | Why |
| --- | --- | --- | --- |
| `perDegree` | 8 | **25** | degree-weighted repulsion is what loosens clusters, and it does so without scattering isolated files, whose degree is 0 |
| `centre` | 0.02 | **0.25** | the isolate halo needs it, and the clusters are not crushed by it |
| `repel` | 200 | **150** | `perDegree` carries the load now |

The middle row deserves a warning, because it contradicts a lesson recorded in
`NEXT-AGENT.md`: *"do not raise gravity thinking it will tighten things."* That
was true of FA2, whose repulsion falls off as 1/d and therefore fights a central
pull all the way out. d3's `manyBody` falls off as 1/d², so at range there is
almost nothing to fight, and a centre force **ten times FA2's gravity** is what
stops unconnected files from drifting to infinity. The old lesson does not
transfer. Anyone who "fixes" `centre` back down to 0.02 will get the sprawl back.

Measured cost: **864 nodes, 3,121 edges, 300 ticks in ~410 ms** — 1.37 ms per
tick for the solver alone. A 60 fps frame is 16.7 ms, so the solver is 8% of the
budget on this graph. The unknown is the sigma refresh sitting on top of it,
which is what §7 is about.

## 1. The force model

Obsidian exposes four dials. Their d3 equivalents:

| Obsidian | d3 | Role |
| --- | --- | --- |
| Center force | `forceX(0)` + `forceY(0)`, weak | holds the whole together, stops drift |
| Repel force | `forceManyBody()`, negative, Barnes-Hut, **weighted by degree** | **the core** — this is what separates everything |
| Link force | `forceLink().strength()` | spring stiffness |
| Link distance | `forceLink().distance()` | spring rest length |

Plus two that Obsidian does not expose but that decide how the thing feels:
`velocityDecay` (friction — the difference between elastic and syrupy) and
`alphaDecay` (cooling).

`forceX(0)`/`forceY(0)` rather than `forceCenter`: `forceCenter` recentres by
translating the whole system, with no adjustable strength, whereas Obsidian's
"center force" slider is a dosable attraction. `forceCenter` would also fight
the camera rather than the layout.

Repulsion is `-(repel + perDegree × degree)`, not a constant. FA2 repels by
`(deg(a)+1)(deg(b)+1)`, so hubs claim space and clusters breathe; d3's
`manyBody` is uniform unless told otherwise, and uniform is what produced the
dense slabs in §0. This term is not a refinement, it is the difference between
readable and not.

`forceCollide` stays in the model. It was going to be optional until the node
sizes were looked at: `sizeFor` is `2.2 + √degree × 1.55`, so a degree-50 node
draws at 13 px against 2.2 for an unconnected one. Without anti-overlap, small
nodes vanish *inside* large ones — and "nothing disappears silently" is rule 2
of this project, which does not stop applying because the disappearance is
geometric.

## 2. The load sequence

1. **Seed**, tightened. The Louvain golden-angle spiral stays, but at a much
   smaller radius than today. The current spiral spreads wide because FA2 was
   never going to move things far; here the start has to be a compact cluster or
   the bloom does not read as a bloom.
2. **Run visibly.** Alpha decays, the graph unfolds and settles in about two
   seconds.
3. **On freeze** (`alpha < alphaMin`, which d3 detects and stops on):
   - one **full `renderer.refresh()`** — this is what puts the spatial index and
     the label grid back in step with reality. Without it, hover and click point
     at nodes where they were a second ago: the same trap already recorded for
     `skipIndexation` and the label grid, in a new place.
   - one `frame()` to fit what was actually drawn.

**The camera does not move during the bloom.** The alternative was to run the
solver headlessly first to learn the final extent and frame it up front — but
300 ticks over 2,800 nodes is more than a second of frozen page before anything
appears, which is a worse problem than the one it solves. The camera holds still
at a fixed initial ratio, the graph grows into it, and one `frame()` at freeze
corrects whatever overflowed. If that overflow is ugly on `acme-saas`, it is
a tuning item, not a redesign.

## 3. Module boundary

New file `app/src/graph/physics.ts`, importing **neither React nor sigma** —
only `d3-force` and a graphology graph. That is what makes it testable without a
canvas, and it keeps `graph-canvas.tsx` (already 672 lines) from absorbing a
second responsibility.

| File | After |
| --- | --- |
| `physics.ts` | the solver, the id↔d3-node bridge, `PHYSICS`, the dev handle |
| `graph-canvas.tsx` | wires drag events to it, reacts to freeze |
| `build.ts` | loses `forceAtlas2` and `parkIsolated`, keeps a tightened `seed()` |

Dependency added: `d3-force` alone (~10 kB), not `d3`. Bundled, no CDN — the
page still never touches the network.

The bridge is the part worth stating. d3 wants its own objects
(`{x, y, vx, vy, fx, fy}`); sigma reads graphology attributes. Positions are
written back every tick through **one `updateEachNodeAttributes` call with
`hints: {attributes: ['x', 'y']}`** — not 5,600 `setNodeAttribute` calls. That
is the difference between one event per frame and five thousand, and the hint is
what lets sigma skip work it does not need.

## 4. Waking and cooling

d3's own loop is already on `requestAnimationFrame`, and **it stops by itself**
when alpha falls below `alphaMin`. We listen to `"tick"` to paint and `"end"` to
freeze, and never drive the clock. "Vibrating forever" is therefore impossible
by construction rather than prevented by a guard.

| Event | Action |
| --- | --- |
| grab | `fx`/`fy` ← current position, `alphaTarget(dragAlpha).restart()` |
| move | `fx`/`fy` ← cursor, in graph coordinates |
| release | `fx`/`fy` ← `null`, `alphaTarget(0)` |
| freeze | full `refresh()`, then `frame()` |

A released node is **not** returned anywhere. It keeps its velocity, the
neighbourhood re-equilibrates around wherever it lands, and the layout is
allowed to drift. This is Obsidian's behaviour and it is the point: the map is
not a document being curated, it is an equilibrium being disturbed.

d3's defaults settle over ~300 ticks, about five seconds — too slow for the
target feel. `alphaDecay` near 0.056 gives roughly two seconds. It is a dial,
not a constant, until §9 says otherwise.

## 5. Isolated nodes

They float. `parkIsolated()` is deleted, and the 151 unconnected files on
`acme-saas` (17% of that graph) are subject only to repulsion and the centre
force, so they settle into a halo around the connected graph — exactly what
Obsidian does with orphans.

This knowingly reintroduces the camera cost that `parkIsolated` was written to
remove. No "hide orphans" toggle is added: rule 2 of this project is that
nothing disappears silently, and a file that imports nothing and is imported by
nothing is a finding, not noise. If the halo proves unreadable at scale, the
answer is revisited with a measurement, not with a checkbox.

Measured (§0): at `centre = 0.02` they sprawl across more area than the graph
itself, which is the failure `parkIsolated` was written for. At `centre = 0.25`
they pack into a compact cloud offset from the connected graph and cost almost
nothing. The halo is viable, but only because of that one dial — which is why
§0 spends a paragraph telling the next reader not to turn it back down.

## 6. Framing and edge culling, under motion

- **On mount**: no `frame()` on the seed — it would fit the compact cluster
  moments before the graph leaves it. Fixed initial ratio instead; the single
  `frame()` at freeze does the fitting.
- **The `selected` and `visible` effects**: both animate the camera over 320 ms
  toward positions that are, while hot, still moving. They are held while the
  solver is hot and replayed once on freeze. Two camera animations competing
  with a moving target is the failure this avoids.
- **Edge culling needs no work at all.** `inFrame()` reads `x`/`y` live and
  sigma re-runs the reducers on every refresh, so the answer is correct on every
  frame without touching it. `onScreen.current` is the *camera* box, not a node
  position, so it is right to recompute only when the camera moves.

  It is better than free: during the bloom much of the graph is outside the
  frame, so fewer edges are drawn at exactly the moment drawing is most
  expensive.

## 7. Scale

`acme-saas` (864 nodes) holds with room to spare — 1.37 ms per tick, 8% of a
frame, measured in §0. The doubt is `documenso`: **2,800 nodes, ~4,000 edges,
60 fps for two seconds of bloom.** Per tick that is a Barnes-Hut
pass, a link pass, the batched write-back, and a sigma refresh running the node
reducer 2,800 times and the edge reducer 4,000 times.

Two levers before a worker is considered:

1. the batched write-back of §3
2. paint every second tick above ~1,500 nodes — the solver still advances at
   full rate, only the painting halves

One suspect to check: `zIndex: true` makes sigma sort nodes on every refresh.
Free on a frozen graph, not free at 60 fps.

**Both repositories get measured.** Tuning the physics on `acme-saas` alone
would repeat the mistake this project already documented for Go — a number that
has never met a large input is not a number.

## 8. Reduced motion

`prefers-reduced-motion: reduce` runs the solver to freeze without painting,
then paints once. No animation, but a laid-out graph. On `documenso` that blocks
the page for roughly a second, which is the right trade for someone who asked
for no movement: the alternative is either motion they rejected or no layout at
all.

## 9. The tuning session

`PHYSICS` lives as one exported object in `physics.ts`. Under
`import.meta.env.DEV` only, `window.__kogPhysics` exposes it plus a `restart()`,
so values can be swept from the console and each attempt is a command that can
be repeated. Vite drops the handle from the production build; only the chosen
constants ship.

Order: sweep on `acme-saas` for feel, in both themes, watching the console;
then `documenso` for frame rate; then the winners go in hard and the handle is
removed from the reasoning, not just from the build.

## 10. Tests

The front has no test harness — `just test` is `cargo test --workspace`, and all
237 tests are Rust. `physics.ts` is the one front module that can be tested
without a canvas, so vitest is added: a dev dependency, a script in `app/`, and
a `just test-app` recipe.

- the solver stops on its own
- grabbing a node pins it to the given coordinates
- releasing a node clears its pin and lets alpha decay to `alphaMin`
- a graph with no edges and nothing grabbed does not move once frozen
- every member of a community is seeded inside that community's disc

"The solver stops" is exactly the kind of claim that should not be made without
a command and its output.

## 11. Acceptance

- `acme-saas` (864 nodes, 151 isolated): blooms and settles in ≤ 3 s, and the
  settled graph is at least as readable as today's — clusters separated, not one
  disc. Screenshots of both, both themes, side by side.
- `documenso` (2,800 nodes): blooms without dropping below 30 fps, settles, and
  the page stays responsive throughout.
- Dragging a node moves its neighbourhood; releasing settles it; the solver
  reaches `end` and the canvas goes idle. Verified in a real browser, both
  themes, console clean.
- `just lint`, `just test`, `just test-app` all green.
- A real scan of `~/acme` (9 projects, 2,179 files) before committing.

## 12. Open risks

1. ~~**Layout regression.**~~ Closed by §0: d3 reads better than FA2 on
   `acme-saas`. Still to confirm on `documenso`.
2. ~~**The orphan halo** may cost more camera area than `parkIsolated` saved.~~
   Closed by §0, conditional on `centre = 0.25`.
3. **Frame rate on `documenso`** may need the worker that §7 holds in reserve.
   The solver is cheap; the sigma refresh is not measured yet.
4. **Repeated drags drift the layout** — accepted, by design, and the reason
   there is no persisted position and no claim that the picture is canonical.
5. **Louvain is not deterministic** — see §13. It is not caused by this work,
   but this work makes it much more visible.

## 13. Found on the way — Louvain reshuffles on every load

`communities.ts` calls `louvain(graph, { resolution: 1 })` with no `rng`, so
the library falls back to `Math.random`. Two loads of the *same* scan produce
different partitions: across four screenshots taken minutes apart,
`apps/frontend` was measured at 192, 192, 255 and 186 files, and
`apps/frontend/src` at 145, 145, 86 and 109.

That is already wrong today — the community list in the rail and the colour of
every node change between reloads of a scan whose numbers are supposed to be
auditable. **This work makes it worse**, because the Louvain partition seeds the
layout: the same repository would open to a visibly different map every time.

It is out of scope here and belongs in its own commit, but it blocks §11 —
"screenshots side by side" cannot compare two layouts if the partition moved
underneath them. The fix is one argument: pass a seeded PRNG as `rng`.
