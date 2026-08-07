/**
 * THROWAWAY SPIKE — delete before merging.
 *
 * Runs d3-force live on the built graph so the bloom, the drag and the cooling
 * can be watched rather than argued about. Sigma is already bound to the
 * graphology instance, so writing positions back on every tick is all the
 * wiring the picture needs.
 *
 * Off unless the page is opened with `?layout=d3`, and compiled out of any
 * production build by `import.meta.env.DEV`.
 */

import type Graph from "graphology";
import {
  forceCollide,
  forceLink,
  forceManyBody,
  forceSimulation,
  forceX,
  forceY,
  type Simulation,
  type SimulationNodeDatum,
} from "d3-force";

type Node = SimulationNodeDatum & { id: string; deg: number; size: number };
type Link = { source: string; target: string };

/** Every dial, readable from the query string so a sweep costs no rebuild. */
function dials(params: URLSearchParams) {
  const num = (key: string, fallback: number) => {
    const raw = params.get(key);
    const value = raw === null ? NaN : Number(raw);
    return Number.isFinite(value) ? value : fallback;
  };
  return {
    /** Spring rest length. Sets the scale of the whole picture. */
    link: num("link", 30),
    /** Spring stiffness. `-1` leaves d3's own 1/min(deg) weighting alone. */
    linkStrength: num("linkStrength", -1),
    /** Flat part of the repulsion. */
    repel: num("repel", 150),
    /**
     * Repulsion added per unit of degree.
     *
     * FA2 repels by `(deg(a)+1)(deg(b)+1)`, so hubs claim space; d3's
     * `manyBody` is uniform unless told otherwise. Measured at 25 — this is
     * what loosens clusters, and it leaves degree-0 files alone.
     */
    perDegree: num("perDegree", 25),
    /**
     * Pull toward the origin — Obsidian's "center force".
     *
     * Ten times FA2's gravity, deliberately. FA2 repels as 1/d and fights a
     * central pull all the way out; d3 repels as 1/d², so at range there is
     * almost nothing to fight and unconnected files drift off without this.
     */
    centre: num("centre", 0.25),
    /** Anti-overlap margin on top of each node's drawn radius. */
    collide: num("collide", 2),
    /** How fast it cools. d3's default ~0.0228 settles over five seconds. */
    alphaDecay: num("alphaDecay", 0.055),
    /** Friction. Lower is bouncier, higher is more syrupy. */
    velocityDecay: num("velocityDecay", 0.4),
    /** How hard a drag reheats the solver. */
    dragAlpha: num("dragAlpha", 0.3),
    /** How much heat a release buys, so the springs can pull the node home. */
    releaseAlpha: num("releaseAlpha", 0.45),
    /** Multiplier on the Louvain seed's radius. Small starts bloom. */
    seed: num("seed", 0.35),
  };
}

const params =
  typeof window === "undefined"
    ? new URLSearchParams()
    : new URLSearchParams(window.location.search);

/** Whether the spike is driving the layout at all. */
export const spikeOn =
  import.meta.env.DEV && params.get("layout") === "d3";

let simulation: Simulation<Node, Link> | null = null;
let byId = new Map<string, Node>();
let grabbed: Node | null = null;
const freezeHandlers = new Set<() => void>();

/** Called once the solver has cooled and stopped. */
export function onSpikeFreeze(handler: () => void): () => void {
  freezeHandlers.add(handler);
  return () => freezeHandlers.delete(handler);
}

/**
 * Start the live solver on the graph that is actually being drawn.
 *
 * Called from the canvas effect, never from `buildGraph`. `buildGraph` is a
 * `useMemo` factory: React may call it more than once per value it keeps, so a
 * solver started there can end up ticking against a graph React discarded —
 * the ids still match, the positions go nowhere, and dragging looks dead.
 *
 * Returns a stopper, so the solver's life is exactly the renderer's life.
 */
export function startSpike(graph: Graph): () => void {
  if (!spikeOn) return () => {};

  simulation?.stop();

  const d = dials(params);
  const started = performance.now();

  const nodes: Node[] = graph.mapNodes((node, attributes) => ({
    id: node,
    deg: graph.degree(node),
    size: attributes.size as number,
    // The Louvain seed, kept and tightened: it is the one thing Obsidian has
    // no equivalent for, and a compact start is what makes a bloom a bloom.
    x: (attributes.x as number) * d.seed,
    y: (attributes.y as number) * d.seed,
  }));
  byId = new Map(nodes.map((node) => [node.id, node]));
  const links: Link[] = graph.mapEdges((_e, _a, source, target) => ({
    source,
    target,
  }));

  const springs = forceLink<Node, Link>(links)
    .id((node) => node.id)
    .distance(d.link);
  if (d.linkStrength >= 0) springs.strength(d.linkStrength);

  let ticks = 0;
  simulation = forceSimulation(nodes)
    .force("link", springs)
    .force(
      "charge",
      forceManyBody<Node>().strength((n) => -(d.repel + d.perDegree * n.deg)),
    )
    .force("x", forceX(0).strength(d.centre))
    .force("y", forceY(0).strength(d.centre))
    .force(
      "collide",
      forceCollide<Node>().radius((n) => n.size + d.collide),
    )
    .alphaDecay(d.alphaDecay)
    .velocityDecay(d.velocityDecay)
    .on("tick", () => {
      ticks += 1;
      // One event for the whole graph rather than two per node. The hint tells
      // sigma which attributes moved so it can skip the rest.
      graph.updateEachNodeAttributes(
        (node, attributes) => {
          const point = byId.get(node);
          if (!point) return attributes;
          return { ...attributes, x: point.x ?? 0, y: point.y ?? 0 };
        },
        { attributes: ["x", "y"] },
      );
    })
    .on("end", () => {
      console.info("[spike-d3] settled", {
        dials: d,
        nodes: nodes.length,
        links: links.length,
        ticks,
        ms: Math.round(performance.now() - started),
      });
      for (const handler of freezeHandlers) handler();
    });

  // `forceSimulation` starts its timer on construction, and React's strict
  // mode mounts an effect, tears it down, and mounts it again. A tick landing
  // in that gap makes sigma schedule a repaint for a renderer that is about to
  // be killed, and the queued repaint then throws on the dead renderer's node
  // programs. Holding the first tick until the effect has survived one frame
  // is what makes the double-mount harmless.
  simulation.stop();
  const mine = simulation;
  let waking = 0;

  if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
    // Reduced motion gets the layout without the journey.
    while (mine.alpha() > mine.alphaMin()) mine.tick();
    mine.tick(0);
  } else {
    waking = requestAnimationFrame(() => mine.restart());
  }

  return () => {
    cancelAnimationFrame(waking);
    mine.stop();
    if (simulation === mine) {
      simulation = null;
      grabbed = null;
    }
  };
}

/** Pin a node to the cursor and reheat the solver. */
export function spikeGrab(id: string): void {
  if (!spikeOn || !simulation) return;
  const node = byId.get(id);
  if (!node) return;
  grabbed = node;
  node.fx = node.x;
  node.fy = node.y;
  simulation.alphaTarget(dials(params).dragAlpha).restart();
}

export function spikeMove(x: number, y: number): void {
  if (!grabbed) return;
  grabbed.fx = x;
  grabbed.fy = y;
}

/**
 * Let go, and give the springs enough heat to pull the node home.
 *
 * Clearing the pin and dropping alphaTarget to 0 is not enough on its own:
 * the solver is already cold by the end of a drag, so the node simply stayed
 * wherever it was dropped and the graph kept the dent. Re-raising alpha buys
 * roughly a hundred ticks — enough for the neighbourhood to close back over
 * it, which is what makes a tug a question rather than an edit.
 */
export function spikeRelease(): void {
  if (!spikeOn || !simulation || !grabbed) return;
  grabbed.fx = null;
  grabbed.fy = null;
  grabbed = null;
  simulation
    .alphaTarget(0)
    .alpha(Math.max(simulation.alpha(), dials(params).releaseAlpha))
    .restart();
}
