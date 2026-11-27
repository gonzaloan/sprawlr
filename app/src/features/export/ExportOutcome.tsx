import { useEffect, useRef } from "react";
import { checkLine, shortList } from "../../domain";
import type { ExportResult } from "../../platform/backend";

/** What an export wrote, what to fill in, what it left out, and how to check it. */
export function ExportOutcome({ result }: { result: ExportResult }) {
  const { summary: s, dir } = result;
  const inputs = shortList(s.inputs, 6);
  const left = shortList(s.left_out, 4);
  const heading = useRef<HTMLHeadingElement>(null);
  // The button that started the export is gone: the result takes the focus, and is read out.
  useEffect(() => heading.current?.focus(), []);
  return (
    <div className="export__outcome">
      <h3 ref={heading} tabIndex={-1} className="export__done">
        The code is written
      </h3>
      <p>
        <strong>
          {s.resources} {s.resources === 1 ? "resource" : "resources"} of {s.title}
        </strong>{" "}
        written to <code>{dir}</code>, with the documentation.
      </p>
      {s.inputs.length > 0 && (
        <section>
          <h3>
            Fill in {s.inputs.length} {s.inputs.length === 1 ? "value" : "values"} first
          </h3>
          <p className="muted small">sprawlr never writes them. Set them in terraform/terraform.tfvars, or as CloudFormation parameters.</p>
          <ul className="export__list">
            {inputs.shown.map((i, k) => (
              <li key={k}>
                {i.resource}: <code>{i.value}</code> <span className="muted">{i.why}</span>
              </li>
            ))}
          </ul>
          {inputs.more > 0 && <p className="muted small">And {inputs.more} more, listed in README.md.</p>}
        </section>
      )}
      {s.left_out.length > 0 && (
        <section>
          <h3>Left out ({s.left_out.length})</h3>
          <ul className="export__list">
            {left.shown.map((l) => (
              <li key={l.node}>
                <strong>{l.name}</strong>: {l.reason}
              </li>
            ))}
          </ul>
          {left.more > 0 && <p className="muted small">And {left.more} more, listed in README.md.</p>}
        </section>
      )}
      {s.notes.length > 0 && (
        <section>
          <h3>Before CloudFormation takes it</h3>
          <ul className="export__list">
            {s.notes.map((n) => (
              <li key={n}>{n}</li>
            ))}
          </ul>
        </section>
      )}
      <section>
        <h3>Check it before you trust it</h3>
        <p>{checkLine(s)} README.md has the commands, for CloudFormation too.</p>
      </section>
    </div>
  );
}
