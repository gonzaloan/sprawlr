# Turn the account or an app into code you can check

Status: Accepted · Owner: gonzaloan · Date: 2026-11-27

## Problem

Teams that find their AWS account was built by hand (or by a stack nobody owns) want it under code: to review changes, to rebuild an environment, to stop drift. Writing that code means reading every resource's configuration in the console and copying it into Terraform or CloudFormation, one property at a time. It takes days for an app, and the result is never checked against what runs: a missed property shows up as a change on the first deploy, or as an outage.

## Outcome

From the whole account or one app, the person gets a folder with documentation, a Terraform project and CloudFormation templates for what runs there, written from each resource's live configuration. They can check it before trusting it: `terraform plan` imports every resource and changes nothing, and CloudFormation imports the same resources into a stack. No value that could be a secret is ever written; those become inputs the person fills in.

## Scope

**In:**
- Export the whole account (one file or template per app, plus `shared` for resources in no app) or one app.
- The folder:
  ```
  README.md                 what is here, how to check it, inputs to fill in, what was left out and why
  docs/                     the documentation pages, as "Save documentation" writes them
  terraform/
    providers.tf            the awscc provider, one per region
    variables.tf            inputs: values sprawlr never writes
    terraform.tfvars.example
    imports.tf              one import block per resource
    <app>.tf                the resources of each app
  cloudformation/
    <app>.json              one template per app and region, every resource kept on delete
    <app>.import.json       the resources to import, for an IMPORT change set
  ```
- What it reads: each resource's full configuration again, on demand, through Cloud Control (`GetResource`, the call a scan already makes), and each type's schema (`cloudformation:DescribeType`). Cloud Control adds no charge; the services behind some types (Secrets Manager, KMS) bill their own reads by the request. They need an optional IAM statement; when AWS refuses a read, the export says which resource and quotes AWS.
- Values never written, turned into inputs instead (sensitive Terraform variables, `NoEcho` parameters):
  - every value under environment variables, whatever their shape, unless it names another resource in the export, which becomes a reference;
  - values whose names suggest secrets (password, secret, token, credential, private key, API key, authorization), as properties or as `{ Key, Value }` and header pairs, and the values of CloudFront origin custom headers;
  - personal data: email addresses and phone numbers;
  - EC2 user data and SSM parameter values;
  - required properties AWS never returns (a Lambda function's code location).
- References: an ARN, URL or identifier of another resource in the export becomes a reference (`Ref` or `Fn::GetAtt` in the same template; an attribute in Terraform), so the code reads as a system and not as a dump. A reference that would close a cycle stays a literal.
- Left out, each with the reason in the README and the app:
  - resources a CloudFormation stack already manages (`aws:cloudformation:stack-name`): their code is that stack's template;
  - resources AWS owns: service-linked roles, AWS managed keys, the default event bus, default security groups;
  - resources whose configuration could not be read, or whose type AWS would not describe (without its schema, what AWS sets can't be told from what a template sets).

  What is not an AWS resource (the internet, outside services, regions) is simply not exported.
- Terraform without changes on import: properties AWS never returns are listed under `lifecycle { ignore_changes }`, so a plan never redeploys code from a guess.
- The CLI does the same: `sprawlr export --snapshot snapshot.json [--app checkout] --out folder [--profile p]`.

**Out (and why):**
- The `hashicorp/aws` provider. It needs a hand-written mapping per resource type. The `awscc` provider is generated from the same CloudFormation schemas sprawlr reads, so every resource has the same shape there.
- Exporting a flow or a single resource. A flow cuts across apps; code is owned per app. A resource's code in its panel can come later.
- Running `terraform`, `cfn-lint` or a change set. sprawlr is read-only; the README gives the commands.
- Companion resources that are not in the snapshot (ECS task definitions, listeners, target groups). They stay referenced by their identifier.
- Terraform modules per app. One root module keeps references across apps plain; files per app keep it readable.

## Experience

1. On the whole account or an app, the person picks **Export as code** in the top bar. A panel says:
   - what it writes: documentation, Terraform and CloudFormation;
   - what it reads: each resource's configuration again, which is free, through Cloud Control;
   - that values that could be secrets become inputs, never written.
2. They pick a folder: an empty one, or the folder of an earlier export, whose files are replaced (and nothing else in it). Progress shows each region as it is read, then the types' schemas.
3. The result:
   - says what was written: "23 resources of checkout written to ~/code/checkout";
   - lists the inputs to fill in ("InvoiceProcessor: STRIPE_API_KEY");
   - lists what was left out and why;
   - gives the commands to check it.
   
   Opening the folder shows the same in `README.md`.
4. When nothing could be read (credentials expired, permission missing), the panel says so in AWS's words and what to add. A partial read still writes what it could and lists the rest.

Browser demo: the button explains that the export runs in the desktop app and links to the demo account's export in the repository (`examples/demo-export`).

## Design

- **Domain (`crates/sprawlr-iac`, new, pure):**
  - parse a CloudFormation type schema (read-only, write-only and required properties, primary identifier, the shape of each property);
  - plan an export: names, groups, references, inputs, what to leave out;
  - render the Terraform files, the CloudFormation templates and the README.
  
  Attribute names follow the awscc provider's own algorithm, ported with its tests. The plan is deterministic: the same reads give the same files.
- **Port (`sprawlr-app`):** `ConfigSource::read_configs(wanted, progress)` returns each resource's CloudFormation type, identifier and properties, the schemas of those types, and the resources that could not be read. Use case `export_code(source, snapshot, scope, date, progress)` picks the resources, reads them, plans and returns the files. The demo implements the port with fictional configurations.
- **Adapters:**
  - `sprawlr-iac::select` maps a resource to its type and identifier; `sprawlr-aws/src/config.rs` calls `GetResource` and `DescribeType` with adaptive retry, and keeps who asked out of AWS's messages;
  - `sprawlr-fs::write_export` writes the folder, refusing paths that leave it before writing anything.
- **Drivers:** CLI `export`; Tauri `export_code`; frontend `features/export` through `useBackend().exportCode`.
- **Snapshot contract:** unchanged. The configuration read for an export never enters the snapshot or its history (ADR 0006).
- **Costs, permissions, privacy:**
  - Cloud Control and DescribeType add no charge; Secrets Manager and KMS bill their own reads by the request, a fraction of a cent for an account;
  - new optional statement `OptionalExport` in `iam-policy.json`;
  - the configuration is kept in memory and written only to the chosen folder.

## Acceptance criteria

- [x] Given the demo's checkout app, when it is exported, then `terraform/checkout.tf` has the queue, topic, role and Lambda function with their properties, each with an import block, and `cloudformation/checkout.json` has the same resources with `DeletionPolicy: Retain`.
- [x] Given a Lambda function whose environment holds `STRIPE_API_KEY`, then the export writes no value for it, and the key appears as a sensitive variable and a `NoEcho` parameter.
- [x] Given an environment variable whose value is the URL of a queue in the export, then it becomes a reference to that queue.
- [x] Given a resource tagged `aws:cloudformation:stack-name`, then it is left out with that stack's name and how to get its template.
- [x] Given two resources that refer to each other, then one reference stays a literal.
- [x] Given a resource AWS refuses to read, then the rest is written and the README quotes AWS's message for that resource.
- [x] Given the same reads twice, then the files are byte for byte the same.
- [x] Given the demo export, when `terraform validate` runs against the awscc provider, then it passes.
- [ ] Given the demo export, when `cfn-lint` runs on its templates, then it passes.
- [x] Given a path from outside the folder (`../x`), then `write_export` refuses it and writes nothing.
- [x] Given a folder with files sprawlr did not write, then the export refuses it; given the folder of an earlier export, then the files that export wrote and this one doesn't are removed, and nothing else.
- [x] Given an email subscription, a secret header or environment variables in any shape, then none of their values is written.

## Open questions

- Some Cloud Control read handlers may call actions the policy denies (`secretsmanager:GetSecretValue`). If one does, that resource is left out with AWS's message; the deny stays.
- Whether to add companion resources (task definitions, listeners) once the scan reads them.
