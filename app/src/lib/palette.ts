/**
 * What a colour means on the canvas.
 *
 * Colour is always on, and it always says the same thing: **which language
 * this file is written in**. That is the one property of a file that is both
 * stable and worth seeing from across the room — a monorepo's shape is its
 * languages meeting, and this is what draws that meeting.
 *
 * Hues are the ones developers already carry in their heads (Go's cyan,
 * Rust's rust, Ruby's red, Python's blue-and-yellow split resolved to blue)
 * but harmonised: every one sits in the same chroma and lightness band per
 * theme, so no language shouts over another and the whole reads as one
 * palette rather than a wall of vendor logos.
 *
 * State does not take a colour — it takes a **ring**, drawn on top:
 *
 *   magenta ring   KOG cannot read this language; its own imports are missing
 *   amber ring     this file has an import KOG could not resolve
 *   pale ring      selected
 *
 * That separation is what lets both be true at once. A `.sql` file is the
 * colour of SQL *and* wears the ring that says nothing was read out of it,
 * where a single fill could only ever have said one of the two.
 */

export type Theme = "light" | "dark";

export type Swatch = { light: string; dark: string };

export function swatch(value: Swatch, theme: Theme): string {
  return theme === "light" ? value.light : value.dark;
}

/**
 * One entry per language KOG labels a node with, plus the common unread
 * ones. Keys match `Node::lang` from the scan exactly.
 */
const LANGUAGE: Record<string, Swatch> = {
  typescript: { light: "#2f6fd0", dark: "#5b9bf0" },
  javascript: { light: "#b8912a", dark: "#e3c04b" },
  tsx: { light: "#2f6fd0", dark: "#5b9bf0" },
  go: { light: "#0f8fa8", dark: "#3fc3dd" },
  rust: { light: "#b4622c", dark: "#e08a4e" },
  python: { light: "#3a6ea8", dark: "#6aa4e0" },
  java: { light: "#a8562f", dark: "#df8b5e" },
  csharp: { light: "#6b4bb8", dark: "#a888ec" },
  ruby: { light: "#b83c46", dark: "#ef7078" },
  php: { light: "#6a5bb5", dark: "#9c8ee8" },
  c: { light: "#5a7290", dark: "#8fa9c6" },
  cpp: { light: "#3f6f9a", dark: "#74a5cf" },
  html: { light: "#c25a29", dark: "#ef8b52" },
  css: { light: "#2b7fb8", dark: "#5fb4e6" },
  sass: { light: "#b8578f", dark: "#e88dbd" },
  less: { light: "#3f5f9a", dark: "#7d9ad6" },
  stylus: { light: "#6f8f3f", dark: "#a6c46e" },
  vue: { light: "#2f9270", dark: "#55c79c" },
  svelte: { light: "#c2502c", dark: "#f08154" },
  astro: { light: "#8a4fc4", dark: "#b98af0" },
  shell: { light: "#4f8a4f", dark: "#7cc07c" },
  sql: { light: "#a86a2c", dark: "#dba055" },
  dockerfile: { light: "#2f7fb8", dark: "#5fb0e6" },
  make: { light: "#7a7f8c", dark: "#a9aebc" },
};

/** A language with no entry: named, but never given an invented hue. */
const UNKNOWN_LANGUAGE: Swatch = { light: "#7f8494", dark: "#8b90a0" };

/** Not code. Drawn far enough back that it never competes with code. */
const ASSET: Swatch = { light: "#cdd0da", dark: "#33363f" };

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
  const f = (n: number) => l - a * Math.max(-1, Math.min(k(n) - 3, Math.min(9 - k(n), 1)));
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
 * free here (size already carries degree, hue carries language), so it goes
 * to the area of the codebase: `apps/backend` and `apps/frontend` come out
 * as two shades of the same blue. Same hue still means same language, which
 * is the property that had to survive.
 *
 * The range is deliberately narrow: wide enough to separate two areas side
 * by side, never wide enough to be mistaken for another language.
 */
const SHADE_RANGE = 0.13;

function shade(hex: string, seed: string, theme: Theme): string {
  if (!seed) return hex;
  let hash = 0;
  for (let i = 0; i < seed.length; i++) hash = (hash * 31 + seed.charCodeAt(i)) >>> 0;
  const [h, s, l] = toHsl(hex);
  // −1…1, so a folder is as likely to come out lighter as darker.
  const offset = ((hash % 1000) / 500 - 1) * SHADE_RANGE;
  const floor = theme === "dark" ? 0.42 : 0.3;
  const ceiling = theme === "dark" ? 0.78 : 0.62;
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

export function hasLanguageColour(lang: string): boolean {
  return lang in LANGUAGE;
}

/**
 * Canvas colours, resolved from the theme rather than read back out of CSS.
 *
 * They used to come from `getComputedStyle`, which made the canvas depend on
 * *when* the `data-theme` attribute landed — and React runs a child's effects
 * before its parent's, so the canvas read the previous theme and stayed the
 * wrong colour until something else forced a repaint. A value that must be
 * correct at a particular moment should not be fetched from the DOM.
 */
export const CANVAS: Record<
  Theme,
  {
    background: string;
    edge: string;
    edgeMuted: string;
    label: string;
    labelHalo: string;
    signal: string;
    warn: string;
    focus: string;
  }
> = {
  dark: {
    background: "#0d0d0f",
    edge: "#3a3d47",
    edgeMuted: "#23252c",
    label: "#e8e8ea",
    labelHalo: "rgba(13,13,15,0.82)",
    signal: "#e0457b",
    warn: "#e0a63f",
    focus: "#f2f3f7",
  },
  light: {
    background: "#fbfbfd",
    edge: "#b9bcc8",
    edgeMuted: "#dcdee6",
    label: "#15151a",
    labelHalo: "rgba(251,251,253,0.86)",
    signal: "#c9295f",
    warn: "#b07400",
    focus: "#15151a",
  },
};

export type CanvasTheme = (typeof CANVAS)[Theme] & { mode: Theme };

export function canvasTheme(theme: Theme): CanvasTheme {
  return { ...CANVAS[theme], mode: theme };
}
