import Graph from "graphology";
import forceAtlas2 from "graphology-layout-forceatlas2";
import type { KogProject, NodeKind, ProjectIndex } from "@/lib/kog";
import { dirName } from "@/lib/kog";
import type { Communities } from "./communities";

/**
 * Turning a scan into something sigma can draw.
 *
 * The layout is computed once per view and never again: filtering, selecting
 * and recolouring all run through sigma's reducers, which repaint without
 * moving a node. A graph that rearranges itself every time you tick a
 * checkbox is unreadable, however fast it is.
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

function sizeFor(degree: number, kind: NodeKind): number {
  const base = 2.2 + Math.sqrt(degree) * 1.55;
  return kind === "asset" ? base * 0.6 : base;
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
  graph.forEachNode((node, attributes) => {
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
}

/**
 * Park the files nothing points at, and that point at nothing.
 *
 * A force layout has no opinion about a node with no edges: it drifts
 * wherever the last repulsion pushed it, and a few hundred of them scatter
 * across the whole canvas and triple the area the camera has to cover to fit
 * the graph. They are real files and they stay on screen — packed into a
 * tidy block below the graph, where they read as what they are: a shelf of
 * things that connect to nothing.
 */
function parkIsolated(graph: Graph): void {
  const isolated = graph.filterNodes((node) => graph.degree(node) === 0);
  if (isolated.length === 0) return;

  let minX = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  graph.forEachNode((node, attributes) => {
    if (graph.degree(node) === 0) return;
    minX = Math.min(minX, attributes.x as number);
    maxX = Math.max(maxX, attributes.x as number);
    maxY = Math.max(maxY, attributes.y as number);
  });
  if (!Number.isFinite(minX)) {
    minX = -200;
    maxX = 200;
    maxY = 0;
  }

  const step = 13;
  // A block about as wide as it is tall, never wider than the graph above it.
  //
  // It used to fill the full width in a single row, which reads fine until
  // you filter: ask for a language whose files import nothing — 56 `.sql`
  // files, say — and the camera has to zoom out to the width of the entire
  // graph to hold a strip one node high, so the answer arrives as a dotted
  // line too small to see. A compact block frames at a useful zoom.
  const widest = Math.max(4, Math.floor(Math.max(maxX - minX, 200) / step));
  const perRow = Math.min(widest, Math.ceil(Math.sqrt(isolated.length * 1.6)));
  const left = (minX + maxX) / 2 - ((perRow - 1) * step) / 2;
  isolated.forEach((node, index) => {
    graph.setNodeAttribute(node, "x", left + (index % perRow) * step);
    graph.setNodeAttribute(
      node,
      "y",
      maxY + 70 + Math.floor(index / perRow) * step,
    );
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

  const order = graph.order;
  forceAtlas2.assign(graph, {
    // More repulsion needs more time to settle: cut the run short and the
    // clusters are still on their way apart when the picture is taken.
    iterations: order > 2000 ? 240 : order > 700 ? 420 : 380,
    settings: {
      ...forceAtlas2.inferSettings(graph),
      // Gravity near zero: the default pulls everything back into one disc,
      // which is precisely the shape that made the graph unreadable. What
      // holds it together is the edges, and they are enough.
      gravity: 0.02,
      // Room between clusters, and no overlapping discs inside them.
      //
      // Scaled with the graph: at 14 a 900-node, 3,000-edge repository packed
      // its clusters until they touched, and every edge running between two
      // of them crossed the same few hundred pixels. No amount of thinning
      // the ink fixes that — the strokes still overlap, they just overlap
      // fainter. Space is the fix; thin ink is what keeps it calm once there
      // is space.
      scalingRatio: order > 700 ? 32 : order > 250 ? 22 : 14,
      adjustSizes: true,
      // Hubs are pushed to the edge of their own cluster instead of sitting
      // on top of it, which is what makes a cluster's shape readable.
      outboundAttractionDistribution: true,
      barnesHutOptimize: order > 500,
      slowDown: 4,
    },
  });

  parkIsolated(graph);

  return graph;
}
