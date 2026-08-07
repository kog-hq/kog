/**
 * Turning a scan into something sigma can draw.
 *
 * The layout is computed once per view and never again: filtering, selecting
 * and recolouring all run through sigma's reducers, which repaint without
 * moving a node. A graph that rearranges itself every time you tick a
 * checkbox is unreadable, however fast it is.
 */

import Graph from "graphology";
import forceAtlas2 from "graphology-layout-forceatlas2";
import type { KogProject, NodeKind, ProjectIndex } from "@/lib/kog";
import { dirName } from "@/lib/kog";
import { languageColour, shadeKey, type Theme } from "@/lib/palette";

export type NodeAttributes = {
  label: string;
  size: number;
  color: string;
  kind: NodeKind;
  lang: string;
  /** Files behind this node: one, unless folders are collapsed. */
  members: string[];
  x: number;
  y: number;
};

function sizeFor(degree: number, kind: NodeKind): number {
  const base = 2.6 + Math.sqrt(degree) * 1.7;
  return kind === "asset" ? base * 0.6 : base;
}

export type BuildOptions = {
  /** Collapse every file into the folder that holds it. */
  groupByFolder: boolean;
};

/**
 * Build the drawable graph. Every node of the scan is here, whatever the
 * filters say: filtering is a reducer, not a rebuild, so a node hidden now
 * keeps the position it will have when it comes back.
 */
export function buildGraph(
  project: KogProject,
  index: ProjectIndex,
  theme: Theme,
  options: BuildOptions,
): Graph {
  const graph = new Graph({ type: "directed", multi: false });

  const add = (
    id: string,
    label: string,
    lang: string,
    kind: NodeKind,
    members: string[],
  ) => {
    graph.addNode(id, {
      label,
      size: 2,
      // The first paint only. From then on the reducer decides, so a theme
      // change never has to reach back into these attributes.
      color: languageColour(lang, kind === "asset", theme, shadeKey(id)),
      kind,
      lang,
      members,
      x: Math.random(),
      y: Math.random(),
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
      // A folder takes the language of whatever it mostly holds — the one
      // thing about a package you can read from across the room.
      const langs = new Map<string, number>();
      let kind: NodeKind = "asset";
      for (const id of files) {
        const node = index.byId.get(id);
        if (!node) continue;
        langs.set(node.lang, (langs.get(node.lang) ?? 0) + 1);
        if (node.kind === "source") kind = "source";
        else if (node.kind === "unread_source" && kind === "asset")
          kind = "unread_source";
      }
      const lang =
        [...langs.entries()].sort((a, b) => b[1] - a[1])[0]?.[0] ?? "";
      add(folder, folder, lang, kind, files);
    }
    for (const edge of project.graph.edges) {
      const from = dirName(edge.source);
      const to = dirName(edge.target);
      if (from === to || !graph.hasNode(from) || !graph.hasNode(to)) continue;
      graph.mergeEdge(from, to);
    }
  } else {
    for (const node of project.graph.nodes) {
      add(node.id, index.label.get(node.id) ?? node.id, node.lang, node.kind, [
        node.id,
      ]);
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

  // Iterations traded against size: a 3,000-node layout that takes four
  // seconds reads as a hang, and the extra passes buy very little once the
  // clusters have separated.
  const order = graph.order;
  const iterations = order > 2000 ? 120 : order > 700 ? 200 : 320;
  forceAtlas2.assign(graph, {
    iterations,
    settings: {
      ...forceAtlas2.inferSettings(graph),
      gravity: 0.7,
      barnesHutOptimize: order > 500,
    },
  });

  return graph;
}
