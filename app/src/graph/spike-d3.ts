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
  forceRadial,
  forceSimulation,
  forceX,
  forceY,
  type Simulation,
  type SimulationNodeDatum,
} from "d3-force";

type Node = SimulationNodeDatum & {
  id: string;
  deg: number;
  size: number;
  /** Where this node settled. Null until the first freeze — see `adopt`. */
  hx: number | null;
  hy: number | null;
};
type Link = { source: string; target: string };

/** Every dial, readable from the query string so a sweep costs no rebuild. */
function dials(params: URLSearchParams) {
  const num = (key: string, fallback: number) => {
    const raw = params.get(key);
    const value = raw === null ? NaN : Number(raw);
    return Number.isFinite(value) ? value : fallback;
  };
  return {
    /**
     * Spring rest length. Sets the scale of the whole picture.
     *
     * Obsidian publishes `linkDistance: 198` against our 30, and that single
     * number is most of why their clusters have white space between them and
     * ours touch. Their units are not ours, but the direction is not in doubt.
     */
    link: num("link", 110),
    /** Spring stiffness. `-1` leaves d3's own 1/min(deg) weighting alone. */
    linkStrength: num("linkStrength", -1),
    /** Flat part of the repulsion. */
    repel: num("repel", 260),
    /**
     * Repulsion added per unit of degree.
     *
     * FA2 repels by `(deg(a)+1)(deg(b)+1)`, so hubs claim space; d3's
     * `manyBody` is uniform unless told otherwise. This is what loosens
     * clusters, and it leaves degree-0 files alone.
     */
    perDegree: num("perDegree", 22),
    /** Pull toward the origin — Obsidian's "center force", published at 0.48. */
    centre: num("centre", 0.18),
    /** Anti-overlap margin on top of each node's drawn radius. */
    collide: num("collide", 3),
    /**
     * The radius unconnected files are held at, and how hard.
     *
     * This is the ring Obsidian puts its orphans on. Without it they keep
     * whatever the seed gave them — a degree-0 node feels only the floor of
     * the repulsion, so nothing redistributes them and they stay a lump in
     * whichever corner they started. A radial force spreads them evenly at
     * one distance, which reads as a border rather than as debris.
     */
    ring: num("ring", 780),
    ringStrength: num("ringStrength", 0.6),
    /** How fast it cools. d3's default ~0.0228 settles over five seconds. */
    alphaDecay: num("alphaDecay", 0.03),
    /** Friction. Lower is bouncier, higher is more syrupy. */
    velocityDecay: num("velocityDecay", 0.4),
    /** How hard a drag reheats the solver. */
    dragAlpha: num("dragAlpha", 0.3),
    /** How much heat a release buys, so the springs can pull the node home. */
    releaseAlpha: num("releaseAlpha", 0.5),
    /**
     * How firmly a node is held at the place it settled.
     *
     * Zero is the pure Obsidian model: no home, and a released node stays
     * wherever physics leaves it. Above zero the map becomes canonical — a
     * tug deforms it and letting go restores it. The two are not in tension
     * because the home is not invented: it is the position d3 itself chose,
     * adopted at the moment the bloom finishes, when every force is already
     * satisfied and adding it moves nothing.
     */
    home: num("home", 0.22),
    /** Multiplier on the Louvain seed's radius. Small starts bloom. */
    seed: num("seed", 0.35),
  };
}

const params =
  typeof window === "undefined"
    ? new URLSearchParams()
    : new URLSearchParams(window.location.search);

/** Whether the spike is driving the layout at all. */
export const spikeOn = import.meta.env.DEV && params.get("layout") === "d3";

let simulation: Simulation<Node, Link> | null = null;
let byId = new Map<string, Node>();
let grabbed: Node | null = null;
let hot = false;
const freezeHandlers = new Set<(first: boolean) => void>();

/**
 * Whether the solver is still moving nodes.
 *
 * Read by the canvas on every repaint: culling edges by what is on screen is
 * correct on a still graph and a strobe on a moving one, because an edge
 * crossing the frame boundary appears and vanishes on alternate frames.
 */
export function spikeHot(): boolean {
  return hot;
}

/** Called once the solver has cooled and stopped. `first` on the initial one. */
export function onSpikeFreeze(handler: (first: boolean) => void): () => void {
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
    hx: null,
    hy: null,
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

  // Home is null until the first freeze, so before then these are the plain
  // centre force and after it they are anchors. One force object, two lives.
  const homeX = forceX<Node>((n) => n.hx ?? 0).strength((n) =>
    n.hx === null ? d.centre : d.home,
  );
  const homeY = forceY<Node>((n) => n.hy ?? 0).strength((n) =>
    n.hy === null ? d.centre : d.home,
  );

  let ticks = 0;
  let settled = false;
  hot = true;

  simulation = forceSimulation(nodes)
    .force("link", springs)
    .force(
      "charge",
      forceManyBody<Node>().strength((n) => -(d.repel + d.perDegree * n.deg)),
    )
    .force("x", homeX)
    .force("y", homeY)
    // Only the unconnected are held on the ring; everyone else gets strength
    // zero and the force costs them nothing.
    .force(
      "ring",
      forceRadial<Node>(d.ring, 0, 0).strength((n) =>
        n.deg === 0 ? d.ringStrength : 0,
      ),
    )
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
      hot = false;
      const first = !settled;
      if (first && d.home > 0) {
        // Adopt the settled layout as home. Every anchor is exactly satisfied
        // at this instant, so switching them on moves nothing — and from here
        // a tug is a question the map answers by springing back, rather than
        // an edit the map keeps.
        for (const node of nodes) {
          node.hx = node.x ?? 0;
          node.hy = node.y ?? 0;
        }
        settled = true;
      }
      console.info("[spike-d3] settled", {
        dials: d,
        nodes: nodes.length,
        links: links.length,
        ticks,
        ms: Math.round(performance.now() - started),
        first,
      });
      for (const handler of freezeHandlers) handler(first);
    });

  simulation.stop();
  const mine = simulation;
  let waking = 0;

  if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
    // Reduced motion gets the layout without the journey.
    while (mine.alpha() > mine.alphaMin()) mine.tick();
    mine.tick(0);
    hot = false;
  } else {
    // `forceSimulation` starts its timer on construction, and React's strict
    // mode mounts an effect, tears it down, and mounts it again. A tick landing
    // in that gap makes sigma schedule a repaint for a renderer about to be
    // killed, and the queued repaint throws on the dead renderer's programs.
    waking = requestAnimationFrame(() => mine.restart());
  }

  return () => {
    cancelAnimationFrame(waking);
    mine.stop();
    if (simulation === mine) {
      simulation = null;
      grabbed = null;
      hot = false;
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
  hot = true;
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
 * wherever it was dropped and the graph kept the dent.
 */
export function spikeRelease(): void {
  if (!spikeOn || !simulation || !grabbed) return;
  grabbed.fx = null;
  grabbed.fy = null;
  grabbed = null;
  hot = true;
  simulation
    .alphaTarget(0)
    .alpha(Math.max(simulation.alpha(), dials(params).releaseAlpha))
    .restart();
}
