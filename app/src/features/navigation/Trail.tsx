import type { Crumb, Whereabouts } from "../../domain";

interface Props {
  crumbs: Crumb[];
  canBack: boolean;
  canForward: boolean;
  onBack: () => void;
  onForward: () => void;
  onGo: (to: Whereabouts) => void;
}

/**
 * Back, forward, and the way from the whole account to where the person is. The step buttons
 * are aria-disabled rather than disabled, so a keyboard user's focus stays on them.
 */
export function Trail({ crumbs, canBack, canForward, onBack, onForward, onGo }: Props) {
  const last = crumbs.length - 1;
  return (
    <nav className="trail" aria-label="Where you are">
      <button className="trail__step" onClick={() => canBack && onBack()} aria-disabled={!canBack} aria-label="Back" title="Back">
        ‹
      </button>
      <button className="trail__step" onClick={() => canForward && onForward()} aria-disabled={!canForward} aria-label="Forward" title="Forward">
        ›
      </button>
      {crumbs.length > 1 && (
        <ol className="trail__crumbs">
          {crumbs.map((c, i) => (
            <li key={i}>
              {i < last ? (
                <button className="trail__link" onClick={() => onGo(c.to)}>
                  {c.label}
                </button>
              ) : (
                <span aria-current="location" title={c.label}>
                  {c.label}
                </span>
              )}
            </li>
          ))}
        </ol>
      )}
    </nav>
  );
}
