// Article font size and column width, remembered in localStorage. The width is in multiples of the font size.
import { createSignal } from "solid-js";

const FONT_KEY = "kletse-rss:font-size";
const WIDTH_KEY = "kletse-rss:width-factor";
const FONT = { initial: 18, min: 12, max: 32, step: 1 } as const;
const WIDTH = { initial: 40, min: 20, max: 80, step: 4 } as const;

function clamp(value: number, range: { min: number; max: number }): number {
  return Math.min(range.max, Math.max(range.min, value));
}

/** A positive number saved earlier, or the fallback (also when storage is unavailable). */
function stored(key: string, fallback: number): number {
  try {
    const value = Number(localStorage.getItem(key));
    return Number.isFinite(value) && value > 0 ? value : fallback;
  } catch {
    return fallback;
  }
}

function save(key: string, value: number): void {
  try {
    localStorage.setItem(key, String(value));
  } catch {
    // Storage unavailable (private mode, blocked): the size just isn't remembered.
  }
}

const [fontSize, setFontSizeSignal] = createSignal(stored(FONT_KEY, FONT.initial));
const [widthFactor, setWidthSignal] = createSignal(stored(WIDTH_KEY, WIDTH.initial));

export { fontSize, widthFactor };

function setFontSize(px: number): void {
  const size = setFontSizeSignal(clamp(px, FONT));
  document.documentElement.style.setProperty("--article-size", `${size}px`);
  save(FONT_KEY, size);
}

function setWidth(factor: number): void {
  const width = setWidthSignal(clamp(factor, WIDTH));
  document.documentElement.style.setProperty("--width-factor", String(width));
  save(WIDTH_KEY, width);
}

export function biggerFont(): void {
  setFontSize(fontSize() + FONT.step);
}

export function smallerFont(): void {
  setFontSize(fontSize() - FONT.step);
}

export function wider(): void {
  setWidth(widthFactor() + WIDTH.step);
}

export function narrower(): void {
  setWidth(widthFactor() - WIDTH.step);
}

export function resetSize(): void {
  setFontSize(FONT.initial);
  setWidth(WIDTH.initial);
}

/** Applies the remembered sizes to the page; called once at start. */
export function applyPrefs(): void {
  setFontSize(fontSize());
  setWidth(widthFactor());
}
