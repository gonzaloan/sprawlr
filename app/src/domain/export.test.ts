import { describe, expect, it } from "vitest";
import { checkLine, exportError, exportScopeOf, shortList, type ExportSummary } from ".";

const summary = (resources: number, terraform = resources): ExportSummary => ({ title: "checkout", resources, terraform, groups: ["checkout"], inputs: [], left_out: [], notes: [] });

describe("exporting as code", () => {
  it("is offered for the whole account and for an app, the units code is owned by", () => {
    expect(exportScopeOf({ kind: "overview" })).toEqual({ app: null, title: "the whole account" });
    expect(exportScopeOf({ kind: "app", name: "checkout" })).toEqual({ app: "checkout", title: "checkout" });
    expect(exportScopeOf({ kind: "flow", id: "f" })).toBeNull();
    expect(exportScopeOf({ kind: "resource", id: "r" })).toBeNull();
    expect(exportScopeOf({ kind: "cleanup" })).toBeNull();
  });

  it("says what a check that the code matches the account looks like", () => {
    expect(checkLine(summary(23))).toBe("In terraform/, terraform plan should import 23 resources and change nothing.");
    expect(checkLine(summary(1))).toBe("In terraform/, terraform plan should import 1 resource and change nothing.");
    expect(checkLine(summary(0))).toBe("Nothing was written as code: README.md says why for each resource.");
    expect(checkLine(summary(12, 11))).toBe("In terraform/, terraform plan should import 11 resources and change nothing. 1 more is only in CloudFormation, as README.md explains.");
  });

  it("says what went wrong in one sentence, and what to check only when AWS is involved", () => {
    expect(exportError(new Error("AccessDenied: not authorized."), false)).toBe(
      "Could not export: AccessDenied: not authorized. Check that the profile is signed in (aws sso login) and allows the OptionalExport actions in iam-policy.json.",
    );
    expect(exportError("there is no app named x in this scan", false)).toBe("Could not export: there is no app named x in this scan.");
    expect(exportError("anything", true)).toBe("Could not export: anything.");
  });

  it("lists a few items and says how many more the README has", () => {
    expect(shortList([1, 2, 3], 5)).toEqual({ shown: [1, 2, 3], more: 0 });
    expect(shortList([1, 2, 3, 4, 5, 6], 4)).toEqual({ shown: [1, 2, 3, 4], more: 2 });
  });
});
