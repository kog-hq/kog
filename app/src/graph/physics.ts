/**
 * The living layout: Obsidian's force model, run on the graph sigma is drawing.
 *
 * Every constant below was read out of Obsidian's own shipped simulation
 * (`obsidian.asar` → `sim.js`, which is d3-force with a WebAssembly inner
 * loop) rather than guessed from how the picture looks. They are quoted here
 * with the value they hold there, so a future change is a decision to diverge
 * and not an accident:
 *
 *     var Q = 1,                          // alpha
 *         B = 1 - Math.pow(.001, 1/300),  // alphaDecay
 *         y = 0,                          // alphaTarget
 *         c = .1,                         // centre strength
 *         E = 1,                          // link strength multiplier
 *         v = 250,                        // link distance
 *         x = -1e3;                       // repel strength
 *     …forceManyBody().strength(-1e3).distanceMin(30)
 *     …forceCollide().radius(60).strength(.5)
 *     …t.x += t.vx *= .6                  // velocityDecay 0.4
 *
 * This module imports neither React nor sigma: only `d3-force` and a
 * graphology graph. That is what keeps it testable without a canvas.
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

type Node = SimulationNodeDatum & { id: string };
type Link = { source: string; target: string };

export const PHYSICS = {
  /**
   * Spring rest length, and with it the scale of the whole picture.
   *
   * Obsidian's own default. Our previous layout used 30, and later 110, which
   * is most of why their clusters have white space between them and ours
   * touched.
   */
  linkDistance: 250,
  /** Spring stiffness, as a multiplier on d3's own 1/min(degree) weighting. */
  linkStrength: 1,
  /**
   * Repulsion. Flat, not weighted by degree.
   *
   * An earlier attempt here scaled repulsion with degree on the reasoning that
   * ForceAtlas2 does — but FA2 needs it because its repulsion falls off as
   * 1/d, and d3's falls off as 1/d². Obsidian runs the same solver we do and
   * uses a flat -1000, so the degree term was solving a problem this solver
   * does not have.
   */
  repel: -1000,
  /** Below this distance repulsion stops growing, so coincident nodes do not explode. */
  repelDistanceMin: 30,
  /** Pull toward the origin. Weak: the edges are what hold the graph together. */
  centre: 0.1,
  /**
   * Anti-overlap. **This is the dial that makes a graph breathe.**
   *
   * A flat 60, against node radii of 3 to 11 — so the minimum gap between any
   * two files is many times either one's size. Our previous value was
   * `node.size + 3`, which is anti-overlap and nothing more: it lets a cluster
   * pack until its members touch, and no amount of thinning the edges rescues
   * a picture whose nodes are already shoulder to shoulder. Obsidian's
   * clusters are legible because this number has nothing to do with how big a
   * node is drawn.
   */
  collideRadius: 60,
  collideStrength: 0.5,
  /** Friction. d3's default, and Obsidian's. */
  velocityDecay: 0.4,
  /** Cooling: 300 ticks from alpha 1 down to alphaMin. */
  alphaDecay: 1 - Math.pow(0.001, 1 / 300),
  /** How hot a drag holds the solver while the pointer is down. */
  dragAlpha: 0.3,
  /**
   * How far the pointer must travel before a press becomes a drag.
   *
   * Obsidian's threshold, squared, in graph units. Without it every click
   * nudges the node it lands on, and a graph you cannot click without
   * disturbing is a graph you stop clicking.
   */
  dragSlopSquared: 25,
} as const;

export type Physics = {
  /** Stop ticking and release everything. */
  stop(): void;
  /** Pin a node under the pointer and reheat the solver. */
  grab(id: string): void;
  /** Move the pinned node, in graph coordinates. */
  moveTo(x: number, y: number): void;
  /** Let go. The node keeps its velocity; nothing is returned anywhere. */
  release(): void;
  /** The node being dragged, if any — it highlights like a hovered one. */
  dragging(): string | null;
  /** Whether the solver is still moving nodes. */
  hot(): boolean;
  /** Called once the solver has cooled and stopped. */
  onSettle(handler: (first: boolean) => void): () => void;
};

/**
 * Start the solver on the graph that is actually being drawn.
 *
 * Called from the canvas effect, never from `buildGraph`. `buildGraph` is a
 * `useMemo` factory: React may call it more than once per value it keeps, so a
 * solver started there can end up ticking against a graph React discarded —
 * the ids still match, so a grab appears to succeed, and every position is
 * written into an object nothing draws.
 */
export function startPhysics(graph: Graph): Physics {
  const nodes: Node[] = graph.mapNodes((node, attributes) => ({
    id: node,
    // The Louvain seed, kept. Obsidian has no communities and starts from a
    // phyllotactic spiral; we know which files belong together before the
    // solver does, so repulsion gets the easy job — pushing groups apart —
    // instead of the impossible one, untangling them.
    x: attributes.x as number,
    y: attributes.y as number,
  }));
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const links: Link[] = graph.mapEdges((_edge, _attributes, source, target) => ({
    source,
    target,
  }));

  const settleHandlers = new Set<(first: boolean) => void>();
  let grabbed: Node | null = null;
  let hot = true;
  let settled = false;

  const springs = forceLink<Node, Link>(links)
    .id((node) => node.id)
    .distance(PHYSICS.linkDistance);
  // Multiply d3's own 1/min(degree) weighting rather than replacing it, which
  // is what Obsidian's link-force slider does.
  const defaultStrength = springs.strength();
  springs.strength((link, index, all) => {
    return PHYSICS.linkStrength * defaultStrength(link, index, all);
  });

  const simulation: Simulation<Node, Link> = forceSimulation(nodes)
    .force("link", springs)
    .force(
      "charge",
      forceManyBody<Node>()
        .strength(PHYSICS.repel)
        .distanceMin(PHYSICS.repelDistanceMin),
    )
    // `forceX`/`forceY` at the origin rather than `forceCenter`: the latter
    // recentres by translating the whole system, with no adjustable strength,
    // where Obsidian's "center force" is a dosable attraction.
    //
    // There is deliberately no "home" anchor here. An earlier version adopted
    // each node's settled position and pulled it back after a drag, which made
    // the map canonical — and made every release a fight between the anchors
    // and the springs, felt as the whole graph lurching. Obsidian has no such
    // force: a released node stays where physics leaves it, the neighbourhood
    // re-equilibrates around it, and the map is allowed to drift.
    .force("x", forceX<Node>(0).strength(PHYSICS.centre))
    .force("y", forceY<Node>(0).strength(PHYSICS.centre))
    .force(
      "collide",
      forceCollide<Node>(PHYSICS.collideRadius).strength(
        PHYSICS.collideStrength,
      ),
    )
    .alphaDecay(PHYSICS.alphaDecay)
    .velocityDecay(PHYSICS.velocityDecay)
    .on("tick", () => {
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
      settled = true;
      for (const handler of settleHandlers) handler(first);
    });

  // There is no explicit force parking the files nothing points at. With
  // repulsion at -1000 against a centre force of 0.1, a node with no edges
  // settles where those two balance — the same distance for all of them — and
  // they spread evenly around that circle by repelling each other. The ring of
  // orphans in Obsidian's graph is not drawn, it is what falls out of these two
  // numbers. An earlier `forceRadial(780)` here was an attempt to impose it,
  // and it produced a lopsided arc because the radius was a guess rather than
  // an equilibrium.

  simulation.stop();
  let waking = 0;

  if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
    // Reduced motion gets the layout without the journey.
    while (simulation.alpha() > simulation.alphaMin()) simulation.tick();
    simulation.tick(0);
    hot = false;
    // `tick()` does not fire "end", so the settle handlers are called by the
    // caller's first paint instead — see the canvas.
  } else {
    // `forceSimulation` starts its timer on construction, and React's strict
    // mode mounts an effect, tears it down, and mounts it again. A tick landing
    // in that gap makes sigma schedule a repaint for a renderer about to be
    // killed, and the queued repaint throws on the dead renderer's programs.
    waking = requestAnimationFrame(() => simulation.restart());
  }

  return {
    stop() {
      cancelAnimationFrame(waking);
      simulation.stop();
      grabbed = null;
      hot = false;
    },
    grab(id) {
      const node = byId.get(id);
      if (!node) return;
      grabbed = node;
      node.fx = node.x;
      node.fy = node.y;
      hot = true;
      simulation.alphaTarget(PHYSICS.dragAlpha).restart();
    },
    moveTo(x, y) {
      if (!grabbed) return;
      grabbed.fx = x;
      grabbed.fy = y;
    },
    release() {
      if (!grabbed) return;
      grabbed.fx = null;
      grabbed.fy = null;
      grabbed = null;
      // `alphaTarget(0)` and nothing else. An earlier version also kicked
      // alpha back up to 0.5 so the springs could "pull the node home", which
      // re-bloomed the entire graph on every release — the single loudest
      // source of "dragging makes it go haywire". The solver is still at
      // `dragAlpha` when the button comes up, and that is exactly the heat
      // needed to re-settle the neighbourhood and no more.
      simulation.alphaTarget(0);
    },
    dragging() {
      return grabbed?.id ?? null;
    },
    hot() {
      return hot;
    },
    onSettle(handler) {
      settleHandlers.add(handler);
      return () => settleHandlers.delete(handler);
    },
  };
}
