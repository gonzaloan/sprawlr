import type { RefreshState } from "../../platform";

const time = (iso: string) => new Date(iso).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });

/** Text with `commands` in backticks, commands shown as selectable code. */
function WithCode({ text }: { text: string }) {
  return (
    <>
      {text.split("`").map((part, i) => (i % 2 ? <code key={i}>{part}</code> : <span key={i}>{part}</span>))}
    </>
  );
}

/** What the last or current refresh is doing, in words. */
export function RefreshLine({ refresh, readAt }: { refresh: RefreshState; readAt: string | null }) {
  const from = readAt ? ` The status below is from ${time(readAt)}.` : "";
  switch (refresh.state) {
    case "running":
      return (
        <p className="tray__refresh small" role="status">
          {refresh.step}…
        </p>
      );
    case "failed":
      return (
        <p className="tray__refresh tray__refresh--bad small" role="status">
          Refresh failed: {refresh.message}.{from}
          {refresh.background && refresh.next_at ? ` Next try at ${time(refresh.next_at)}.` : ""}
        </p>
      );
    case "sign_in_needed":
      return (
        <p className="tray__refresh tray__refresh--bad small" role="status">
          Can't sign in with {refresh.profile ?? "the default credentials"}. <WithCode text={refresh.hint} />
          {refresh.background && refresh.paused ? " Background refresh is paused until a refresh works." : ""}
          {from}
        </p>
      );
    default:
      return refresh.warnings > 0 ? (
        <p className="tray__refresh small" role="status">
          {refresh.warnings === 1 ? "1 part" : `${refresh.warnings} parts`} could not be read. Open sprawlr to see which.
        </p>
      ) : null;
  }
}
