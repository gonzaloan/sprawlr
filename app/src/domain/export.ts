// Export as code: where it is offered, and how its result reads. The export itself runs in
// Rust (sprawlr-iac), with each resource's configuration read again from AWS.
import type { Place } from "./location";

/** What the desktop app reports once an export is written (sprawlr_iac::Summary). */
export interface ExportSummary {
  /** "checkout", or "the whole account". */
  title: string;
  resources: number;
  /** Of them, those written as Terraform too (the awscc provider lacks a few types). */
  terraform: number;
  groups: string[];
  /** Values the person fills in: sprawlr never writes them. */
  inputs: { resource: string; value: string; why: string }[];
  left_out: { node: string; name: string; reason: string }[];
  /** What to do before CloudFormation takes a template (it is too big). */
  notes: string[];
}

export interface ExportScope {
  /** null: the whole account. */
  app: string | null;
  title: string;
}

/** Code is owned per app or for the whole account; a flow or a single resource is not a unit of it. */
export function exportScopeOf(place: Place): ExportScope | null {
  if (place.kind === "overview") return { app: null, title: "the whole account" };
  if (place.kind === "app") return { app: place.name, title: place.name };
  return null;
}

export function checkLine(s: ExportSummary): string {
  if (s.resources === 0) return "Nothing was written as code: README.md says why for each resource.";
  const plan = `In terraform/, terraform plan should import ${s.terraform} ${s.terraform === 1 ? "resource" : "resources"} and change nothing.`;
  const rest = s.resources - s.terraform;
  return rest > 0 ? `${plan} ${rest} more ${rest === 1 ? "is" : "are"} only in CloudFormation, as README.md explains.` : plan;
}

/** An error from the export, as one sentence, with what to check when AWS is involved. */
export function exportError(error: unknown, demo: boolean): string {
  const text = String(error).replace(/^Error:\s*/, "").replace(/\.+$/, "");
  const fromAws = !demo && !/^there is no app named/.test(text) && !/already has files sprawlr did not write/.test(text);
  return `Could not export: ${text}.${fromAws ? " Check that the profile is signed in (aws sso login) and allows the OptionalExport actions in iam-policy.json." : ""}`;
}

/** The first `max` items, and how many the README lists beyond them. */
export function shortList<T>(items: T[], max: number): { shown: T[]; more: number } {
  return { shown: items.slice(0, max), more: Math.max(0, items.length - max) };
}
