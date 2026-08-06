/**
 * What a colour means on the canvas.
 *
 * The old renderer gave every directory a hue off a hash — 360 possible
 * colours, none of them chosen. That is what made the graph look like a
 * decade-old dependency chart, and it quietly broke the one promise the
 * palette makes: on a real monorepo `apps/backend` hashed close enough to
 * the signal magenta that a whole NestJS tree read as unread code.
 *
 * So colour now carries state, not identity — position and labels already
 * carry identity, and they do it better. Four meanings, and each one has a
 * second channel so nothing rests on hue alone:
 *
 *   read      neutral, no ring          the ordinary case, and most of the graph
 *   gap       amber, thin ring          a file with an import KOG could not resolve
 *   not read  signal magenta, ring      a language KOG has no extractor for
 *   selected  focus blue, bigger, label what you are looking at
 *
 * Validated all-pairs with `dataviz/scripts/validate_palette.js` in both
 * modes, including under protanopia, deuteranopia and tritanopia: the
 * amber/magenta/blue triple clears the CVD floor (worst ΔE 8.4) and the
 * normal-vision floor (worst 17.0). The rings are what make it hold anyway.
 *
 * `colour by folder` is a deliberate second mode rather than the default.
 * It uses three slots that were validated together *and* against both
 * signals; a fourth would not survive, so a fourth folder is grey. A palette
 * that keeps generating hues past what it can distinguish is not a palette.
 */

export type NodeState = "read" | "gap" | "unread" | "asset";

export type Swatch = { light: string; dark: string };

/** The three folder slots, in fixed order. Never cycled, never generated. */
export const FOLDER_SLOTS: Swatch[] = [
  { light: "#eb6834", dark: "#d9762f" }, // orange
  { light: "#1baf7a", dark: "#2bb98a" }, // aqua
  { light: "#4a3aa7", dark: "#8c7fe0" }, // violet
];

/** Anything past the last slot, and every file in "colour by state" mode. */
export const NEUTRAL: Swatch = { light: "#8d90a0", dark: "#6f7382" };

export const STATE_COLOUR: Record<NodeState, Swatch> = {
  read: NEUTRAL,
  gap: { light: "#c07800", dark: "#e0a63f" },
  unread: { light: "#c9295f", dark: "#e0457b" },
  // An asset is not a state so much as an absence of one: it is drawn back
  // far enough that it never competes with code.
  asset: { light: "#c2c4cf", dark: "#3a3d48" },
};

export function swatch(value: Swatch, theme: "light" | "dark"): string {
  return theme === "light" ? value.light : value.dark;
}

/**
 * The folder a file is coloured by: its first two path segments, which is
 * the level that separates `apps/web` from `apps/api` — the split a reader
 * of a monorepo most needs to see. One segment lumps them together.
 */
export function folderKey(id: string): string {
  const segments = id.split("/");
  if (segments.length <= 1) return ".";
  return segments.slice(0, Math.min(2, segments.length - 1)).join("/");
}
