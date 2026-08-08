/**
 * A colour says which language. That is all it says.
 *
 * An earlier version spent colour on state — magenta for a language KOG
 * cannot read, amber for a file with an unresolved import — and it was the
 * wrong trade twice over. Colour is a poor carrier for a claim that needs a
 * sentence ("this import resolved to nothing, on line 14"), and spending it
 * on state means it can no longer say the one thing you can read across a
 * whole graph at a glance: what this codebase is written in. The claims
 * belong in the inspector, in words, where they already are.
 *
 * The hues are the ones you already know — GitHub's language colours, which
 * every developer has been reading for a decade — lifted only where a value
 * is too dark to survive on a near-black background. Same hue, legible
 * surface: Go stays its cyan, TypeScript its deep blue, JavaScript its
 * yellow.
 */

export type Theme = "light" | "dark";

export type Swatch = { light: string; dark: string };

export function swatch(value: Swatch, theme: Theme): string {
  return theme === "light" ? value.light : value.dark;
}

/**
 * Keys match `Node::lang` from the scan exactly. Where light and dark differ
 * it is only in lightness: the dark column exists because `#701516` (Ruby)
 * and `#1D365D` (Less) are invisible on `#0d0d0f`, not because the language
 * changes colour.
 */
const LANGUAGE: Record<string, Swatch> = {
  typescript: { light: "#3178c6", dark: "#4a92e0" },
  javascript: { light: "#c9a800", dark: "#f1e05a" },
  python: { light: "#3572a5", dark: "#5b9bd5" },
  go: { light: "#0091b3", dark: "#00add8" },
  rust: { light: "#b7643a", dark: "#dea584" },
  java: { light: "#b07219", dark: "#d69b4a" },
  csharp: { light: "#178600", dark: "#3fb52a" },
  ruby: { light: "#a01f21", dark: "#e0484b" },
  php: { light: "#4f5d95", dark: "#7f8dc4" },
  c: { light: "#5a5a5a", dark: "#9a9a9a" },
  cpp: { light: "#d0335f", dark: "#f34b7d" },
  html: { light: "#e34c26", dark: "#f4713f" },
  css: { light: "#663399", dark: "#9a6fd4" },
  sass: { light: "#c6538c", dark: "#e07cae" },
  less: { light: "#1d365d", dark: "#5a7fb5" },
  stylus: { light: "#5c7d33", dark: "#8fb95e" },
  shell: { light: "#5aa32a", dark: "#89e051" },
  vue: { light: "#35996b", dark: "#41b883" },
  svelte: { light: "#e0350a", dark: "#ff5a2b" },
  astro: { light: "#c1440e", dark: "#ff7a45" },
  sql: { light: "#c47800", dark: "#e8a33d" },
  dockerfile: { light: "#38566b", dark: "#6f93ad" },
  make: { light: "#427819", dark: "#6faa3f" },
  markdown: { light: "#6a737d", dark: "#9aa3ad" },
};

/**
 * Community colours, in fixed order, biggest community first.
 *
 * The Tableau 10 ramp, which is what a decade of dashboards has trained
 * everyone to read as "these are different groups" — and, unlike a hash of a
 * folder name, it is a chosen order rather than a coincidence. Past the tenth
 * community the ramp does not repeat: everything after it is grey, because
 * an eleventh hue would only be distinguishable from one of the first ten by
 * accident.
 */
const COMMUNITY: Swatch[] = [
  { light: "#3d6fb4", dark: "#5b9bd5" },
  { light: "#d1701c", dark: "#f0913f" },
  { light: "#b8393b", dark: "#e8595c" },
  { light: "#4a9c95", dark: "#5fc4bc" },
  { light: "#4f9146", dark: "#6cbf5f" },
  { light: "#b59a2e", dark: "#ddc04a" },
  { light: "#9a5fa8", dark: "#bb85c8" },
  { light: "#c96f9a", dark: "#e895bb" },
  { light: "#8a6f5e", dark: "#a89282" },
  { light: "#7a7f8c", dark: "#9aa0ad" },
];

/** Past the tenth community, and for anything unassigned. */
const OUT_OF_PALETTE: Swatch = { light: "#a9a294", dark: "#5e5e5e" };

export function communityColour(community: number, theme: Theme): string {
  const slot = COMMUNITY[community];
  return calm(swatch(slot ?? OUT_OF_PALETTE, theme), theme);
}

export const COMMUNITY_SLOTS = COMMUNITY.length;

/**
 * A language with no entry of its own: named, never given an invented hue.
 *
 * Neutral against its own ground rather than against white — a cool grey on
 * cream reads as a different, unnamed language rather than as "no colour".
 */
const UNKNOWN_LANGUAGE: Swatch = { light: "#847d70", dark: "#8f8f8f" };

/** Not code. Drawn far enough back that it never competes with code. */
const ASSET: Swatch = { light: "#cdc4b2", dark: "#343434" };

/** Hex to HSL and back, so a shade can be taken without a colour library. */
function toHsl(hex: string): [number, number, number] {
  const value = Number.parseInt(hex.slice(1), 16);
  const r = ((value >> 16) & 0xff) / 255;
  const g = ((value >> 8) & 0xff) / 255;
  const b = (value & 0xff) / 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  if (max === min) return [0, 0, l];
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  const h =
    max === r
      ? ((g - b) / d + (g < b ? 6 : 0)) / 6
      : max === g
        ? ((b - r) / d + 2) / 6
        : ((r - g) / d + 4) / 6;
  return [h * 360, s, l];
}

function toHex(h: number, s: number, l: number): string {
  const k = (n: number) => (n + h / 30) % 12;
  const a = s * Math.min(l, 1 - l);
  const f = (n: number) =>
    l - a * Math.max(-1, Math.min(k(n) - 3, Math.min(9 - k(n), 1)));
  const channel = (n: number) =>
    Math.round(Math.min(1, Math.max(0, f(n))) * 255)
      .toString(16)
      .padStart(2, "0");
  return `#${channel(0)}${channel(8)}${channel(4)}`;
}

/**
 * Take a hue down to speaking volume.
 *
 * Set against Obsidian, whose nodes are one flat grey, our fully saturated
 * Tableau ramp is the loudest thing on the canvas — a thousand dots at full
 * chroma read as a mosaic, and the reader's eye goes to the colours instead of
 * to the shape they are arranged in. The information is worth keeping: a
 * colour answers "which part of the codebase is this" across the whole graph
 * at a glance, which is the one question Obsidian's graph cannot answer at all.
 * So the hue stays and only its insistence goes.
 *
 * Saturation comes down by a bit over a third, and lightness moves a step
 * toward the background — toward white on a light canvas, toward black on a
 * dark one. Hue is untouched, because hue is the channel carrying the meaning;
 * saturation and lightness were only ever carrying emphasis.
 */
const CALM_SATURATION: Record<Theme, number> = {
  // Not the same number in both, because the same chroma is not the same
  // loudness in both. A saturated dot on white is competing with a bright
  // surround and loses some of its force to it; the same dot on near-black has
  // nothing to compete with and reads as a light source. Matching the two by
  // using one figure left the dark theme visibly shoutier than the light one.
  light: 0.62,
  dark: 0.5,
};
const CALM_TOWARD_BACKGROUND = 0.14;

function calm(hex: string, theme: Theme): string {
  const [h, s, l] = toHsl(hex);
  const settled =
    theme === "dark"
      ? l * (1 - CALM_TOWARD_BACKGROUND)
      : l + (1 - l) * CALM_TOWARD_BACKGROUND;
  return toHex(h, s * CALM_SATURATION[theme], settled);
}

/**
 * A shade of the language's own colour, keyed on the folder.
 *
 * A repository that is 727 TypeScript files out of 863 draws as one flat
 * blue if language is the only channel — true, and useless. Lightness is
 * free (size carries degree, hue carries language), so it goes to the area
 * of the codebase: `apps/backend` and `apps/frontend` come out as two shades
 * of the same blue. Same hue still means same language, which is the
 * property that had to survive.
 *
 * Narrow on purpose: wide enough to separate two areas side by side, never
 * wide enough to be mistaken for another language.
 */
const SHADE_RANGE = 0.11;

function shade(hex: string, seed: string, theme: Theme): string {
  if (!seed) return hex;
  let hash = 0;
  for (let i = 0; i < seed.length; i++)
    hash = (hash * 31 + seed.charCodeAt(i)) >>> 0;
  const [h, s, l] = toHsl(hex);
  const offset = ((hash % 1000) / 500 - 1) * SHADE_RANGE;
  const floor = theme === "dark" ? 0.44 : 0.28;
  const ceiling = theme === "dark" ? 0.78 : 0.6;
  return toHex(h, s, Math.min(ceiling, Math.max(floor, l + offset)));
}

/** The folder a shade is keyed on: the first two path segments. */
export function shadeKey(id: string): string {
  const segments = id.split("/");
  if (segments.length <= 1) return "";
  return segments.slice(0, Math.min(2, segments.length - 1)).join("/");
}

export function languageColour(
  lang: string,
  isAsset: boolean,
  theme: Theme,
  seed = "",
): string {
  if (isAsset) return swatch(ASSET, theme);
  return calm(
    shade(swatch(LANGUAGE[lang] ?? UNKNOWN_LANGUAGE, theme), seed, theme),
    theme,
  );
}

/**
 * Canvas colours, resolved from the theme rather than read back out of CSS.
 *
 * They used to come from `getComputedStyle`, which made the canvas depend on
 * *when* the `data-theme` attribute landed — and React runs a child's effects
 * before its parent's, so the canvas read the previous theme. A value that
 * must be correct at a particular moment should not be fetched from the DOM.
 */
export const CANVAS: Record<
  Theme,
  {
    background: string;
    edgeMuted: string;
    label: string;
    focus: string;
    /**
     * The colour a read neighbourhood is drawn in.
     *
     * Not `focus`, which is the maximum-contrast ink — near-black on light,
     * near-white on dark. A hub with two hundred dependents drawn in maximum
     * contrast is two hundred black rays across the canvas, which is louder
     * than the thing it is pointing at. An accent carries the same "this one"
     * without the weight.
     */
    accent: string;
  }
> = {
  dark: {
    background: "#1c1c1c",
    edgeMuted: "#282828",
    label: "#dadada",
    focus: "#f2f2f2",
    accent: "#8b7cf6",
  },
  light: {
    background: "#fbf9f5",
    edgeMuted: "#f1ebe0",
    label: "#222222",
    focus: "#111111",
    accent: "#7250e8",
  },
};

/**
 * The ink an edge is drawn with, and how wide.
 *
 * Measured against Obsidian rather than chosen: their line is `#3f3f3f` on a
 * `#1e1e1e` background — a step of 33 out of 255, about 13% — and one CSS
 * pixel wide at every zoom. These are the same step against our own two
 * backgrounds. Edges are quieter here than the eye expects because there are
 * thousands of them; the surprise is that Obsidian's are not fainter than
 * ours ever were, only fewer per square inch.
 */
const EDGE_INK: Record<Theme, string> = {
  // Dark is Obsidian's `--graph-line` outright: `#3f3f3f` on `#1c1c1c`.
  //
  // Light is the same *step* — 37 of 255, their `#dadada` under `#ffffff` —
  // taken on our cream instead. Its warmth is deliberately about half what the
  // surface ramp would give at that lightness: a tinted plane reads as paper,
  // but a tinted hairline reads as a stain. Nearly neutral, on warm ground.
  dark: "#3f3f3f",
  light: "#d9d5cd",
};

/** Straight RGB blend, `weight` of `hex` over `onto`. */
export function mix(hex: string, onto: string, weight: number): string {
  const from = Number.parseInt(hex.slice(1), 16);
  const to = Number.parseInt(onto.slice(1), 16);
  const channel = (shift: number) => {
    const a = (from >> shift) & 0xff;
    const b = (to >> shift) & 0xff;
    return Math.round(b + (a - b) * weight)
      .toString(16)
      .padStart(2, "0");
  };
  return `#${channel(16)}${channel(8)}${channel(0)}`;
}

/**
 * How an edge is drawn: one colour, one width, whatever the graph.
 *
 * Both are Obsidian's, and both used to slide with edge count. That ladder
 * came from the era of the frozen hairball, where three thousand strokes
 * crossing the same few hundred pixels genuinely did read as fog — and it was
 * the right fix for a layout whose clusters packed until their members
 * touched. It is the wrong fix now. `linkDistance` at 250 against a collision
 * radius of 60 gives the strokes somewhere to be, so there is no fog left to
 * thin, and all the ladder still did was spend contrast:
 *
 *     3,121 edges → weight 0.70 → dark step 24/255 (9.4%), light 24.5 (9.6%)
 *     Obsidian                  → dark step 35/255 (13.7%), light 37 (14.5%)
 *
 * A third of the separation between an edge and the background, given away to
 * solve a problem the layout had already solved. Obsidian does not vary either
 * number with density at all, so neither do we. If a repository ten times
 * denser than `acme-saas` turns back into fog, that is a measurement away
 * and a lever can come back with a number attached — but it will not come back
 * on a guess.
 *
 * `size` is a width in CSS pixels, and it is real, which took two fixes.
 * Sigma's edge shader computes `max(size / sizeRatio, minEdgeThickness)`, and
 * `minEdgeThickness` defaults to **1.7** — so the old ladder walking `size`
 * from 0.4 down to 0.18 was clamped straight back to 1.7 px on every step and
 * changed nothing. And `sizeRatio` is `√cameraRatio`, so left alone an edge
 * thickens threefold as you zoom into a cluster. Obsidian's line is
 * `lineSizeMult / scale` in graph units — a constant width on screen at every
 * magnification — so the canvas multiplies by `√ratio` to cancel sigma's law
 * and leave the number below as the width actually drawn.
 *
 * The colour is **opaque** rather than translucent, and that is not a style
 * preference. Sigma's curved-edge program discards the alpha channel: an
 * `rgba(…, 0.05)` edge draws at full strength, which is why an earlier attempt
 * to thin the fog by dropping alpha from 0.55 to 0.05 changed nothing on
 * screen. Opaque strokes also overwrite instead of accumulating, so the value
 * chosen is exactly the value drawn, however many edges cross.
 */
export function edgeInk(theme: Theme): { color: string; size: number } {
  return { color: EDGE_INK[theme], size: 1 };
}

export type CanvasTheme = (typeof CANVAS)[Theme] & { mode: Theme };

export function canvasTheme(theme: Theme): CanvasTheme {
  return { ...CANVAS[theme], mode: theme };
}
