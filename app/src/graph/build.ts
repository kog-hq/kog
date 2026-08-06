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
import {
  canvasTheme,
  languageColour,
  shadeKey,
  type CanvasTheme,
  type Theme,
} from "@/lib/palette";

/** What is true about a file, drawn as a ring rather than as a fill. */
export type NodeState = "fine" | "gap" | "unread";

export type NodeAttributes = {
  label: string;
  size: number;
  color: string;
  /** The ring. Equal to `color` when there is nothing to say. */
  borderColor: string;
  state: NodeState;
  kind: NodeKind;
  lang: string;
  /** Files behind this node: one, unless folders are collapsed. */
  members: string[];
  x: number;
  y: number;
};

export function stateOf(
  id: string,
  kind: NodeKind,
  index: ProjectIndex,
): NodeState {
  if (kind === "unread_source") return "unread";
  if (kind === "asset") return "fine";
  return index.diagnosticsByFile.has(id) ? "gap" : "fine";
}

/** The ring colour, or the fill when the file has nothing to report. */
export function ringOf(
  state: NodeState,
  fill: string,
  theme: CanvasTheme,
): string {
  if (state === "unread") return theme.signal;
  if (state === "gap") return theme.warn;
  return fill;
}

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
  const canvas = canvasTheme(theme);

  const add = (
    id: string,
    label: string,
    lang: string,
    kind: NodeKind,
    state: NodeState,
    members: string[],
  ) => {
    const fill = languageColour(lang, kind === "asset", theme, shadeKey(id));
    graph.addNode(id, {
      label,
      size: 2,
      color: fill,
      borderColor: ringOf(state, fill, canvas),
      state,
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
      // A folder takes the state of its worst file — one unread file in a
      // package is what the reader needs to see, not the nine that are fine —
      // and the language of whatever it mostly holds.
      const states = files.map((id) => {
        const node = index.byId.get(id);
        return node ? stateOf(id, node.kind, index) : "fine";
      });
      const state: NodeState = states.includes("unread")
        ? "unread"
        : states.includes("gap")
          ? "gap"
          : "fine";

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
      add(folder, folder, lang, kind, state, files);
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
        stateOf(node.id, node.kind, index),
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

/** Recolour in place: a theme change never relayouts, and never jumps. */
export function recolour(graph: Graph, theme: Theme): void {
  const canvas = canvasTheme(theme);
  graph.forEachNode((node, attributes) => {
    const fill = languageColour(
      attributes.lang as string,
      (attributes.kind as NodeKind) === "asset",
      theme,
      shadeKey(node),
    );
    graph.setNodeAttribute(node, "color", fill);
    graph.setNodeAttribute(
      node,
      "borderColor",
      ringOf(attributes.state as NodeState, fill, canvas),
    );
  });
}
