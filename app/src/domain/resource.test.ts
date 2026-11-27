import { describe, expect, it } from "vitest";
import demo from "../demo/demo-snapshot.json";
import {
  accessLines,
  accessOf,
  accessState,
  allowedIn,
  buildIndex,
  changesAbout,
  conditionWords,
  displayName,
  lastUsedOf,
  managedPolicyNote,
  principalLabel,
  purposeOf,
  rawAttrs,
  runsAs,
  settingsOf,
  takenOnBy,
  trustOf,
  unlistedManaged,
  type ArchNode,
  type Principal,
  type Snapshot,
} from ".";

const snap = demo as unknown as Snapshot;
const ix = buildIndex(snap);
const byName = (name: string) => snap.nodes.find((n) => n.name === name || displayName(n) === name)!;
const names = (ns: ArchNode[]) => ns.map(displayName);
const role = (attrs: Record<string, unknown>): ArchNode => ({
  id: "arn:aws:iam::111122223333:role/r",
  kind: { type: "iam_role" },
  name: "r",
  account: "111122223333",
  region: null,
  parent: null,
  aliases: [],
  app: null,
  env: null,
  tags: {},
  attrs,
  health: "unknown",
});
const p = (kind: Principal["kind"], id: string, conditions: string[] = []): Principal => ({ kind, id, conditions });

describe("who can take a role on", () => {
  it("reads the trust policy kept on the role", () => {
    expect(trustOf(byName("checkout-api-task-role"))).toEqual([p("service", "ecs-tasks.amazonaws.com", ["aws:SourceAccount"])]);
  });

  it("says nothing, rather than nobody, for a role scanned before trust policies were read", () => {
    expect(trustOf(role({ policies: [] }))).toBeNull();
  });

  it("names principals the way a person running the system would", () => {
    const r = role({});
    expect(principalLabel(p("service", "lambda.amazonaws.com"), r)).toBe("Lambda functions");
    expect(principalLabel(p("service", "ecs-tasks.amazonaws.com"), r)).toBe("ECS tasks");
    expect(principalLabel(p("service", "foo.amazonaws.com"), r)).toBe("The foo service");
    expect(principalLabel(p("account", "111122223333"), r)).toBe("This account");
    expect(principalLabel(p("account", "444455556666"), r)).toBe("Account 444455556666");
    expect(principalLabel(p("role", "arn:aws:iam::444455556666:role/ops/deployer"), r)).toBe("Role deployer in account 444455556666");
    expect(principalLabel(p("role", "arn:aws:iam::111122223333:role/deployer"), r)).toBe("Role deployer");
    expect(principalLabel(p("user", "444455556666"), r)).toBe("Users of account 444455556666");
    expect(principalLabel(p("federated", "arn:aws:iam::111122223333:oidc-provider/token.actions.githubusercontent.com"), r)).toBe("GitHub Actions");
    expect(principalLabel(p("federated", "arn:aws:iam::111122223333:saml-provider/Okta"), r)).toBe("SAML sign-in through Okta");
    expect(principalLabel(p("federated", "cognito-identity.amazonaws.com"), r)).toBe("Cognito identity pools");
    expect(principalLabel(p("deleted", "AROAEXAMPLE"), r)).toBe("A role or user that no longer exists");
    expect(principalLabel(p("anyone", "*"), r)).toBe("Anyone with AWS credentials");
  });

  it("shortens an identity provider's condition keys to the claim they check", () => {
    expect(conditionWords(p("federated", "x", ["token.actions.githubusercontent.com:aud", "token.actions.githubusercontent.com:sub"]))).toBe("aud and sub");
    expect(conditionWords(p("account", "444455556666", ["sts:ExternalId"]))).toBe("sts:ExternalId");
  });

  it("lists what in this scan takes a role on, and what a resource runs as", () => {
    expect(names(takenOnBy(ix, byName("checkout-api-task-role").id))).toEqual(["checkout-api"]);
    const prodApi = "arn:aws:ecs:us-east-1:111122223333:service/prod-cluster/checkout-api";
    expect(names(runsAs(ix, prodApi))).toEqual(["checkout-api-task-role"]);
    expect(runsAs(ix, byName("checkout-orders-db").id)).toEqual([]);
  });
});

describe("what a role may do", () => {
  it("is one line per service, in words, with the resources of this scan it reaches", () => {
    const lines = accessLines(ix, byName("billing-invoice-role"));
    const tables = lines.find((l) => l.service === "DynamoDB")!;
    expect(tables.what).toBe("write");
    expect(names(tables.targets)).toEqual(["InvoicesTable"]);
    expect(tables.actions).toEqual(["dynamodb:PutItem", "dynamodb:UpdateItem"]);
    const queue = lines.find((l) => l.service === "SQS")!;
    expect(queue.what).toBe("read and write");
  });

  it("says when a grant covers every resource of a service", () => {
    const lines = accessLines(ix, byName("reports-worker-role"));
    expect(lines.find((l) => l.service === "DynamoDB")).toMatchObject({ what: "full access", scope: "every table" });
  });

  it("counts a known AWS managed policy as what its name says", () => {
    const [admin] = accessLines(ix, byName("ci-deploy-2023"));
    expect(admin).toMatchObject({ service: "Every service", what: "full access", scope: "every resource", via: ["AdministratorAccess, managed by AWS"] });
  });

  it("names resources outside this scan, and lists denies after allows", () => {
    const r = role({
      access: [
        { service: "s3", allow: false, levels: ["write"], actions: ["s3:DeleteObject"], except_actions: [], targets: [], everything: false, except_resources: [], elsewhere: ["arn:aws:s3:::archive/*"], conditional: false, policies: ["p"] },
        { service: "sqs", allow: true, levels: ["write"], actions: ["sqs:SendMessage"], except_actions: [], targets: [], everything: false, except_resources: [], elsewhere: ["arn:aws:sqs:us-east-1:1:jobs"], conditional: true, policies: ["p"] },
      ],
    });
    const lines = accessLines(ix, r);
    expect(lines.map((l) => l.allow)).toEqual([true, false]);
    expect(lines[0]).toMatchObject({ scope: "jobs, not in this scan", conditional: true });
    expect(lines[1].scope).toBe("archive/*, not in this scan");
  });

  it("names the role a Lambda function runs as and what that role allows", () => {
    const [role] = runsAs(ix, byName("InvoiceProcessor").id);
    expect(role.name).toBe("billing-invoice-role");
    const tables = accessLines(ix, role).find((l) => l.service === "DynamoDB")!;
    expect([tables.what, names(tables.targets)]).toEqual(["write", ["InvoicesTable"]]);
  });

  it("shows, for every role of the demo, who can take it on and what it may do", () => {
    const roles = snap.nodes.filter((n) => n.kind.type === "iam_role");
    expect(roles.length).toBeGreaterThan(3);
    for (const r of roles) expect([r.name, trustOf(r) !== null, accessOf(r) !== null]).toEqual([r.name, true, true]);
  });

  it("asks for a new scan when a role's policies were read before sprawlr summed them up", () => {
    expect(accessState(byName("checkout-api-task-role"))).toBe("known");
    expect(accessState(role({ policies: [{ name: "p", statements: [] }] }))).toBe("scan_again");
    expect(accessState(role({ placeholder: true }))).toBe("not_read");
  });

  it("lists only the AWS managed policies that no line already counts", () => {
    expect(unlistedManaged(byName("ci-deploy-2023"))).toEqual([]);
    expect(unlistedManaged(byName("billing-invoice-role"))).toEqual(["AWSLambdaBasicExecutionRole"]);
  });

  it("describes the common AWS managed policies in plain words", () => {
    expect(managedPolicyNote("AWSLambdaBasicExecutionRole")).toBe("writes its logs to CloudWatch Logs");
    expect(managedPolicyNote("SomeTeamPolicy")).toBeNull();
  });
});

describe("who IAM lets into a resource", () => {
  it("names each role that reaches it, by name or because it reaches every one", () => {
    const allowed = allowedIn(ix, byName("InvoicesTable").id);
    expect(allowed.map((a) => [a.role.name, a.what, a.scope])).toEqual([
      ["billing-invoice-role", "write", null],
      ["ci-deploy-2023", "full access", "every resource"],
      ["reports-worker-role", "full access", "every table"],
    ]);
  });

  it("does not list a role as allowed into itself", () => {
    expect(allowedIn(ix, byName("ci-deploy-2023").id)).toEqual([]);
  });
});

describe("how a resource is set up", () => {
  it("reads a table's settings as labelled values", () => {
    expect(settingsOf(byName("InvoicesTable"))).toEqual(
      expect.arrayContaining([
        { label: "Capacity mode", value: "On demand" },
        { label: "Point-in-time recovery", value: "On" },
      ]),
    );
  });

  it("flags plain-text secrets in a function's environment", () => {
    const s = settingsOf(byName("InvoiceProcessor"));
    expect(s).toEqual(expect.arrayContaining([{ label: "Runtime", value: "python3.13" }, { label: "Memory", value: "512 MB" }, { label: "Timeout", value: "60 s" }]));
    expect(s.find((x) => x.label === "Secrets in plain text")).toEqual({ label: "Secrets in plain text", value: "STRIPE_API_KEY", tone: "bad" });
  });

  it("says when a role was last used, in one place for every section", () => {
    expect(lastUsedOf(byName("checkout-api-task-role"))).toBe("Today, in us-east-1");
    expect(lastUsedOf(byName("ci-deploy-2023"))).toBe("410 days ago");
    expect(lastUsedOf(byName("InvoicesTable"))).toBeNull();
  });

  it("keeps the attributes as read for the JSON view, policies included", () => {
    const keys = Object.keys(rawAttrs(byName("reports-worker-role")));
    expect(keys).toContain("policies");
    expect(keys).not.toContain("access");
    expect(keys).not.toContain("trust");
  });

  it("says when a role was last used and where", () => {
    expect(settingsOf(byName("checkout-api-task-role"))).toEqual(expect.arrayContaining([{ label: "Last used", value: "Today, in us-east-1" }]));
    expect(settingsOf(byName("ci-deploy-2023"))).toEqual(expect.arrayContaining([{ label: "Last used", value: "410 days ago", tone: "bad" }]));
  });

  it("keeps out what other sections show, and words unknown settings plainly after the known ones", () => {
    const s = settingsOf({ ...byName("InvoiceProcessor"), attrs: { metrics: [], placeholder: true, access: [], restore_window_count: 5, encrypted: true } });
    expect(s).toEqual([
      { label: "Encrypted", value: "Yes" },
      { label: "Restore window count", value: "5" },
    ]);
  });
});

describe("the rest of the panel", () => {
  it("says what each kind of resource does", () => {
    expect(purposeOf(byName("checkout-api-task-role"))).toBe("Gives permissions to whatever takes it on.");
    expect(purposeOf(byName("InvoicesTable"))).toBe("Stores items by key.");
  });

  it("keeps the changes since the last scan that touch the resource", () => {
    const rows = changesAbout(snap, byName("InvoiceProcessor").id);
    expect(rows.length).toBeGreaterThan(0);
    expect(rows.every((r) => r.id === byName("InvoiceProcessor").id)).toBe(true);
  });
});
