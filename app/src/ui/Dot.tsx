// A coloured status dot. Colour is never the only signal: with a label the dot is announced
// as an image; without one it only repeats text next to it and is hidden from screen readers.
import type { Health } from "../domain";

const HEALTH_TEXT: Record<Health, string> = { ok: "Healthy", warning: "Warning", alarm: "Alarm", unknown: "No health data" };

/** What a health dot means, in words. */
export const healthLabel = (h: Health) => HEALTH_TEXT[h];

export function Dot({ tone, label }: { tone: Health; label?: string }) {
  return label ? (
    <span className={`dot dot--${tone}`} role="img" aria-label={label} title={label} />
  ) : (
    <span className={`dot dot--${tone}`} aria-hidden="true" />
  );
}
