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

/** A language with no entry of its own: named, never given an invented hue. */
const UNKNOWN_LANGUAGE: Swatch = { light: "#7f8494", dark: "#8b90a0" };

/** Not code. Drawn far enough back that it never competes with code. */
const ASSET: Swatch = { light: "#c8cbd6", dark: "#2f323b" };

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
  return shade(swatch(LANGUAGE[lang] ?? UNKNOWN_LANGUAGE, theme), seed, theme);
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
    edge: string;
    edgeMuted: string;
    label: string;
    labelHalo: string;
    focus: string;
    /** The wave that runs out through a selection's dependents. */
    pulse: string;
  }
> = {
  dark: {
    background: "#0d0d0f",
    edge: "#3a3d47",
    edgeMuted: "#21232a",
    label: "#e8e8ea",
    labelHalo: "rgba(13,13,15,0.82)",
    focus: "#f4f5f8",
    pulse: "#8fb8ff",
  },
  light: {
    background: "#fbfbfd",
    edge: "#b4b8c4",
    edgeMuted: "#dcdee6",
    label: "#15151a",
    labelHalo: "rgba(251,251,253,0.86)",
    focus: "#101014",
    pulse: "#2f6fd0",
  },
};

export type CanvasTheme = (typeof CANVAS)[Theme] & { mode: Theme };

export function canvasTheme(theme: Theme): CanvasTheme {
  return { ...CANVAS[theme], mode: theme };
}
