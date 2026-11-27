import { Fragment, useState } from "react";
import { rawAttrs, settingsOf, type ArchNode } from "../../domain";

/** How a resource is set up, in words, with the attributes as read one click away. */
export function Settings({ node }: { node: ArchNode }) {
  const [raw, setRaw] = useState(false);
  const settings = settingsOf(node);
  const attrs = Object.entries(rawAttrs(node));
  if (!settings.length && !attrs.length) return null;
  return (
    <section>
      <h3>Configuration</h3>
      {settings.length > 0 && (
        <dl className="facts facts--settings">
          {settings.map((s) => (
            <Fragment key={s.label}>
              <dt>{s.label}</dt>
              <dd className={s.tone === "bad" ? "is-bad" : undefined}>{s.value}</dd>
            </Fragment>
          ))}
        </dl>
      )}
      {attrs.length > 0 && (
        <>
          <button className="btn btn--link" onClick={() => setRaw(!raw)} aria-expanded={raw}>
            {raw ? "Hide JSON" : "Show as JSON"}
          </button>
          {raw && <pre className="raw">{JSON.stringify(Object.fromEntries(attrs), null, 2)}</pre>}
        </>
      )}
    </section>
  );
}
