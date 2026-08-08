import Sigma from "sigma";
import { EdgeArrowProgram, EdgeRectangleProgram } from "sigma/rendering";
import type { Settings } from "sigma/settings";
import type { NodeDisplayData, PartialButFor } from "sigma/types";
import { useCallback, useEffect, useMemo, useRef } from "react";
import { createNodeBorderProgram } from "@sigma/node-border";
import { buildGraph, type ColourBy } from "./build";
import type { Communities } from "./communities";
import { startPhysics, type Physics, PHYSICS } from "./physics";
import {
  canvasTheme,
  communityColour,
  edgeInk,
  mix,
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
 * How names behave, as two independent questions.
 *
 * `threshold` and `density` are sigma's: they pick *which* nodes are allowed a
 * label at all. `zoom` is Obsidian's, and it decides *whether any name is
 * drawn at this magnification*:
 *
 *     textAlpha = clamp(log2(scale) + 1 - textFadeMultiplier, 0, 1)
 *
 * That single line is why their graph has no labels on it at rest. Zoomed out
 * to fit a vault, `log2(scale)` is around -1.9 and every name is at alpha
 * zero; names appear, continuously, as you come in. Ours were a mode you set
 * and then had to live with, so the overview arrived pre-covered in text —
 * which is the complaint, exactly.
 *
 * `zoom` is the camera ratio at which names reach full strength; they are gone
 * by twice it. Sigma's ratio counts the other way from Obsidian's scale, so
 * the sign is flipped and the shape is the same.
 */
const LABEL_MODES: Record<
  LabelMode,
  { threshold: number; density: number; zoom: number }
> = {
  none: { threshold: Infinity, density: 1, zoom: 0 },
  hubs: { threshold: 12, density: 1, zoom: 0.22 },
  more: { threshold: 5, density: 2, zoom: 0.45 },
  all: { threshold: 0, density: 20, zoom: 1.6 },
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

/**
 * How far the graph recedes behind whatever is being read.
 *
 * Obsidian's `QQ = .2`, lifted whole. Not zero: the shape of the whole is the
 * context that makes a neighbourhood mean anything, and a fifth of full
 * strength is enough to keep it there without competing.
 */
const DIMMED = 0.2;

/**
 * How fast the graph fades, as the fraction of the gap left standing per frame
 * at 60 Hz.
 *
 * This is Obsidian's whole animation, and it is three tokens long:
 *
 *     $Q = function(e, t, n) { return void 0===n && (n=.9), e*n + t*(1-n) }
 *
 * applied to every alpha on every rendered frame. It is not a transition with
 * a duration — there is no start, no end and no easing curve to choose. The
 * value simply chases its target, quickly at first and ever more gently, and
 * it is already chasing a new target if you move the pointer mid-flight.
 *
 * That is the difference the eye reads as "soft". Our previous fade was a
 * 320 ms tween started on a state change: interrupt it and it restarts from
 * wherever it was toward a new end point, which is why it read as switching
 * rather than gliding. A damped value cannot be interrupted, because it was
 * never following a plan.
 *
 * Applied per frame it would run twice as fast on a 120 Hz display, so the
 * exponent below is corrected by elapsed time. At 60 Hz that is exactly
 * Obsidian's number; the time constant is about 158 ms either way.
 */
const FADE_DAMP_AT_60HZ = 0.9;

/**
 * How close to its target the fade has to be before the loop stops.
 *
 * An exponential never actually arrives, so something has to call it, and the
 * honest place to stop is the frame on which the picture stops changing. A
 * node moves at most `1 - DIMMED` of the way from its own colour to the
 * background, so a change in the fade smaller than `1/255 / 0.8` cannot move
 * any channel by a whole value. Below that the loop would be repainting 2,800
 * nodes to produce an identical image.
 */
const FADE_SETTLED = 1 / 255 / (1 - DIMMED);

/**
 * The fixed extent, in graph units, that the drawn coordinate space is pinned
 * to — and the reason dragging a node no longer drags the world with it.
 *
 * Sigma normalises graph coordinates into 0..1 before drawing, and by default
 * it rebuilds that mapping from the graph's own bounding box on every full
 * refresh:
 *
 *     this.nodeExtent = graphExtent(this.graph);
 *     this.normalizationFunction = createNormalizationFunction(
 *       this.customBBox || this.nodeExtent);
 *
 * On a frozen layout that is invisible. On a live one it means the picture is
 * rescaled and recentred around whatever the outermost node is doing: pull one
 * file toward the top of the screen and the box grows, so every other node is
 * squeezed and slid to compensate, and the whole graph appears to follow the
 * cursor. Obsidian has no such step — it draws in absolute coordinates and
 * moves only the camera.
 *
 * A `customBBox` overrides the mapping and freezes it. The size is derived
 * from the node count rather than fixed, because the layout's own radius grows
 * as √n: the centre force at 0.1 balances repulsion at -1000 around
 * r ≈ 100·√n. Scaling the box the same way means a 64-file project and a
 * 2,800-file one both settle into a similar fraction of the frame, so one set
 * of camera ratios and one label calibration hold across every repository
 * instead of being tuned for whichever one was open.
 *
 * Nothing is clipped by it: the mapping is affine, and a node outside the box
 * simply lands outside 0..1 and draws normally.
 */
function drawnExtent(order: number): { x: [number, number]; y: [number, number] } {
  const bound = Math.max(2000, 130 * Math.sqrt(order)) * 1.15;
  return { x: [-bound, bound], y: [-bound, bound] };
}

function prefersReducedMotion(): boolean {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

export type CanvasState = {
  /** Ids that pass the current filters. Everything else is hidden. */
  visible: Set<string> | null;
  selected: string | null;
  /** What the pointer is on, whether in the canvas or in a side panel. */
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
  onHover: (id: string | null) => void;
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

/** What the reducers hand the label drawer, on top of sigma's own fields. */
type LabelData = { labelFade?: number };

/**
 * Draw a node's name centred beneath it.
 *
 * There used to be a slab of background behind every name, because a name
 * crossing a dense patch of edges is unreadable. That was a fix for the wrong
 * problem: the slabs turned a graph with every label on into a wall of boxes,
 * and the real complaint was never legibility but noise. Faint edges make the
 * halo unnecessary; the halo never made the edges quieter.
 *
 * Alpha is the product of two independent things, which is Obsidian's `y *= s`
 * exactly: how far the camera is out, and how far this particular node has
 * receded behind whatever is being read. A name being read is exempt from the
 * first — see `forceLabel`.
 */
function labelDrawer(
  theme: CanvasTheme,
  zoomAlpha: { current: number },
  bold = false,
) {
  return (
    context: CanvasRenderingContext2D,
    data: PartialButFor<
      NodeDisplayData,
      "x" | "y" | "size" | "label" | "color"
    >,
    settings: Settings,
  ): void => {
    if (!data.label) return;
    const fade = (data as LabelData).labelFade ?? 1;
    // A forced label is one the reader asked for by pointing or clicking, and
    // it is legible at any magnification. Everything else lives or dies by the
    // camera.
    const alpha = (data.forceLabel ? 1 : zoomAlpha.current) * fade;
    if (alpha < 0.02) return;

    const size = settings.labelSize;
    context.font = `${bold ? 600 : settings.labelWeight} ${size}px ${settings.labelFont}`;
    // Blended towards the background rather than set as a globalAlpha: the
    // canvas is opaque, so the two are identical on screen, and a blend costs
    // no state change on a context drawing hundreds of names.
    context.fillStyle = mix(
      bold ? theme.focus : theme.label,
      theme.background,
      alpha,
    );
    context.textAlign = "center";
    context.fillText(data.label, data.x, data.y + data.size + size + 2);
    context.textAlign = "left";
  };
}

/**
 * Hovering a node reads its name, in the focus ink.
 *
 * The rest of the answer to a hover — the graph receding, the neighbourhood
 * staying lit — is in the reducers, and it arrives as a fade rather than as a
 * switch. Sigma draws this one on top of everything else.
 */
function hoverDrawer(theme: CanvasTheme, zoomAlpha: { current: number }) {
  return labelDrawer(theme, zoomAlpha, true);
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

/** The anchor for highlighting, and the two directions around it. */
type Focus = {
  anchor: string;
  uses: Set<string>;
  usedBy: Set<string>;
  nameThem: boolean;
} | null;

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
    onHover,
    capture,
  } = props;

  const container = useRef<HTMLDivElement>(null);
  const sigma = useRef<Sigma | null>(null);
  const physics = useRef<Physics | null>(null);
  // Read by the edge reducer on every repaint, written when the camera
  // settles. A ref rather than state: it must not re-render React.
  const onScreen = useRef<Box>(null);
  /** How far the graph has faded back behind whatever is being read, 0 to 1. */
  const fade = useRef(0);
  /**
   * What is being read, as a ref rather than a reducer dependency.
   *
   * The reducers used to be re-registered and the graph fully re-indexed every
   * time this changed, which on a hover is every node the pointer crosses. A
   * full `refresh()` over 2,800 nodes per pointer sample is why answering a
   * hover had to be delayed by 90 ms to stay usable at all — and that delay is
   * what made hovering feel like operating a switch. Obsidian assigns a
   * variable and lets the next frame read it; so does this.
   */
  const focused = useRef<Focus>(null);
  /** Label strength from the camera alone, before any node's own fade. */
  const zoomAlpha = useRef(1);
  /**
   * What an edge's width must be multiplied by to come out the same on screen
   * at any zoom. See `edgeInk` and the edge reducer.
   */
  const edgeScale = useRef(1);
  /** The camera ratio at which names reach full strength. */
  const labelZoom = useRef(LABEL_MODES[labelMode].zoom);

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

  /** Names fade with the camera, continuously. See `LABEL_MODES`. */
  const readZoom = useCallback((ratio: number) => {
    const zoom = labelZoom.current;
    if (zoom <= 0) return 0;
    return Math.min(1, Math.max(0, 1 - Math.log2(ratio / zoom)));
  }, []);

  /**
   * Bring a set of nodes into view.
   *
   * Filtering used to leave the camera where it was, which on any filter whose
   * matches sit outside the current frame produced an empty canvas — filter to
   * SQL on a repository whose 56 `.sql` files import nothing, and all 56 are
   * parked in a row below the graph, off screen. The reader's conclusion is
   * that the files are not there.
   */
  const frame = useCallback(
    (ids: string[], travel: boolean) => {
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
        ratio: Math.min(Math.max(span * 1.35, 0.08), 3),
      };
      zoomAlpha.current = readZoom(state.ratio);

      if (travel && !prefersReducedMotion()) {
        renderer
          .getCamera()
          .animate(state, { duration: TRAVEL_MS, easing: "quadraticInOut" });
      } else {
        renderer.getCamera().setState({ ...state, angle: 0 });
      }
    },
    [readZoom],
  );

  const focus = useMemo<Focus>(() => {
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
    const ink = edgeInk(theme);
    const renderer = new Sigma(graph, container.current, {
      renderEdgeLabels: false,
      defaultEdgeColor: ink.color,
      // Arrows are registered but never the default: 3,000 arrowheads at low
      // zoom is noise. The reducer promotes an edge to an arrow only while it
      // is being read.
      //
      // Straight lines, not curves. Curves were the right call for a frozen
      // layout — a chord across a cluster leaves the middle alone where a
      // straight line cuts through it. They stop being right the moment
      // positions move: the control point of a curved edge scales with the
      // edge's length, so a node pulled away from its cluster turns its two
      // hundred edges into arcs that sweep the whole screen. Obsidian draws
      // straight lines, and a straight line between two moving points is the
      // only edge whose shape says nothing beyond where its ends are.
      defaultEdgeType: "line",
      edgeProgramClasses: {
        line: EdgeRectangleProgram,
        arrow: EdgeArrowProgram,
      },
      // Sigma's floor on how thin an edge may be drawn, in pixels. It defaults
      // to 1.7, and until it was lowered here every width `edgeInk` chose was
      // clamped straight back up to it — the whole density ladder was
      // decorative. Now that the reducer sets a real width, this is only a
      // guard against an edge thinning into nothing.
      minEdgeThickness: 0.5,
      // See `drawnExtent`. Together these are what stop the picture from
      // rescaling itself around a dragged node.
      autoRescale: false,
      // The camera can no longer be flown into empty space. Zooming past
      // these is never useful: below `min` a single node fills the frame,
      // above `max` the graph is a smudge.
      minCameraRatio: 0.02,
      maxCameraRatio: 6,
      defaultNodeType: "bordered",
      nodeProgramClasses: { bordered: NodeWithRing },
      labelFont: '"JetBrains Mono Variable", ui-monospace, monospace',
      labelSize: 11.5,
      labelWeight: "500",
      labelGridCellSize: 72,
      defaultDrawNodeLabel: labelDrawer(canvas, zoomAlpha),
      defaultDrawNodeHover: hoverDrawer(canvas, zoomAlpha),
      zIndex: true,
    });
    renderer.setCustomBBox(drawnExtent(graph.order));
    sigma.current = renderer;

    // A wide ratio to start, because framing the seed would fit the compact
    // knot moments before the graph leaves it. The camera holds while it
    // blooms; one frame at settle fits what was actually drawn.
    renderer.getCamera().setState({ x: 0.5, y: 0.5, ratio: 1.6, angle: 0 });

    renderer.on("clickNode", ({ node }) => onSelect(node));
    renderer.on("clickStage", () => onSelect(null));

    // Sigma zooms on double click, on a node and on the background alike. It
    // fights every other way of moving around and lands you somewhere you did
    // not ask to be, so it is switched off rather than tuned.
    renderer.on("doubleClickNode", ({ preventSigmaDefault }) =>
      preventSigmaDefault(),
    );
    renderer.on("doubleClickStage", ({ preventSigmaDefault }) =>
      preventSigmaDefault(),
    );

    // Hover answers immediately. There used to be a 90 ms delay here so that
    // dragging the pointer across a dense cluster would not fire on every node
    // it crossed — but the strobing that was meant to prevent came from the
    // effect being a hard switch, not from how often it fired. A damped fade
    // crossing five nodes in five frames simply never gets far from where it
    // was. Obsidian has no delay either.
    renderer.on("enterNode", ({ node }) => {
      // Held nodes keep their own cursor and their own highlight. Without this
      // guard a drag flickers: the node slides out from under the pointer,
      // sigma reports a leave and then an enter, and each one overwrites the
      // grab cursor and the thing being read.
      if (dragging) return;
      // A node is clickable and draggable, so it should say so before you
      // find out by trying.
      if (container.current) container.current.style.cursor = "pointer";
      onHover(node);
    });
    renderer.on("leaveNode", () => {
      if (dragging) return;
      if (container.current) container.current.style.cursor = "";
      onHover(null);
    });

    const engine = startPhysics(graph);
    physics.current = engine;

    if (import.meta.env.DEV) {
      // A handle for tuning from the console, and for driving the canvas from
      // a browser test — a WebGL canvas has no DOM to assert against, so
      // without this the only way to check that a drag moved a node rather
      // than the camera is to look at a screenshot and guess. Vite drops the
      // whole block from a production build.
      (window as unknown as Record<string, unknown>).__kog = {
        renderer,
        physics: engine,
        forces: PHYSICS,
      };
    }

    // Dragging a node. The camera is disabled for the duration, or the drag
    // pans the view instead of moving the node.
    let pressed: string | null = null;
    let pressedAt: { x: number; y: number } | null = null;
    let dragging = false;
    const onDownNode = ({
      node,
      event,
    }: {
      node: string;
      event: { x: number; y: number };
    }) => {
      pressed = node;
      pressedAt = { x: event.x, y: event.y };
      dragging = false;
      renderer.getCamera().disable();
    };
    renderer.on("downNode", onDownNode);
    const mouse = renderer.getMouseCaptor();
    const onMove = (event: {
      x: number;
      y: number;
      preventSigmaDefault: () => void;
      original: Event;
    }) => {
      if (!pressed || !pressedAt) return;
      if (!dragging) {
        // A press only becomes a drag once the pointer has actually travelled.
        // Obsidian's threshold, and without it every click nudges the node it
        // lands on — a graph you cannot click without disturbing is a graph
        // you stop clicking.
        const dx = event.x - pressedAt.x;
        const dy = event.y - pressedAt.y;
        if (dx * dx + dy * dy <= PHYSICS.dragSlopSquared) return;
        dragging = true;
        if (container.current) container.current.style.cursor = "grabbing";
        // A dragged node reads as a held one: Obsidian highlights
        // `dragNode || highlightNode`, so the neighbourhood you are pulling
        // stays lit while you pull it.
        onHover(pressed);
        engine.grab(pressed);
      }
      // Held inside the frame. Dragging a node past the edge used to keep
      // pulling — the pointer left the canvas, the spring stayed stretched,
      // and the centre force dragged the whole graph after it, so the picture
      // slid off the top of the screen with nothing to stop it. The cursor may
      // leave; the node may not.
      const { width, height } = renderer.getDimensions();
      const at = renderer.viewportToGraph({
        x: Math.min(width, Math.max(0, event.x)),
        y: Math.min(height, Math.max(0, event.y)),
      });
      // With a solver running, the node is moved by pinning it, not by writing
      // its position: a write would fight the next tick.
      engine.moveTo(at.x, at.y);
      // Sigma would otherwise also pan; the browser would otherwise select
      // text across the page while the button is held.
      event.preventSigmaDefault();
      event.original.preventDefault();
      event.original.stopPropagation();
    };
    const onRelease = () => {
      if (!pressed) return;
      pressed = null;
      pressedAt = null;
      if (dragging) engine.release();
      dragging = false;
      if (container.current) container.current.style.cursor = "";
      renderer.getCamera().enable();
    };
    mouse.on("mousemovebody", onMove);
    // A drag ends when the button comes up, wherever the pointer happens to be
    // — hence `window` and not the canvas.
    //
    // It used to end on `mouseleave` as well, and that one line was most of
    // "dragging a node takes the whole graph with it". Pull a node toward the
    // top of the screen — the gesture the complaint was about — and the
    // pointer crosses out of the canvas partway through. `mouseleave` then
    // ended the drag and, worse, re-enabled the camera *mid-gesture*, so every
    // remaining millimetre of the same movement panned the view instead. The
    // graph appeared to follow the cursor off the top of the window because
    // that is exactly what it was doing.
    //
    // Sigma emits `mousemovebody` precisely so a drag can survive leaving the
    // container. Leaving is not letting go.
    window.addEventListener("mouseup", onRelease);

    // Recompute what is on screen once the camera settles, and repaint.
    //
    // Debounced rather than per-frame: the edge set only has to be right when
    // someone is looking at it, and re-running the reducers over 2,800 nodes
    // on every frame of a drag would cost far more than it buys. Label
    // strength is not debounced — it is one logarithm, and it has to track the
    // wheel rather than arrive after it.
    let settle: number | undefined;
    let lastRatio = renderer.getCamera().ratio;
    edgeScale.current = Math.sqrt(lastRatio);
    const onCamera = () => {
      // Rule 3 of this file: every callback that can outlive its renderer has
      // to re-check that it is still the live one. This handler used to be
      // exempt in practice, because all it did was schedule a debounced
      // refresh and *that* carried the guard. Repainting synchronously on a
      // change of zoom moved the danger up here — a camera animation still in
      // flight when the project changes reaches a killed renderer and throws
      // on its own node programs.
      if (sigma.current !== renderer) return;
      // Keep the graph reachable. The drawn space is now fixed rather than
      // fitted to the graph, so clamping the centre to a little beyond 0..1
      // means a pan can never leave you staring at blank canvas with no way
      // back. Written only when it is actually out of bounds — `setState`
      // re-fires this handler.
      const camera = renderer.getCamera();
      const state = camera.getState();
      const x = Math.min(1.3, Math.max(-0.3, state.x));
      const y = Math.min(1.3, Math.max(-0.3, state.y));
      if (x !== state.x || y !== state.y) {
        camera.setState({ ...state, x, y });
        return;
      }
      // Zoom changes what a name and a line look like; panning does not. So
      // the two are answered differently: a change of ratio repaints at once,
      // because a width that arrives 90 ms after the wheel is a width that
      // visibly snaps, while the pan-dependent edge culling below can wait.
      if (state.ratio !== lastRatio) {
        lastRatio = state.ratio;
        zoomAlpha.current = readZoom(state.ratio);
        edgeScale.current = Math.sqrt(state.ratio);
        renderer.refresh({ skipIndexation: true });
      }
      window.clearTimeout(settle);
      settle = window.setTimeout(() => {
        // Same guard as the fade loop: a deferred callback in this file must
        // re-check that its renderer is still the live one.
        if (sigma.current !== renderer) return;
        onScreen.current = viewportOf(renderer);
        renderer.refresh({ skipIndexation: true });
      }, 90);
    };
    renderer.getCamera().on("updated", onCamera);
    onScreen.current = viewportOf(renderer);

    // Sigma sizes itself from its container, but only re-reads that size on a
    // **window** resize — it installs no `ResizeObserver`. Every other way the
    // canvas can change width therefore went unnoticed, and closing the side
    // column is now one of them: the window never resizes, so the drawing
    // buffer stays at the old width and the graph is drawn into a frame that
    // no longer exists. Observing the container covers all of them at once.
    const resized = new ResizeObserver(() => {
      if (sigma.current !== renderer) return;
      renderer.resize();
      onScreen.current = viewportOf(renderer);
      renderer.refresh({ skipIndexation: true });
    });
    resized.observe(container.current);

    const unsubscribe = engine.onSettle((first) => {
      // Switching project between the last tick and the settle leaves this
      // handler holding a renderer React has already killed, and refreshing
      // one throws on its own node programs. The callback has to be scoped to
      // the renderer that is still on screen.
      if (sigma.current !== renderer) return;
      // A full refresh, not a `skipIndexation` one: the spatial index and the
      // label grid are both a second out of date after a bloom, so hover would
      // otherwise point at where nodes used to be.
      renderer.refresh();
      onScreen.current = viewportOf(renderer);
      // Only the first settle fits the camera. Every drag reheats the solver
      // and every release settles it again, so framing on each one meant the
      // view jumped back a step after every single interaction — which is
      // exactly what "it zooms out by itself" was.
      if (first) frame(graph.nodes(), true);
    });

    // Reduced motion runs the solver to a standstill inside `startPhysics`
    // without ever firing an end event, so the one framing it is owed happens
    // here instead.
    if (prefersReducedMotion()) {
      renderer.refresh();
      onScreen.current = viewportOf(renderer);
      frame(graph.nodes(), false);
    }

    return () => {
      resized.disconnect();
      engine.stop();
      physics.current = null;
      unsubscribe();
      window.clearTimeout(settle);
      mouse.off("mousemovebody", onMove);
      window.removeEventListener("mouseup", onRelease);
      // The camera outlives nothing here, but the listener was never removed
      // and a leaked one is how the guard above came to be needed.
      renderer.getCamera().off("updated", onCamera);
      renderer.kill();
      sigma.current = null;
    };
    // Deliberately not keyed on the theme: colours are decided in the
    // reducers, so a theme change is a repaint and never a rebuild.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [graph, onSelect, onHover, frame, readZoom]);

  /**
   * Chase the fade toward its target, one damped step per frame.
   *
   * The loop runs until the gap closes and then stops on its own; an idle
   * canvas still costs nothing. Because the value is damped rather than
   * tweened, a hover that arrives mid-flight needs no special handling — it
   * changes the target and the next frame carries on from wherever the value
   * had got to.
   */
  useEffect(() => {
    const renderer = sigma.current;
    if (!renderer) return;
    focused.current = focus;
    const target = focus ? 1 : 0;

    if (prefersReducedMotion()) {
      fade.current = target;
      renderer.refresh({ skipIndexation: true });
      return;
    }

    let raf = 0;
    let last = performance.now();
    const step = (now: number) => {
      // This effect is keyed on `focus` alone, so a graph change swaps the
      // renderer underneath a loop that is still in flight. Refreshing a
      // killed renderer throws on its own node programs. Latent until the
      // layout became live — now the loop is nearly always running.
      if (sigma.current !== renderer) return;
      const elapsed = Math.min(64, now - last);
      last = now;
      const keep = Math.pow(FADE_DAMP_AT_60HZ, elapsed / (1000 / 60));
      fade.current = fade.current * keep + target * (1 - keep);
      const remaining = Math.abs(target - fade.current);
      if (remaining < FADE_SETTLED) fade.current = target;
      renderer.refresh({ skipIndexation: true });
      if (remaining >= FADE_SETTLED) raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  }, [focus]);

  // Filtering, selection and colour are one repaint: sigma asks these reducers
  // what each node and edge looks like right now. Hover is deliberately absent
  // from the dependencies — it arrives through `focused` and costs a frame, not
  // a re-registration. See that ref.
  useEffect(() => {
    const renderer = sigma.current;
    if (!renderer) return;
    const canvas = canvasTheme(theme);
    const ink = edgeInk(theme);

    // Everything recedes towards the background rather than towards a mid
    // grey: the grey turned the rest of a 900-node graph into one solid slab,
    // where receding towards the background is what "further away" actually
    // looks like.
    //
    // The blend is exact. It used to be quantised to sixteen steps so the
    // results could be cached — blending means parsing two hex strings and
    // formatting a third, and doing that for 2,800 nodes sixty times a second
    // is a quarter of a million string operations per second. But sixteen
    // steps across the fade is a step of 5% of the distance between a node's
    // colour and the background, and on a saturated fill that is ten values
    // per channel: visible banding. The fade was stepping because it had been
    // told to.
    //
    // The cache was keyed wrongly, is all. Within one frame `fade` does not
    // change, so neither does `strength` — the only thing that varies across
    // the 2,800 calls is the fill. Keyed on the fill alone and cleared
    // whenever the strength moves, the same map answers every node from at
    // most a few dozen entries: eleven community colours, or a language and
    // folder shade per area. Cheaper than the quantised version was, and with
    // nothing rounded off.
    const blended = new Map<string, string>();
    let blendedAt = Number.NaN;
    const towards = (fill: string, strength: number): string => {
      if (strength !== blendedAt) {
        blended.clear();
        blendedAt = strength;
      }
      const cached = blended.get(fill);
      if (cached !== undefined) return cached;
      const value = mix(fill, canvas.background, strength);
      blended.set(fill, value);
      return value;
    };

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

      const focus = focused.current;
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
        // Faded, not shrunk and not hidden. Shrinking was ours, not
        // Obsidian's, and it is the wrong channel twice over: size means
        // degree here, so animating it says something false about the graph,
        // and a node that both dims and shrinks reads as leaving rather than
        // as receding.
        const strength = 1 - (1 - DIMMED) * fade.current;
        return {
          ...data,
          color: towards(fill, strength),
          // The name recedes with its node. Multiplied into the camera's own
          // label strength by the drawer, which is Obsidian's `y *= s`.
          labelFade: strength,
          zIndex: 0,
        };
      }
      return { ...data, color: fill };
    });

    /** Whether a node sits inside the part of the graph on screen. */
    const inFrame = (node: string): boolean => {
      // Nothing is culled while the solver is moving nodes. Culling by what is
      // on screen is correct on a still graph and a strobe on a moving one:
      // an edge whose end crosses the frame boundary is drawn, dropped, drawn
      // again on alternate frames, and a few hundred of those flickering at
      // once is what read as "the lines are not fixed".
      if (physics.current?.hot()) return true;
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
      const focus = focused.current;
      if (focus) {
        // Both directions in the accent, told apart by weight rather than by
        // two competing inks. A hub's two hundred rays have to be legible as a
        // fan, and two colours at that density read as a mess.
        if (source === focus.anchor && focus.uses.has(target)) {
          return {
            ...data,
            type: "arrow",
            color: canvas.accent,
            size: ink.size * 1.7 * edgeScale.current,
            zIndex: 2,
          };
        }
        if (target === focus.anchor && focus.usedBy.has(source)) {
          return {
            ...data,
            color: mix(canvas.accent, canvas.background, 0.72),
            size: ink.size * 1.2 * edgeScale.current,
            zIndex: 2,
          };
        }
        // Everything the anchor does not touch recedes to a fifth. Width is
        // left alone: at one pixel there is nowhere for it to go, and it is
        // the ink that was competing.
        return {
          ...data,
          color: towards(ink.color, 1 - (1 - DIMMED) * fade.current),
          size: ink.size * edgeScale.current,
        };
      }
      return { ...data, color: ink.color, size: ink.size * edgeScale.current };
    });

    renderer.setSetting("defaultDrawNodeLabel", labelDrawer(canvas, zoomAlpha));
    renderer.setSetting("defaultDrawNodeHover", hoverDrawer(canvas, zoomAlpha));
    renderer.setSetting("defaultEdgeColor", ink.color);
    renderer.refresh();
  }, [graph, index, visible, selected, colourBy, edgeMode, theme]);

  useEffect(() => {
    const renderer = sigma.current;
    if (!renderer) return;
    const mode = LABEL_MODES[labelMode];
    labelZoom.current = mode.zoom;
    zoomAlpha.current = readZoom(renderer.getCamera().ratio);
    renderer.setSetting("renderLabels", labelMode !== "none");
    renderer.setSetting("labelRenderedSizeThreshold", mode.threshold);
    renderer.setSetting("labelDensity", mode.density);
    // A full refresh, deliberately: which labels get drawn is decided from
    // sigma's label grid, and `skipIndexation` is exactly what skips
    // rebuilding it — so the setting landed but nothing changed on screen
    // until some other interaction forced a real refresh.
    renderer.refresh();
  }, [labelMode, readZoom]);

  // Clicking a node no longer moves the camera.
  //
  // It used to frame the whole neighbourhood, on the reasoning that an
  // off-screen selection reads as a no-op. On a live layout that reasoning
  // inverts: the camera lurches on every click, and since the answer to the
  // click is a change of *colour* — the neighbourhood lights, the rest fades —
  // it was already visible wherever you were. Moving the ground under the
  // reader to show them something they could already see is the worst trade
  // in the file.
  //
  // Selecting from the search dialog is the one case that genuinely needs the
  // camera, and it is left to be solved on its own terms rather than by
  // moving the view on every click.

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
