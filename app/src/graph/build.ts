import Graph from "graphology";
import type { KogProject, NodeKind, ProjectIndex } from "@/lib/kog";
import { dirName } from "@/lib/kog";
import type { Communities } from "./communities";

/**
 * Turning a scan into something sigma can draw.
 *
 * This function seeds positions and stops. The layout itself is live and
 * belongs to `physics.ts`, which the canvas starts on the graph its renderer
 * is drawing. Nothing long-lived may begin here: this is a `useMemo` factory,
 * and React may call it more than once per value it keeps.
 *
 * Filtering, selecting and recolouring all run through sigma's reducers, which
 * repaint without moving a node.
 */

export type ColourBy = "community" | "language";

export type NodeAttributes = {
  label: string;
  size: number;
  color: string;
  borderColor: string;
  community: number;
  kind: NodeKind;
  lang: string;
  members: string[];
  x: number;
  y: number;
};

/**
 * How big a file is drawn, in screen pixels.
 *
 * Obsidian's curve — `clamp(3·√(weight+1), 8, 30)` — rescaled to our pixel
 * range. Two properties carry over, and both matter more than the exact
 * numbers:
 *
 * **A floor.** Our previous curve started at 2.2 px, so a file that imports
 * nothing and is imported by nothing drew as a speck that reads as dirt on the
 * screen rather than as a finding. Nothing here disappears quietly, and being
 * too small to see is a way of disappearing.
 *
 * **A ceiling.** The old curve had a 6× spread between the smallest and a
 * 232-dependent hub, so hubs became discs that swallowed their own
 * neighbourhoods. Obsidian's spread is 3.75×; this is 3.8×. Degree is already
 * carried by where a node sits and how many lines reach it — size only has to
 * rank it, not shout.
 */
function sizeFor(degree: number, kind: NodeKind): number {
  const base = Math.min(11, Math.max(2.9, 1.1 * Math.sqrt(degree + 1)));
  return kind === "asset" ? base * 0.75 : base;
}

export type BuildOptions = {
  /** Collapse every file into the folder that holds it. */
  groupByFolder: boolean;
};

/**
 * Seed each community on its own patch of the plane before relaxing.
 *
 * This is the single change that turns a 3,000-edge disc into a readable
 * map. Force-directed layout is a local optimiser: from a random start it
 * finds a minimum in which every cluster overlaps every other, and no number
 * of iterations gets it out. Started with the communities already apart, the
 * same solver spends its effort on shape instead of separation.
 *
 * It is also the one place KOG can beat Obsidian on Obsidian's own ground:
 * they run the same solver, and they have no communities to seed it with.
 *
 * The radii here are small against the solver's 250-unit springs, and that is
 * deliberate — the graph starts as a tight knot and blooms outward into its
 * real scale, which is what makes the opening read as an unfolding rather than
 * as a jolt.
 *
 * Communities are laid on a spiral by rank, biggest nearest the middle, so a
 * long tail of two-file groups packs tightly at the edge instead of each
 * claiming a slot on a huge ring — which is what happened when every
 * community, whatever its size, got an equal share of a circle: forty
 * one-file communities drew a vast dotted halo around a graph squeezed into
 * one corner.
 */
function seed(graph: Graph, communities: Communities): void {
  const centres = new Map<number, { x: number; y: number }>();
  let radius = 0;
  communities.list.forEach((community, rank) => {
    // Golden angle: consecutive ranks land on opposite sides, so neighbours
    // in size never end up as neighbours in space.
    const angle = rank * 2.399963;
    radius += 26 + 90 / (rank + 2);
    centres.set(community.id, { x: Math.cos(angle) * radius, y: Math.sin(angle) * radius });
  });

  const placed = new Map<number, number>();
  const unconnected: string[] = [];
  graph.forEachNode((node, attributes) => {
    if (graph.degree(node) === 0) {
      unconnected.push(node);
      return;
    }
    const community = attributes.community as number;
    const centre = centres.get(community) ?? { x: 0, y: 0 };
    const index = placed.get(community) ?? 0;
    placed.set(community, index + 1);
    // A phyllotactic spiral rather than a ring: it fills a disc evenly, so a
    // 200-file community does not start as a circle of overlapping dots.
    const angle = index * 2.399963;
    const spread = Math.sqrt(index) * 7;
    graph.setNodeAttribute(node, "x", centre.x + Math.cos(angle) * spread);
    graph.setNodeAttribute(node, "y", centre.y + Math.sin(angle) * spread);
  });

  // The files nothing points at, and that point at nothing, start spread
  // evenly around the outside — one circle, one file per equal slice of it.
  //
  // Where they end up is not decided here: repulsion against the centre force
  // settles every degree-0 file at the same distance, and that ring is a
  // property of the physics rather than of this seed. What the seed decides is
  // whether they arrive there *evenly*. Louvain has no use for a node with no
  // edges, so it drops all of them into one community, and seeding that
  // community like any other put all 151 unconnected files of `acme-saas`
  // on a single patch of the spiral. They then blew outward together and
  // settled as a lopsided arc across a third of the circle, because the only
  // thing that could have spread them the rest of the way round is their own
  // mutual repulsion — and by the time it had pushed them that far the solver
  // had cooled. Starting them apart costs nothing and the arc closes.
  const step = (Math.PI * 2) / Math.max(1, unconnected.length);
  const ring = radius * 1.2 + 40;
  unconnected.forEach((node, index) => {
    graph.setNodeAttribute(node, "x", Math.cos(index * step) * ring);
    graph.setNodeAttribute(node, "y", Math.sin(index * step) * ring);
  });
}

export function buildGraph(
  project: KogProject,
  index: ProjectIndex,
  communities: Communities,
  options: BuildOptions,
): Graph {
  const graph = new Graph({ type: "directed", multi: false });

  const add = (
    id: string,
    label: string,
    lang: string,
    kind: NodeKind,
    community: number,
    members: string[],
  ) => {
    graph.addNode(id, {
      label,
      size: 2,
      // Both are decided by the reducer on every frame, from the current
      // theme and colour mode. Nothing here can go stale.
      color: "#888888",
      borderColor: "#00000000",
      community,
      kind,
      lang,
      members,
      x: 0,
      y: 0,
    });
  };

  if (options.groupByFolder) {
    const members = new Map<string, string[]>();
    for (const node of project.graph.nodes) {
      const folder = dirName(node.id);
      const list = members.get(folder);
      if (list) list.push(node.id);
      else members.set(folder, [node.id]);
    }
    for (const [folder, files] of members) {
      const langs = new Map<string, number>();
      const groups = new Map<number, number>();
      let kind: NodeKind = "asset";
      for (const id of files) {
        const node = index.byId.get(id);
        if (!node) continue;
        langs.set(node.lang, (langs.get(node.lang) ?? 0) + 1);
        const community = communities.byNode.get(id) ?? 0;
        groups.set(community, (groups.get(community) ?? 0) + 1);
        if (node.kind === "source") kind = "source";
        else if (node.kind === "unread_source" && kind === "asset")
          kind = "unread_source";
      }
      const lang =
        [...langs.entries()].sort((a, b) => b[1] - a[1])[0]?.[0] ?? "";
      const community =
        [...groups.entries()].sort((a, b) => b[1] - a[1])[0]?.[0] ?? 0;
      add(folder, folder, lang, kind, community, files);
    }
    for (const edge of project.graph.edges) {
      const from = dirName(edge.source);
      const to = dirName(edge.target);
      if (from === to || !graph.hasNode(from) || !graph.hasNode(to)) continue;
      graph.mergeEdge(from, to);
    }
  } else {
    for (const node of project.graph.nodes) {
      add(
        node.id,
        index.label.get(node.id) ?? node.id,
        node.lang,
        node.kind,
        communities.byNode.get(node.id) ?? 0,
        [node.id],
      );
    }
    for (const edge of project.graph.edges) {
      if (graph.hasNode(edge.source) && graph.hasNode(edge.target)) {
        graph.addDirectedEdge(edge.source, edge.target);
      }
    }
  }

  graph.forEachNode((node, attributes) => {
    graph.setNodeAttribute(
      node,
      "size",
      sizeFor(graph.degree(node), attributes.kind as NodeKind),
    );
  });

  seed(graph, communities);

  return graph;
}
