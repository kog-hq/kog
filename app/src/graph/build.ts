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
  FOLDER_SLOTS,
  NEUTRAL,
  STATE_COLOUR,
  folderKey,
  swatch,
  type NodeState,
} from "@/lib/palette";

export type ColourBy = "state" | "folder";

export type Theme = "light" | "dark";

export type NodeAttributes = {
  label: string;
  size: number;
  color: string;
  state: NodeState;
  folder: string;
  kind: NodeKind;
  lang: string;
  /** Files behind this node: one, unless folders are collapsed. */
  members: string[];
  x: number;
  y: number;
};

export function readCanvasTheme(theme: Theme) {
  const style = getComputedStyle(document.documentElement);
  const value = (name: string) => style.getPropertyValue(name).trim();
  return {
    mode: theme,
    edge: value("--edge"),
    labelHalo: value("--label-halo"),
    label: value("--foreground"),
    background: value("--background"),
    signal: value("--color-signal") || "#e0457b",
    focus: value("--color-focus") || "#4a90d9",
  };
}

export type CanvasTheme = ReturnType<typeof readCanvasTheme>;

/** What the file is, as far as the map is concerned. */
export function stateOf(id: string, kind: NodeKind, index: ProjectIndex): NodeState {
  if (kind === "unread_source") return "unread";
  if (kind === "asset") return "asset";
  return index.diagnosticsByFile.has(id) ? "gap" : "read";
}

export function colourOf(
  state: NodeState,
  folder: string,
  mode: ColourBy,
  index: ProjectIndex,
  theme: Theme,
): string {
  // The two signals win in both modes: a file KOG could not read is drawn as
  // one wherever it sits, or the colour means nothing.
  if (mode === "state" || state === "unread" || state === "asset") {
    return swatch(STATE_COLOUR[state], theme);
  }
  const slot = index.folderSlot.get(folder);
  return swatch(slot === undefined ? NEUTRAL : FOLDER_SLOTS[slot], theme);
}

/** A ring is the second channel: the signal never rests on hue alone. */
export function ringOf(state: NodeState, theme: CanvasTheme): string | null {
  if (state === "unread") return theme.signal;
  if (state === "gap") return swatch(STATE_COLOUR.gap, theme.mode);
  return null;
}

function sizeFor(degree: number, kind: NodeKind): number {
  const base = 2.4 + Math.sqrt(degree) * 1.7;
  return kind === "asset" ? base * 0.62 : base;
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
  options: BuildOptions,
): Graph {
  const graph = new Graph({ type: "directed", multi: false });

  if (options.groupByFolder) {
    const members = new Map<string, string[]>();
    for (const node of project.graph.nodes) {
      const folder = dirName(node.id);
      const list = members.get(folder);
      if (list) list.push(node.id);
      else members.set(folder, [node.id]);
    }
    for (const [folder, files] of members) {
      // A folder takes the state of its worst file: one unread file in a
      // package is what the reader needs to see, not the nine that are fine.
      const states = files.map((id) => {
        const node = index.byId.get(id);
        return node ? stateOf(id, node.kind, index) : "asset";
      });
      const state: NodeState = states.includes("unread")
        ? "unread"
        : states.includes("gap")
          ? "gap"
          : states.includes("read")
            ? "read"
            : "asset";
      graph.addNode(folder, {
        label: folder,
        size: 3,
        color: NEUTRAL.dark,
        state,
        folder: folderKey(`${folder}/x`),
        kind: index.byId.get(files[0])?.kind ?? "asset",
        lang: index.byId.get(files[0])?.lang ?? "",
        members: files,
        x: Math.random(),
        y: Math.random(),
      });
    }
    for (const edge of project.graph.edges) {
      const from = dirName(edge.source);
      const to = dirName(edge.target);
      if (from === to || !graph.hasNode(from) || !graph.hasNode(to)) continue;
      graph.mergeEdge(from, to);
    }
  } else {
    for (const node of project.graph.nodes) {
      graph.addNode(node.id, {
        label: index.label.get(node.id) ?? node.id,
        size: 2,
        color: NEUTRAL.dark,
        state: stateOf(node.id, node.kind, index),
        folder: folderKey(node.id),
        kind: node.kind,
        lang: node.lang,
        members: [node.id],
        x: Math.random(),
        y: Math.random(),
      });
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

/** Recolour in place: a theme or mode change never relayouts, and never jumps. */
export function recolour(
  graph: Graph,
  index: ProjectIndex,
  mode: ColourBy,
  theme: Theme,
): void {
  graph.forEachNode((node, attributes) => {
    graph.setNodeAttribute(
      node,
      "color",
      colourOf(attributes.state as NodeState, attributes.folder as string, mode, index, theme),
    );
  });
}
