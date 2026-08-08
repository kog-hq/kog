# Design — a living layout

The graph is laid out once and then frozen. Dragging a node moves that node and
nothing else: the edges attached to it stretch, its neighbours do not notice.
This document replaces that with a force simulation that runs, responds, and
stops.

The target is explicit: **the graph view in Obsidian**. Not "something like it"
— the same model, the same four dials, the same behaviour on release. Obsidian's
graph was reached from this repository's own Obsidian vault export, which is why
the comparison is available at all.

Obsidian's own account states the stack: **d3-force for the simulation, PixiJS
for the rendering**. So the solver chosen here is literally theirs; only the
renderer differs, and sigma is already WebGL. Their documented drag behaviour
also matches the choice made below — a released node is immediately re-subjected
to the active forces rather than left pinned.

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

### Three lifecycle rules, each learned by breaking it

The spike's first live version looked completely dead: dragging a node moved
nothing. All three causes are lifecycle, none are physics, and all three will
recur in the real module.

**1. The solver belongs to the renderer's effect, never to `buildGraph`.**
`buildGraph` is a `useMemo` factory, and React may call it more than once per
value it keeps. A solver started there ticks against a graph React discarded:
the node ids still match, so a grab appears to succeed, and every position is
written into an object nothing draws. `startPhysics(graph)` is called from the
canvas effect, on the graph that renderer is drawing, and returns a stopper so
the solver's life is exactly the renderer's life.

**2. The solver must not tick before the effect has survived a frame.**
`forceSimulation` starts its timer on construction, and strict mode mounts an
effect, tears it down, and mounts it again. A tick landing in that gap makes
sigma schedule a repaint for a renderer about to be killed, and the queued
repaint then throws on the dead renderer's node programs. Build the simulation
stopped and `restart()` it inside one `requestAnimationFrame`, cancelled by the
stopper.

**3. Every deferred callback in `graph-canvas.tsx` must re-check its renderer.**
The fade loop captures `sigma.current` and is keyed on `[focus]` alone, so a
graph change swaps the renderer underneath a loop still in flight; the camera
settle timeout has the same shape. **This is a pre-existing defect, not one this
work introduces** — but it is unobservable today, because with a frozen layout
those loops are almost never running. A live layout runs them constantly. The
guard is one line, `if (sigma.current !== renderer) return`, and it belongs in
every deferred callback in the file.

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

`alphaDecay` decides how many ticks the solver gets before it stops, and this
document was wrong to treat "settles in ~2 s" as free. **The bloom's speed and
the map's quality are the same dial**, pulling opposite ways:

| `alphaDecay` | ticks | at 60 fps | result |
| --- | --- | --- | --- |
| 0.055 | 122 | ~2.0 s | brisk bloom, **under-relaxed map** — clusters freeze as dense discs |
| 0.035 | 194 | ~3.2 s | compromise |
| 0.023 (d3 default) | 297 | ~4.9 s | the airy map measured in §0, slow bloom |

Measured, not projected: at 0.055 the spike froze after 123 ticks on a map that
300 ticks had made readable. The likely way out is a partial head start — ~100
ticks run headlessly before the first paint costs about 140 ms, not the second
that §2 rejects, and the visible bloom then covers the remainder. To settle in
§9.

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

## 14. Built — and what the guessing had cost

The spike is gone; `physics.ts` ships and ForceAtlas2, `parkIsolated` and
`graphology-layout-forceatlas2` are deleted. Three things turned out to matter
more than every dial this document had argued about, and none of them were
physics.

### Obsidian's constants, read rather than inferred

`§1` reasoned about Obsidian's model from its four sliders. The model is
readable directly: `/Applications/Obsidian.app/Contents/Resources/obsidian.asar`
unpacks to `app.js` and `sim.js`, and `sim.js` is d3-force — the same solver —
with a WebAssembly inner loop and a plain JS fallback. Its state is declared in
one line:

    var Q = 1, B = 1 - Math.pow(.001, 1/300), y = 0,
        c = .1, E = 1, v = 250, x = -1e3;
    // alpha, alphaDecay, alphaTarget, centre, linkStrength, linkDistance, repel

plus `forceManyBody().distanceMin(30)`, `forceCollide().radius(60).strength(.5)`,
and `t.x += t.vx *= .6`. The repel slider is **cubed** before it reaches the
worker (`repelStrength: e*e*e`), which is why a default of 10 means -1000.

Two of this document's own conclusions were wrong against that:

- **Degree-weighted repulsion was a mistake.** `§1` argued `perDegree` was "not
  a refinement, it is the difference between readable and not", reasoning from
  FA2. Obsidian runs *our* solver with flat repulsion and no degree term at
  all. The dense slabs of `§0` were never about degree.
- **`forceCollide().radius(60)` is the dial that makes a graph breathe**, and
  this document never mentioned it except as anti-overlap. A flat 60 against
  node radii of 3–11 means the minimum gap between two files has nothing to do
  with how big either is drawn. Ours was `size + 3` — anti-overlap and nothing
  more, which lets a cluster pack until its members touch.

The orphan ring is not a force. With repulsion at -1000 against a centre force
of 0.1, every degree-0 file settles where those balance — the same distance for
all of them — and they spread by repelling each other. A `forceRadial(780)` had
been added to impose it and produced a lopsided arc, because a guessed radius
is not an equilibrium. What the ring did need was an even *seed*: Louvain drops
every unconnected file into one community, so all 151 of `acme-saas` started
on one patch of the spiral and cooled before mutual repulsion could push them
the rest of the way round.

### Three sigma defaults, which were the actual complaints

| Setting | Default | What it was doing |
| --- | --- | --- |
| `autoRescale` | `true` | recomputed `nodeExtent = graphExtent(graph)` on every refresh and renormalised the whole picture onto it |
| `minEdgeThickness` | `1.7` | clamped every width `edgeInk` chose, so the density ladder moved nothing |
| `pickingDownSizingRatio` | `4` | hit-testing happens on a quarter-resolution buffer |

`autoRescale` is "dragging a node drags the world". The mapping is rebuilt from
the bounding box, so pulling one file outward grows the box and slides every
other node to compensate. Fixed with `autoRescale: false` and a `customBBox`
sized from √n, which also holds the drawn scale constant across repositories so
one label calibration works everywhere.

`minEdgeThickness` is "the lines are too coarse". The shader is
`max(normalLength / u_sizeRatio, minThickness)`. Walking `size` from 0.4 to
0.18 to thin a dense graph was clamped straight back to 1.7 px every time —
three settings that had never moved a pixel. Obsidian's line is one.

### The fade was never a duration

Obsidian's entire hover animation is two constants: a dim target of `0.2` and

    $Q = function(e, t, n) { return void 0===n && (n=.9), e*n + t*(1-n) }

applied to every alpha on every frame. There is no duration, no easing and no
start — a damped value chasing a target cannot be interrupted, because it was
never following a plan. Our 320 ms tween restarted from wherever it had reached
whenever the pointer moved, which is precisely what read as switching rather
than gliding. The 90 ms hover delay went with it: the strobing it was added to
prevent came from the effect being a hard switch, not from how often it fired.

**A second bug hid inside the same gesture.** The drag ended on `mouseleave` as
well as `mouseup`, so hauling a node toward the top of the screen — the exact
complaint — dropped the drag partway through *and re-enabled the camera
mid-gesture*, panning the view with the rest of the same movement. Sigma emits
`mousemovebody` so a drag can survive leaving the container. Leaving is not
letting go.

Names now follow Obsidian's `textAlpha = clamp(log2(scale) + 1 - fade, 0, 1)`,
so they are a function of zoom rather than a mode you set and live with. The
overview arrives with no text on it at all.

### Measured

`acme-saas`, 864 nodes / 3,121 edges, 1244×862, Chrome on a 120 Hz display:

| | |
| --- | --- |
| bloom, median frame | **8.3 ms** (120 fps) |
| bloom, 90th percentile | 9.0 ms |
| bloom, worst frame | 17.0 ms |
| frames over 33 ms | **0** |
| repaints while idle | **0** |
| camera drift while dragging a node off-canvas | **0** (`x` 0.510, `y` 0.504, `ratio` 0.909 — identical before, during and after) |
| labels drawn at camera ratio 0.9 / 0.45 / 0.22 | 0 / 0 / 24 |

`§11` also asks for `documenso` at 2,800 nodes. **That is still unmeasured** —
the fixture in `app/public/graph.json` tops out at `acme-saas`, and a number
that has never met a large input is not a number. The frame budget has 4× of
headroom on this machine, which is a reason to expect it to hold and not a
reason to claim it does.

`§13` also still stands: Louvain reshuffles on every load, and it is now very
visible, because the partition seeds the layout.

### The dev handle

`window.__kog` exposes `{ renderer, physics, forces }` under
`import.meta.env.DEV`, which `§9` asked for. It earned itself immediately: a
WebGL canvas has no DOM to assert against, so "did that drag move a node or the
camera?" is unanswerable from a screenshot. Both of the drag bugs above were
found by reading the camera state across a scripted gesture, after two earlier
attempts drew the wrong conclusion from a press that had simply missed.

## 15. Two corrections after the first pass

**Edge width was only half fixed.** Lowering `minEdgeThickness` made the number
in `edgeInk` real, but sigma still divides it by `sizeRatio`, which is
`√cameraRatio` — so a line that matched Obsidian at the overview was three
times too heavy by the time you were inside a cluster. Obsidian's width is
`lineSizeMult / scale` in graph units, which is *constant on screen at every
magnification*. The reducer now multiplies by `√ratio` to cancel sigma's law,
and a change of ratio repaints immediately rather than after the 90 ms pan
debounce, because a width that arrives late visibly snaps. Measured across a
22× range — camera ratio 1.8 to 0.08 — the drawn width holds at 0.85 px while
the reducer's own figure walks from 1.14 to 0.24.

That immediate repaint promptly broke lifecycle rule 3: `onCamera` had never
needed the `sigma.current !== renderer` guard, because everything it did was
deferred and the deferred callback carried it. Refreshing synchronously moved
the danger into the handler itself, and a camera animation still in flight when
the project changes reached a killed renderer. The camera listener was also
never removed on teardown, which is how a leaked one came to matter.

**The rail and the inspector had one surface between them.** Both were
`--card`, so they sat at the same depth, and the two want different things: a
panel floating over the canvas has to lift off it, while a rail bolted to the
window edge only has to be distinct from it. `--rail` is now its own token and
`--card` is free to be the elevated one.

*Which way* the rail steps was then guessed, and guessed wrong. Obsidian's
rule, read out of `app.css` rather than off a screenshot, is that the canvas
takes the **extreme** value of the theme and the sidebar moves toward mid grey:

    --background-primary:   var(--color-base-00)   // #1C1C1C  the canvas
    --background-secondary: var(--color-base-20)   // #282828  the sidebar

So in dark the sidebar is the *lighter* surface, not the darker one, and the
graph sits on the darkest thing on screen. In light it inverts, because there
the extreme is the pale end (`#ffffff` against `#f6f6f6`). One rule, two
directions — the stage is always the surface furthest from the reader.

Their greys are also **neutral**. A slate with blue in it looked like a design
choice and read as one; the graph sat in a cold room. Dark now uses Obsidian's
scale outright, down to `--graph-line: #3f3f3f`. Light keeps a cream
(`#fcfaf5`) rather than their `#ffffff` — the one deliberate divergence, because
white is the brightest thing a screen can do and on a page that is mostly
background it becomes the loudest element. Black is avoided for the mirror
reason: it gives a coloured dot nothing to compete with, so each one reads as a
light source instead of an object. `#1C1C1C` is not black.

## 16. The density ladder was spending contrast it no longer had to

Compared side by side with Obsidian at cluster zoom, our edges were too faint
against the background — in **both** themes, though it showed up first in the
light one:

| | background → line | step | of range |
| --- | --- | --- | --- |
| Obsidian dark | `#1c1c1c` → `#3f3f3f` | 35 | 13.7% |
| Obsidian light | `#ffffff` → `#dadada` | 37 | 14.5% |
| KOG dark, before | `#1c1c1c` → `#343434` | 24 | 9.4% |
| KOG light, before | `#fbf9f5` → `#e9e2d6` | 24.5 | 9.6% |

A third of the separation, gone. The cause was `edgeInk`'s density ladder:
`acme-saas` has 3,121 edges, which landed in the `> 3000` bucket and blended
the ink 30% back into the background.

That ladder was not wrong when it was written. Under the frozen ForceAtlas2
layout, three thousand strokes crossed the same few hundred pixels and did read
as fog. It is wrong now, and for a reason worth stating plainly: **the layout
fix removed the problem the colour fix existed for.** `linkDistance` at 250
against a collision radius of 60 gives the strokes room, so there is no fog
left to thin — and all the ladder still did was pay for a solved problem with
contrast. Obsidian varies neither colour nor width with density; neither do we
now. Both are flat, at their exact step. Measured after: light 37, dark 35.

The light ink is `#d9d5cd` — the same 37-point step as theirs, taken on cream,
with about half the warmth the surface ramp would give at that lightness. A
tinted plane reads as paper; a tinted hairline reads as a stain.

If a repository ten times denser than `acme-saas` fogs again, the lever
comes back with a measurement attached. It does not come back on a guess.
