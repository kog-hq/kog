import Sigma from "sigma";
import { EdgeCurvedArrowProgram } from "@sigma/edge-curve";
import { EdgeArrowProgram } from "sigma/rendering";
import type { Settings } from "sigma/settings";
import type { NodeDisplayData, PartialButFor } from "sigma/types";
import { useCallback, useEffect, useMemo, useRef } from "react";
import { createNodeBorderProgram } from "@sigma/node-border";
import { buildGraph, type ColourBy } from "./build";
import type { Communities } from "./communities";
import {
  canvasTheme,
  communityColour,
  edgeInk,
  languageColour,
  shadeKey,
  type CanvasTheme,
  type Theme,
} from "@/lib/palette";
import type { KogProject, ProjectIndex } from "@/lib/kog";

export type LabelMode = "none" | "hubs" | "more" | "all";

/**
 * How many edges are drawn.
 *
 * `linked` is the default and the interesting one: an edge with *both* ends
 * off screen is a line crossing the view that connects nothing you can see.
 * Zoomed into one cluster of a 3,000-edge graph those are most of the ink,
 * and the answer to "what is here" cannot include relationships between two
 * elsewheres. Zoomed out everything is in frame, so nothing is culled and the
 * graph is never quietly reduced.
 *
 * It is a control rather than a rule because the trade is real: a hub with a
 * hundred dependents spread across the graph genuinely has a hundred edges,
 * and whether you want to see them leaving is a question about what you are
 * reading for, not one this file can answer.
 */
export type EdgeMode = "none" | "linked" | "all";

/**
 * How many names are on screen at once. Sigma decides by drawn size, so the
 * threshold is what separates "the five hubs" from "everything".
 */
const LABEL_MODES: Record<LabelMode, { threshold: number; density: number }> = {
  none: { threshold: Infinity, density: 1 },
  hubs: { threshold: 12, density: 1 },
  more: { threshold: 5, density: 2 },
  all: { threshold: 0, density: 20 },
};

/** Every name on a small graph, only the hubs on a large one. */
export function defaultLabelMode(nodeCount: number): LabelMode {
  if (nodeCount <= 120) return "all";
  if (nodeCount <= 400) return "more";
  return "hubs";
}

/**
 * Nodes wear their state as a ring: the fill says which language, the ring
 * says whether KOG could read it. One channel each, both always on.
 */
const NodeWithRing = createNodeBorderProgram({
  borders: [
    {
      color: { attribute: "borderColor", defaultValue: "#00000000" },
      size: { value: 0.2 },
    },
    { color: { attribute: "color" }, size: { fill: true } },
  ],
});

/**
 * Above this many neighbours, a neighbourhood stops labelling all of itself.
 * `packages/shared-types/src/index.ts` has 232 dependents: forcing 232 names
 * on screen at once answers "which files" with a grey smear, where the
 * inspector's list answers it precisely and the graph is left to show shape.
 */
const LABEL_ALL_NEIGHBOURS_UP_TO = 40;

/** How long the camera takes to travel to a selection, in milliseconds. */
const TRAVEL_MS = 320;

function prefersReducedMotion(): boolean {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

export type CanvasState = {
  /** Ids that pass the current filters. Everything else is hidden. */
  visible: Set<string> | null;
  selected: string | null;
  /** Anything the pointer is deliberately on, in a side panel. */
  hovered: string | null;
  labelMode: LabelMode;
  edgeMode: EdgeMode;
  groupByFolder: boolean;
  colourBy: ColourBy;
  theme: Theme;
};

type Props = CanvasState & {
  project: KogProject;
  index: ProjectIndex;
  communities: Communities;
  onSelect: (id: string | null) => void;
  /**
   * Filled in with a function that captures the canvas.
   *
   * The capture has to live next to the renderer: sigma draws nodes and edges
   * in WebGL, and a WebGL drawing buffer is cleared once the browser has
   * composited it. Reading it from anywhere else returns a transparent image
   * — which is exactly what happened, and produced a PNG of nothing but the
   * labels, those being the only layer drawn in plain 2D.
   */
  capture: React.RefObject<(() => string | null) | null>;
};

/**
 * Draw a node's name over a slab of background.
 *
 * Sigma paints label text straight onto the canvas, so a name crossing a
 * dense patch of edges becomes unreadable exactly where the graph is most
 * worth reading. The halo is the whole point of overriding the renderer.
 */
function labelDrawer(theme: CanvasTheme, bold = false) {
  return (
    context: CanvasRenderingContext2D,
    data: PartialButFor<
      NodeDisplayData,
      "x" | "y" | "size" | "label" | "color"
    >,
    settings: Settings,
  ): void => {
    if (!data.label) return;
    const size = settings.labelSize;
    context.font = `${bold ? 600 : settings.labelWeight} ${size}px ${settings.labelFont}`;
    const width = context.measureText(data.label).width;
    const x = data.x + data.size + 5;
    const y = data.y + size / 3;

    context.fillStyle = theme.labelHalo;
    context.beginPath();
    context.roundRect(x - 4, y - size + 1, width + 8, size + 5, 4);
    context.fill();

    context.fillStyle = bold ? theme.focus : theme.label;
    context.fillText(data.label, x, y);
  };
}

/**
 * Hovering a node reads its name. That is the whole of it.
 *
 * An earlier version answered a hover the way it answers a click: dim the
 * graph, light the neighbourhood, breathe a ring around the node. Dragging
 * the pointer across a dense cluster then fired that on every node it crossed,
 * and the graph strobed. Pointing at something is not the same act as asking
 * about it — the second one is a click, and it still gets the full answer.
 */
function hoverDrawer(theme: CanvasTheme) {
  return labelDrawer(theme, true);
}

/**
 * A box in graph coordinates, or `null` when nothing should be culled.
 */
type Box = { minX: number; minY: number; maxX: number; maxY: number } | null;

/**
 * The part of the graph currently on screen, in graph coordinates.
 *
 * Grown by a margin so an edge whose far end sits just past the frame still
 * draws, and the picture does not visibly change as you nudge the camera.
 */
function viewportOf(renderer: Sigma): Box {
  const { width, height } = renderer.getDimensions();
  const topLeft = renderer.viewportToGraph({ x: 0, y: 0 });
  const bottomRight = renderer.viewportToGraph({ x: width, y: height });
  const minX = Math.min(topLeft.x, bottomRight.x);
  const maxX = Math.max(topLeft.x, bottomRight.x);
  const minY = Math.min(topLeft.y, bottomRight.y);
  const maxY = Math.max(topLeft.y, bottomRight.y);
  const margin = Math.max(maxX - minX, maxY - minY) * 0.25;
  return {
    minX: minX - margin,
    minY: minY - margin,
    maxX: maxX + margin,
    maxY: maxY + margin,
  };
}

/** The smallest box holding every point, without spreading into `Math.max`. */
function extent(points: { x: number; y: number }[]) {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const point of points) {
    if (point.x < minX) minX = point.x;
    if (point.x > maxX) maxX = point.x;
    if (point.y < minY) minY = point.y;
    if (point.y > maxY) maxY = point.y;
  }
  return { minX, minY, maxX, maxY };
}

export function GraphCanvas(props: Props) {
  const {
    project,
    index,
    communities,
    visible,
    selected,
    hovered,
    labelMode,
    edgeMode,
    groupByFolder,
    colourBy,
    theme,
    onSelect,
    capture,
  } = props;

  const container = useRef<HTMLDivElement>(null);
  const sigma = useRef<Sigma | null>(null);
  // Read by the edge reducer on every repaint, written when the camera
  // settles. A ref rather than state: it must not re-render React.
  const onScreen = useRef<Box>(null);

  // The layout is the expensive part, so it is tied to what actually changes
  // its shape: the project and whether folders are collapsed. Filters,
  // selection, colour mode and hover never reach here.
  const graph = useMemo(
    () => buildGraph(project, index, communities, { groupByFolder }),
    // Deliberately not keyed on the theme: a theme change recolours in
    // place. Relaying out a 2,800-node graph to change its palette would
    // throw away the shape the reader had just learned.
    [project, index, communities, groupByFolder],
  );

  /**
   * Bring a set of nodes into view.
   *
   * Filtering used to leave the camera where it was, which on any filter whose
   * matches sit outside the current frame produced an empty canvas — filter to
   * SQL on a repository whose 56 `.sql` files import nothing, and all 56 are
   * parked in a row below the graph, off screen. The reader's conclusion is
   * that the files are not there.
   */
  const frame = useCallback((ids: string[], travel: boolean) => {
    const renderer = sigma.current;
    if (!renderer) return;
    const points = ids
      .map((id) => renderer.getNodeDisplayData(id))
      .filter((point) => point !== undefined);
    if (points.length === 0) return;

    const { minX, minY, maxX, maxY } = extent(points);
    const span = Math.max(maxX - minX, maxY - minY, 0.06);
    const state = {
      x: (minX + maxX) / 2,
      y: (minY + maxY) / 2,
      ratio: Math.min(Math.max(span * 1.35, 0.08), 1.2),
    };

    if (travel && !prefersReducedMotion()) {
      renderer
        .getCamera()
        .animate(state, { duration: TRAVEL_MS, easing: "quadraticInOut" });
    } else {
      renderer.getCamera().setState({ ...state, angle: 0 });
    }
  }, []);

  /** The anchor for highlighting, and the two directions around it. */
  const focus = useMemo(() => {
    // `hovered` only ever comes from a deliberate point in a side panel now;
    // the canvas itself no longer reports what the pointer passes over.
    const anchor = hovered ?? selected;
    if (!anchor || groupByFolder) return null;
    const uses = new Set(index.dependencies.get(anchor) ?? []);
    const usedBy = new Set(index.dependents.get(anchor) ?? []);
    return {
      anchor,
      // What this file uses, and what uses it. Direction is the whole
      // question a dependency graph answers, so the two are drawn
      // differently rather than merged into one blob of "related".
      uses,
      usedBy,
      nameThem: uses.size + usedBy.size <= LABEL_ALL_NEIGHBOURS_UP_TO,
    };
  }, [hovered, selected, index, groupByFolder]);

  useEffect(() => {
    if (!container.current) return;
    const canvas = canvasTheme(theme);
    const renderer = new Sigma(graph, container.current, {
      renderEdgeLabels: false,
      defaultEdgeColor: edgeInk(graph.size, theme).color,
      // Arrows are registered but never the default: 3,000 arrowheads at low
      // zoom is noise. The reducer promotes an edge to an arrow only while it
      // is being read.
      // Curves, not straight lines. Two files linked across a cluster draw
      // a chord that leaves the middle alone, where a straight line cuts
      // through everything between them — which is what made the graph look
      // like a scribble rather than a map.
      defaultEdgeType: "curve",
      edgeProgramClasses: {
        curve: EdgeCurvedArrowProgram,
        arrow: EdgeArrowProgram,
      },
      defaultNodeType: "bordered",
      nodeProgramClasses: { bordered: NodeWithRing },
      labelFont: '"JetBrains Mono Variable", ui-monospace, monospace',
      labelSize: 11.5,
      labelWeight: "500",
      labelGridCellSize: 72,
      defaultDrawNodeLabel: labelDrawer(canvas),
      defaultDrawNodeHover: hoverDrawer(canvas),
      zIndex: true,
    });
    sigma.current = renderer;
    // The layout's extent changes with the graph, so the camera is framed on
    // what was actually drawn rather than left wherever the previous one sat.
    renderer.getCamera().setState({ x: 0.5, y: 0.5, ratio: 1.05, angle: 0 });
    frame(graph.nodes(), false);

    renderer.on("clickNode", ({ node }) => onSelect(node));
    renderer.on("clickStage", () => onSelect(null));

    // Recompute what is on screen once the camera settles, and repaint.
    //
    // Debounced rather than per-frame: the edge set only has to be right when
    // someone is looking at it, and re-running the reducers over 2,800 nodes
    // on every frame of a drag would cost far more than it buys.
    let settle: number | undefined;
    const onCamera = () => {
      window.clearTimeout(settle);
      settle = window.setTimeout(() => {
        onScreen.current = viewportOf(renderer);
        renderer.refresh({ skipIndexation: true });
      }, 90);
    };
    renderer.getCamera().on("updated", onCamera);
    onScreen.current = viewportOf(renderer);

    return () => {
      window.clearTimeout(settle);
      renderer.kill();
      sigma.current = null;
    };
    // Deliberately not keyed on the theme: colours are decided in the
    // reducers, so a theme change is a repaint and never a rebuild.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [graph, onSelect, frame]);

  // Filtering, selection and hover are all one repaint: sigma asks these
  // reducers what each node and edge looks like right now.
  useEffect(() => {
    const renderer = sigma.current;
    if (!renderer) return;
    const canvas = canvasTheme(theme);
    const ink = edgeInk(graph.size, theme);

    renderer.setSetting("nodeReducer", (node, data) => {
      if (visible && !visible.has(node)) return { ...data, hidden: true };
      // Colour is computed here rather than stored on the node, so the
      // theme can never be one repaint behind: there is no cached attribute
      // to go stale.
      const fill =
        colourBy === "community"
          ? communityColour(data.community as number, theme)
          : languageColour(
              data.lang as string,
              data.kind === "asset",
              theme,
              shadeKey(node),
            );

      if (node === selected) {
        // The fill still says which language: a selection that repainted the
        // node would hide the one thing colour is for.
        return {
          ...data,
          color: fill,
          borderColor: canvas.focus,
          size: data.size * 1.4,
          zIndex: 4,
          forceLabel: true,
        };
      }
      if (focus) {
        if (node === focus.anchor) {
          return {
            ...data,
            color: fill,
            size: data.size * 1.3,
            zIndex: 3,
            forceLabel: true,
          };
        }
        if (focus.uses.has(node) || focus.usedBy.has(node)) {
          return { ...data, color: fill, zIndex: 2, forceLabel: focus.nameThem };
        }
        // Dimmed and shrunk rather than hidden: the shape of the whole is
        // the context that makes a neighbourhood mean anything, but it has
        // to stop competing while you read one. `dim` sits close to the
        // background on purpose — the mid grey it used to use turned the rest
        // of a 900-node graph into one solid slab.
        return {
          ...data,
          color: canvas.dim,
          size: data.size * 0.55,
          label: "",
          zIndex: 0,
        };
      }
      return { ...data, color: fill };
    });

    /** Whether a node sits inside the part of the graph on screen. */
    const inFrame = (node: string): boolean => {
      const box = onScreen.current;
      if (!box) return true;
      const x = graph.getNodeAttribute(node, "x") as number;
      const y = graph.getNodeAttribute(node, "y") as number;
      return x >= box.minX && x <= box.maxX && y >= box.minY && y <= box.maxY;
    };

    renderer.setSetting("edgeReducer", (edge, data) => {
      const [source, target] = graph.extremities(edge);
      if (visible && (!visible.has(source) || !visible.has(target))) {
        return { ...data, hidden: true };
      }
      if (edgeMode === "none") return { ...data, hidden: true };
      if (edgeMode === "linked" && !inFrame(source) && !inFrame(target)) {
        return { ...data, hidden: true };
      }
      if (focus) {
        // What the anchor uses: the focus colour, the thing you are
        // following. What uses the anchor: the plain foreground. The two
        // directions never have to be guessed from arrowheads alone.
        if (source === focus.anchor && focus.uses.has(target)) {
          return {
            ...data,
            type: "arrow",
            color: canvas.focus,
            size: 1.4,
            zIndex: 2,
          };
        }
        if (target === focus.anchor && focus.usedBy.has(source)) {
          return {
            ...data,
            type: "arrow",
            color: canvas.label,
            size: 1,
            zIndex: 2,
          };
        }
        return { ...data, color: ink.color, size: ink.size * 0.5 };
      }
      return { ...data, color: ink.color, size: ink.size };
    });

    renderer.setSetting("defaultDrawNodeLabel", labelDrawer(canvas));
    renderer.setSetting("defaultDrawNodeHover", hoverDrawer(canvas));
    renderer.setSetting("defaultEdgeColor", ink.color);
    renderer.refresh();
  }, [graph, index, visible, selected, focus, colourBy, edgeMode, theme]);

  useEffect(() => {
    const renderer = sigma.current;
    if (!renderer) return;
    const mode = LABEL_MODES[labelMode];
    renderer.setSetting("renderLabels", labelMode !== "none");
    renderer.setSetting("labelRenderedSizeThreshold", mode.threshold);
    renderer.setSetting("labelDensity", mode.density);
    // A full refresh, deliberately: which labels get drawn is decided from
    // sigma's label grid, and `skipIndexation` is exactly what skips
    // rebuilding it — so the setting landed but nothing changed on screen
    // until some other interaction forced a real refresh.
    renderer.refresh();
  }, [labelMode]);

  // Selecting from the search dialog, the inspector or the gap list has to
  // bring the node into view, or the selection is invisible and reads as a
  // no-op. The camera frames the whole neighbourhood rather than the node:
  // zooming to a fixed ratio puts a hub's 35 dependents off-screen, which is
  // exactly the answer the reader asked for.
  useEffect(() => {
    if (!selected || !graph.hasNode(selected)) return;
    const ids = [
      selected,
      ...(index.dependents.get(selected) ?? []),
      ...(index.dependencies.get(selected) ?? []),
    ].filter((id) => graph.hasNode(id) && (!visible || visible.has(id)));
    frame(ids, true);
  }, [selected, graph, index, visible, frame]);

  // A filter that leaves its matches off screen reads as a filter that
  // matched nothing. Nothing to do while a selection is on screen: the
  // effect above is already pointing the camera, and two of them competing
  // would fight for it.
  useEffect(() => {
    if (selected) return;
    frame(visible ? [...visible] : graph.nodes(), true);
  }, [visible, selected, graph, frame]);

  // Re-render, then read the buffer in the same task, before the browser
  // composites and clears it.
  useEffect(() => {
    capture.current = () => {
      const renderer = sigma.current;
      const element = container.current;
      if (!renderer || !element) return null;
      renderer.refresh();

      const layers = [...element.querySelectorAll("canvas")].filter(
        (canvas) => !canvas.classList.contains("sigma-mouse"),
      );
      const first = layers[0];
      if (!first) return null;

      const out = document.createElement("canvas");
      out.width = first.width;
      out.height = first.height;
      const context = out.getContext("2d");
      if (!context) return null;
      // A transparent PNG of a light-on-dark graph is invisible in most
      // viewers, so the theme's own background is painted first.
      context.fillStyle = canvasTheme(theme).background;
      context.fillRect(0, 0, out.width, out.height);
      for (const layer of layers) context.drawImage(layer, 0, 0);
      return out.toDataURL("image/png");
    };
    return () => {
      capture.current = null;
    };
  }, [capture, theme]);

  return <div ref={container} className="absolute inset-0" />;
}
