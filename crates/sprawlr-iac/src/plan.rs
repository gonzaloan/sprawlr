//! An export as a plan both formats render from: names, groups, references between resources,
//! inputs for the values sprawlr never writes (ADR 0006), and what was left out.

use crate::model::{ConfigRead, LeftOut, ResourceConfig, Scope};
use crate::naming;
use crate::schema::{guess_shape, Shape, Step, TypeSchema};
use crate::select::Selection;
use serde_json::{Number, Value};
use sprawlr_domain::analysis::display_name;
use sprawlr_domain::secrets::{named_like_secret, personal_data};
use sprawlr_domain::{Node, NodeId};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// A property value as the code writes it.
#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Bool(bool),
    Num(Number),
    Str(String),
    List(Vec<Val>),
    /// An object, with how Terraform writes it ([`Shape::Attrs`], [`Shape::Map`] or [`Shape::Json`]).
    Obj(Shape, BTreeMap<String, Val>),
    /// A value the person fills in ([`Export::inputs`]).
    Input(usize),
    Ref(Reference),
}

/// Another resource of the export, where its identifier or one of its attributes was written.
#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    pub target: usize,
    pub attr: Attr,
    /// The value AWS returned, written where a reference cannot be (another template).
    pub literal: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attr {
    /// The property that identifies the target (`Ref` in CloudFormation).
    Primary(String),
    /// A property AWS sets (`Fn::GetAtt` in CloudFormation).
    ReadOnly(String),
}

impl Attr {
    pub fn property(&self) -> &str {
        match self {
            Attr::Primary(p) | Attr::ReadOnly(p) => p,
        }
    }
}

/// Why a value is left for the person to fill in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputReason {
    /// The value of an environment variable.
    Environment(String),
    /// A value whose name says it may be a secret (a property, a header, a parameter).
    Secret(String),
    /// An email address or a phone number, under this property.
    Personal(String),
    UserData,
    ParameterValue,
    /// A property AWS accepts and never returns.
    NotReturned(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Input {
    pub resource: usize,
    pub reason: InputReason,
    /// CloudFormation parameter name, unique in its template.
    pub parameter: String,
    /// Terraform variable name, unique in the project.
    pub variable: String,
    pub sensitive: bool,
}

#[derive(Debug, Clone)]
pub struct Planned {
    pub node: NodeId,
    pub name: String,
    /// The app it belongs to, as a file name, or the group of resources in no app.
    pub group: String,
    pub region: String,
    pub type_name: String,
    /// The Cloud Control identifier (Terraform imports by it).
    pub identifier: String,
    /// The properties that identify it, with their values (CloudFormation imports by them).
    pub identifier_parts: Vec<(String, String)>,
    pub logical_id: String,
    pub tf_type: String,
    pub tf_name: String,
    /// The awscc provider has a resource for its type.
    pub in_terraform: bool,
    pub properties: BTreeMap<String, Val>,
    /// Top-level properties written although AWS never returns them: Terraform must not
    /// compare them with the account.
    pub ignore_changes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Export {
    pub scope: Scope,
    pub account: Option<String>,
    pub date: String,
    pub default_region: String,
    pub resources: Vec<Planned>,
    pub inputs: Vec<Input>,
    pub left_out: Vec<LeftOut>,
}

/// CloudFormation's limits per template.
const MAX_RESOURCES: usize = 500;
const MAX_PARAMETERS: usize = 200;

impl Export {
    /// "checkout", or "the whole account".
    pub fn title(&self) -> String {
        match &self.scope {
            Scope::Account => "the whole account".into(),
            Scope::App(a) => a.clone(),
        }
    }

    /// A template per app and region: `checkout`, or `checkout-us-west-2` when the app spans
    /// several regions.
    pub fn template_of(&self, i: usize) -> String {
        let r = &self.resources[i];
        let regions: BTreeSet<&str> = self.resources.iter().filter(|o| o.group == r.group).map(|o| o.region.as_str()).collect();
        if regions.len() > 1 {
            format!("{}-{}", r.group, r.region)
        } else {
            r.group.clone()
        }
    }

    /// Each template's resources, by template name.
    pub fn templates(&self) -> BTreeMap<String, Vec<usize>> {
        let mut out: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for i in 0..self.resources.len() {
            out.entry(self.template_of(i)).or_default().push(i);
        }
        out
    }

    pub fn tf_address(&self, i: usize) -> String {
        format!("{}.{}", self.resources[i].tf_type, self.resources[i].tf_name)
    }

    /// Resources written as Terraform too, not only as CloudFormation.
    pub fn in_terraform(&self) -> Vec<usize> {
        (0..self.resources.len()).filter(|&i| self.resources[i].in_terraform).collect()
    }

    pub fn groups(&self) -> Vec<String> {
        self.resources.iter().map(|r| r.group.clone()).collect::<BTreeSet<_>>().into_iter().collect()
    }

    /// What the person must know before using the code: templates CloudFormation won't take
    /// as they are.
    pub fn notes(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (t, members) in self.templates() {
            let parameters = self.inputs.iter().filter(|i| members.contains(&i.resource)).count();
            if members.len() > MAX_RESOURCES {
                out.push(format!(
                    "cloudformation/{t}.json has {} resources; a stack takes at most {MAX_RESOURCES}. Split it before importing.",
                    members.len()
                ));
            }
            if parameters > MAX_PARAMETERS {
                out.push(format!(
                    "cloudformation/{t}.json has {parameters} parameters; a template takes at most {MAX_PARAMETERS}. Split it, or set some values in the template."
                ));
            }
        }
        out
    }
}

/// The plan for a selection and what was read for it. The same reads give the same plan.
pub fn plan(scope: &Scope, nodes: &[Node], selection: Selection, read: ConfigRead, date: &str) -> Export {
    let by_id: HashMap<&NodeId, &Node> = nodes.iter().map(|n| (&n.id, n)).collect();
    let schemas: BTreeMap<String, TypeSchema> =
        read.schemas.iter().filter_map(|(t, raw)| TypeSchema::parse(raw).map(|s| (t.clone(), s))).collect();
    let name_of = |id: &NodeId| by_id.get(id).map(|n| display_name(n)).unwrap_or_else(|| id.0.clone());

    let mut left_out = selection.left_out;
    for nr in read.not_read {
        left_out.push(LeftOut {
            name: name_of(&nr.node),
            reason: format!("Its configuration could not be read: {}", nr.reason),
            node: nr.node,
        });
    }
    let shared = shared_group(nodes);
    let mut drafts = Vec::new();
    for c in read.configs {
        let Some(n) = by_id.get(&c.node) else { continue };
        let Some(tf_type) = naming::terraform_type(&c.type_name) else { continue };
        if !schemas.contains_key(&c.type_name) {
            // Without it, what AWS sets by itself can't be told from what a template sets.
            let reason = format!("AWS did not describe its type, {}, so sprawlr can't tell what to write.", c.type_name);
            left_out.push(LeftOut { name: name_of(&c.node), reason, node: c.node });
            continue;
        }
        let region = region_from(&c).or_else(|| n.region.clone());
        drafts.push(Draft { name: display_name(n), group: group_of(scope, n, &shared), region, tf_type, config: c });
    }
    drafts.sort_by(|a, b| {
        (&a.group, &a.config.type_name, &a.name, &a.config.node).cmp(&(&b.group, &b.config.type_name, &b.name, &b.config.node))
    });

    let default_region = most_common(drafts.iter().filter_map(|d| d.region.clone())).unwrap_or_else(|| "us-east-1".into());
    let mut export = Export {
        scope: scope.clone(),
        account: nodes.iter().find_map(|n| n.account.clone()),
        date: date.to_string(),
        default_region,
        resources: Vec::new(),
        inputs: Vec::new(),
        left_out,
    };
    let mut names = name_resources(&mut export, &drafts);
    write_properties(&mut export, &drafts, &schemas, &mut names);
    export
}

struct Draft {
    name: String,
    group: String,
    region: Option<String>,
    tf_type: String,
    config: ResourceConfig,
}

/// Names taken so far: logical ids and parameters per template, Terraform variables.
struct Names {
    per_template: HashMap<String, BTreeSet<String>>,
    variables: BTreeSet<String>,
}

/// Names come first: references and inputs need every resource's.
fn name_resources(export: &mut Export, drafts: &[Draft]) -> Names {
    let mut tf_taken: HashMap<String, BTreeSet<String>> = HashMap::new();
    for d in drafts {
        let resource = d.config.type_name.rsplit("::").next().unwrap_or("Resource");
        export.resources.push(Planned {
            node: d.config.node.clone(),
            name: d.name.clone(),
            group: d.group.clone(),
            region: d.region.clone().unwrap_or_else(|| export.default_region.clone()),
            type_name: d.config.type_name.clone(),
            identifier: d.config.identifier.clone(),
            identifier_parts: Vec::new(),
            logical_id: String::new(),
            tf_type: d.tf_type.clone(),
            in_terraform: naming::in_awscc(&d.config.type_name),
            tf_name: naming::unique(
                naming::terraform_name(&d.name, &naming::terraform_attribute(resource)),
                tf_taken.entry(d.tf_type.clone()).or_default(),
                "_",
            ),
            properties: BTreeMap::new(),
            ignore_changes: Vec::new(),
        });
    }
    let mut names = Names { per_template: HashMap::new(), variables: BTreeSet::new() };
    for i in 0..export.resources.len() {
        let template = export.template_of(i);
        let resource = export.resources[i].type_name.rsplit("::").next().unwrap_or("Resource").to_string();
        let taken = names.per_template.entry(template).or_default();
        export.resources[i].logical_id = naming::unique(naming::logical_id(&export.resources[i].name, &resource), taken, "");
    }
    names
}

/// Each resource's properties as the code writes them, then what AWS requires and never returns.
fn write_properties(export: &mut Export, drafts: &[Draft], schemas: &BTreeMap<String, TypeSchema>, names: &mut Names) {
    let index = reference_index(drafts, schemas);
    let mut deps: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); drafts.len()];
    for (i, d) in drafts.iter().enumerate() {
        let Some(schema) = schemas.get(&d.config.type_name) else { continue };
        let template = export.template_of(i);
        let mut w = Writer {
            current: i,
            type_name: &d.config.type_name,
            schema,
            index: &index,
            deps: &mut deps,
            inputs: &mut export.inputs,
            resource_logical: export.resources[i].logical_id.clone(),
            resource_tf: export.resources[i].tf_name.clone(),
            parameters: names.per_template.entry(template).or_default(),
            variables: &mut names.variables,
        };
        let mut body = match w.value(&d.config.properties, &[], Ctx::default()) {
            Some(Val::Obj(_, m)) => m,
            _ => BTreeMap::new(),
        };
        for req in &schema.required {
            if !body.contains_key(req) {
                let filled = w.not_returned(req, &d.config.properties);
                body.insert(req.clone(), filled);
            }
        }
        let r = &mut export.resources[i];
        r.ignore_changes = schema.write_only_top().into_iter().filter(|p| body.contains_key(*p)).map(String::from).collect();
        r.identifier_parts = identifier_parts(schema, &d.config);
        r.properties = body;
    }
}

/// Resources in no app share a group, named so no app's name can take it.
fn shared_group(nodes: &[Node]) -> String {
    let apps: BTreeSet<String> = nodes.iter().filter_map(|n| n.app.as_ref()).map(|a| slug(&a.value)).collect();
    ["shared", "no-app", "shared-resources"].into_iter().find(|g| !apps.contains(*g)).unwrap_or("no-app").to_string()
}

fn group_of(scope: &Scope, n: &Node, shared: &str) -> String {
    match scope {
        Scope::App(app) => slug(app),
        Scope::Account => n.app.as_ref().map(|a| slug(&a.value)).unwrap_or_else(|| shared.to_string()),
    }
}

/// Where a resource lives when the scan doesn't say: a bucket's regional host name names it
/// (`orders.s3.eu-west-1.amazonaws.com`).
fn region_from(c: &ResourceConfig) -> Option<String> {
    if c.type_name != "AWS::S3::Bucket" {
        return None;
    }
    let host = c.properties.get("RegionalDomainName")?.as_str()?;
    let region = host.split_once(".s3.")?.1.strip_suffix(".amazonaws.com")?;
    (!region.is_empty() && !region.contains('.')).then(|| region.to_string())
}

/// A file name for an app: lowercase letters, digits and dashes.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "app".into()
    } else {
        out
    }
}

fn most_common(values: impl Iterator<Item = String>) -> Option<String> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for v in values {
        *counts.entry(v).or_default() += 1;
    }
    let max = counts.values().copied().max()?;
    counts.into_iter().find(|(_, c)| *c == max).map(|(v, _)| v)
}

/// The values of a resource's identifying properties, from its Cloud Control identifier.
fn identifier_parts(s: &TypeSchema, c: &ResourceConfig) -> Vec<(String, String)> {
    let values: Vec<&str> = c.identifier.split('|').collect();
    if values.len() != s.primary_identifier.len() {
        return Vec::new();
    }
    s.primary_identifier.iter().cloned().zip(values.into_iter().map(String::from)).collect()
}

/// Values that name a resource of the export (its ARN, URL or identifier), and how.
fn reference_index(drafts: &[Draft], schemas: &BTreeMap<String, TypeSchema>) -> HashMap<String, Vec<(usize, Attr)>> {
    let mut index: HashMap<String, Vec<(usize, Attr)>> = HashMap::new();
    for (i, d) in drafts.iter().enumerate() {
        let Some(s) = schemas.get(&d.config.type_name) else { continue };
        let props = &d.config.properties;
        let mut add = |prop: &str, attr: Attr| {
            if let Some(v) = props.get(prop).and_then(Value::as_str).filter(|v| v.len() >= 3) {
                index.entry(v.to_string()).or_default().push((i, attr));
            }
        };
        if let [p] = s.primary_identifier.as_slice() {
            if !s.top_level_read_only().contains(&p.as_str()) {
                add(p, Attr::Primary(p.clone()));
            }
        }
        let mut ro = s.top_level_read_only();
        ro.sort_by_key(|p| *p != "Arn");
        for p in ro {
            add(p, Attr::ReadOnly(p.to_string()));
        }
    }
    index
}

/// Where a value sits with respect to environment variables.
#[derive(Debug, Clone, Default, PartialEq)]
enum Env {
    #[default]
    Outside,
    /// The value of a property that holds environment variables (`Environment`, `Variables`).
    Holder,
    /// Part of one environment variable's value: every string in it is left out.
    Var(String),
}

#[derive(Debug, Clone, Default)]
struct Ctx {
    json: bool,
    in_tags: bool,
    env: Env,
    /// The value of a pair whose name says it is a secret (a header, a parameter).
    secret: Option<String>,
}

struct Writer<'a> {
    current: usize,
    type_name: &'a str,
    schema: &'a TypeSchema,
    index: &'a HashMap<String, Vec<(usize, Attr)>>,
    deps: &'a mut Vec<BTreeSet<usize>>,
    inputs: &'a mut Vec<Input>,
    resource_logical: String,
    resource_tf: String,
    parameters: &'a mut BTreeSet<String>,
    variables: &'a mut BTreeSet<String>,
}

const NAMES_SOMETHING: [&str; 15] =
    ["name", "names", "id", "ids", "identifier", "arn", "arns", "role", "cluster", "bucket", "table", "queue", "topic", "target", "url"];
const AWS_ID_PREFIXES: [&str; 13] = ["vpc", "subnet", "sg", "igw", "nat", "eipalloc", "i", "vpce", "rtb", "tgw", "acl", "eni", "lt"];
/// Keys of a `{ Name, Value }` pair that name it rather than hold its value.
const PAIR_NAMES: [&str; 4] = ["Name", "Key", "Variable", "Type"];

fn names_something(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    NAMES_SOMETHING.iter().any(|e| k.ends_with(e))
}

/// `sg-0abc`, `subnet-1234`: an EC2 identifier, never anything else.
fn looks_like_aws_id(s: &str) -> bool {
    s.split_once('-').is_some_and(|(p, rest)| {
        AWS_ID_PREFIXES.contains(&p) && rest.len() >= 3 && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    })
}

/// An ARN, a URL or an EC2 identifier: it can only name one thing.
fn unmistakable(s: &str) -> bool {
    s.starts_with("arn:") || s.starts_with("https://") || looks_like_aws_id(s)
}

/// A host name. One AWS generates (`x.elb.amazonaws.com`) names its resource; one a person
/// chose (`www.example.com`) may also be a bucket's name and an alias.
fn host_name(s: &str) -> bool {
    s.contains('.') && !s.contains(char::is_whitespace) && s.chars().any(|c| c.is_ascii_alphabetic())
}

fn env_key(k: &str) -> bool {
    let k = k.to_ascii_lowercase();
    k == "environment" || k == "variables" || k.contains("environmentvariables")
}

fn is_aws_tag(item: &Value) -> bool {
    item.get("Key").and_then(Value::as_str).is_some_and(|k| k.starts_with("aws:"))
}

fn last_key(path: &[Step]) -> Option<&str> {
    path.iter().rev().find_map(|s| match s {
        Step::Key(k) => Some(k.as_str()),
        Step::Item => None,
    })
}

impl Writer<'_> {
    fn value(&mut self, v: &Value, path: &[Step], c: Ctx) -> Option<Val> {
        if !path.is_empty() && !c.json && self.schema.is_read_only(path) {
            return None;
        }
        match v {
            Value::Null => None,
            Value::Bool(b) => Some(Val::Bool(*b)),
            Value::Number(n) => Some(Val::Num(n.clone())),
            Value::String(s) => Some(self.string(s, path, &c)),
            Value::Array(items) => {
                let mut p = path.to_vec();
                p.push(Step::Item);
                let kept = items.iter().filter(|it| !(c.in_tags && is_aws_tag(it)));
                Some(Val::List(kept.filter_map(|it| self.value(it, &p, Ctx { secret: None, ..c.clone() })).collect()))
            }
            Value::Object(map) => Some(self.object(map, v, path, &c)),
        }
    }

    fn object(&mut self, map: &serde_json::Map<String, Value>, v: &Value, path: &[Step], c: &Ctx) -> Val {
        // A variable written as `{ Name, Value }` (ECS, CodeBuild): the pair, not a map of variables.
        let pair_name = map.get("Name").or_else(|| map.get("Variable")).and_then(Value::as_str).map(String::from);
        let shape = if path.is_empty() {
            Shape::Attrs
        } else if c.json || matches!(c.env, Env::Var(_)) {
            Shape::Json
        } else {
            match self.schema.shape(path) {
                s @ (Shape::Attrs | Shape::Map | Shape::Json) => s,
                // Names of variables are data: never guessed into attribute names.
                _ if c.env == Env::Holder && pair_name.is_none() => Shape::Map,
                _ => guess_shape(last_key(path), v),
            }
        };
        let header = map.get("HeaderName").and_then(Value::as_str);
        let secret_pair = map
            .get("Key")
            .or_else(|| map.get("Name"))
            .or_else(|| map.get("ParameterName"))
            .and_then(Value::as_str)
            .filter(|n| named_like_secret(n));
        let mut out = BTreeMap::new();
        for (k, x) in map {
            if c.in_tags && shape == Shape::Map && k.starts_with("aws:") {
                continue;
            }
            let mut p = path.to_vec();
            p.push(Step::Key(k.clone()));
            let env = match (&c.env, shape) {
                (Env::Var(name), _) => Env::Var(name.clone()),
                (Env::Holder, _) if pair_name.is_some() => {
                    if PAIR_NAMES.contains(&k.as_str()) {
                        Env::Outside
                    } else {
                        Env::Var(pair_name.clone().unwrap_or_default())
                    }
                }
                (Env::Holder, Shape::Map | Shape::Json) => Env::Var(k.clone()),
                _ if shape != Shape::Json && env_key(k) => Env::Holder,
                _ => Env::Outside,
            };
            let secret = match k.as_str() {
                "HeaderValue" => header.map(|h| format!("header {h}")),
                "Value" | "ParameterValue" => secret_pair.map(String::from),
                _ => None,
            };
            let child = Ctx { json: shape == Shape::Json, in_tags: c.in_tags || (k == "Tags" && shape != Shape::Json), env, secret };
            if let Some(val) = self.value(x, &p, child) {
                out.insert(k.clone(), val);
            }
        }
        Val::Obj(shape, out)
    }

    fn string(&mut self, s: &str, path: &[Step], c: &Ctx) -> Val {
        let key = last_key(path);
        if let Env::Var(var) = &c.env {
            return self.reference(s, true, false).unwrap_or_else(|| self.input(InputReason::Environment(var.clone()), true));
        }
        if let Some(name) = &c.secret {
            return self.input(InputReason::Secret(name.clone()), true);
        }
        if personal_data(s) {
            return self.input(InputReason::Personal(key.unwrap_or("Value").to_string()), true);
        }
        if c.in_tags {
            return Val::Str(s.to_string());
        }
        if self.type_name == "AWS::SSM::Parameter" && path == [Step::Key("Value".into())] {
            return self.input(InputReason::ParameterValue, true);
        }
        if key == Some("UserData") {
            return self.input(InputReason::UserData, true);
        }
        // In a policy document, `token.actions.githubusercontent.com:sub` is a condition key.
        if let Some(k) = key.filter(|k| named_like_secret(k) && !(c.json && k.contains(':'))) {
            return self.input(InputReason::Secret(k.to_string()), true);
        }
        let own_name = path.len() == 1 && key.is_some_and(|k| k.ends_with("Name") || self.schema.is_create_only(path));
        self.reference(s, key.is_some_and(names_something) && !own_name, own_name).unwrap_or_else(|| Val::Str(s.to_string()))
    }

    /// A reference to the resource `s` names, unless that would point at itself, be
    /// ambiguous, or close a cycle (neither format accepts one). An ARN, a URL or an EC2 id
    /// names one thing anywhere. A host name refers only to one AWS generated. A plain name
    /// refers only where a name is expected, to a resource identified by it (a function's, a
    /// table's), and never in a property that names the resource itself.
    fn reference(&mut self, s: &str, names_allowed: bool, own_name: bool) -> Option<Val> {
        if own_name {
            return None;
        }
        let (id, host) = (unmistakable(s), host_name(s));
        let candidates: Vec<&(usize, Attr)> = self
            .index
            .get(s)?
            .iter()
            .filter(|(_, a)| match a {
                Attr::Primary(_) => id || names_allowed,
                Attr::ReadOnly(_) => id || host,
            })
            .collect();
        let target = candidates.first()?.0;
        if candidates.iter().any(|(t, _)| *t != target) || target == self.current || self.reaches(target, self.current) {
            return None;
        }
        let attr = candidates.iter().map(|(_, a)| a).find(|a| matches!(a, Attr::Primary(_))).unwrap_or(&candidates[0].1).clone();
        self.deps[self.current].insert(target);
        Some(Val::Ref(Reference { target, attr, literal: s.to_string() }))
    }

    fn reaches(&self, from: usize, to: usize) -> bool {
        let mut seen = BTreeSet::new();
        let mut stack = vec![from];
        while let Some(n) = stack.pop() {
            if n == to {
                return true;
            }
            if seen.insert(n) {
                stack.extend(self.deps[n].iter().copied());
            }
        }
        false
    }

    fn input(&mut self, reason: InputReason, sensitive: bool) -> Val {
        let label = match &reason {
            InputReason::Environment(k) | InputReason::Secret(k) | InputReason::Personal(k) | InputReason::NotReturned(k) => k.clone(),
            InputReason::UserData => "UserData".into(),
            InputReason::ParameterValue => "Value".into(),
        };
        let parameter = naming::unique(format!("{}{}", self.resource_logical, naming::logical_id(&label, "Value")), self.parameters, "");
        let variable = naming::unique(format!("{}_{}", self.resource_tf, naming::terraform_name(&label, "value")), self.variables, "_");
        self.inputs.push(Input { resource: self.current, reason, parameter, variable, sensitive });
        Val::Input(self.inputs.len() - 1)
    }

    /// A required property AWS never returns: inputs where the person says what they deploy.
    fn not_returned(&mut self, property: &str, props: &Value) -> Val {
        if self.type_name == "AWS::Lambda::Function" && property == "Code" {
            let image = props.get("PackageType").and_then(Value::as_str) == Some("Image");
            let fields: &[&str] = if image { &["ImageUri"] } else { &["S3Bucket", "S3Key"] };
            let code = fields.iter().map(|f| (f.to_string(), self.input(InputReason::NotReturned(format!("Code{f}")), false))).collect();
            return Val::Obj(Shape::Attrs, code);
        }
        self.input(InputReason::NotReturned(property.to_string()), named_like_secret(property))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::model::{NotRead, ResourceConfig, Wanted};
    use serde_json::json;
    use sprawlr_domain::{InferenceMethod, Inferred, NodeKind};

    pub(crate) fn node(arn: &str, kind: NodeKind, name: &str, app: &str) -> Node {
        let mut n = Node::new(arn, kind, name);
        n.account = Some("111122223333".into());
        n.region = Some("us-east-1".into());
        n.app = Some(Inferred { value: app.into(), method: InferenceMethod::Tag, confidence: 1.0 });
        n
    }

    pub(crate) fn schemas() -> BTreeMap<String, Value> {
        let tag = json!({ "type": "object", "properties": { "Key": {"type": "string"}, "Value": {"type": "string"} } });
        BTreeMap::from([
            (
                "AWS::SQS::Queue".to_string(),
                json!({
                    "typeName": "AWS::SQS::Queue",
                    "definitions": { "Tag": tag },
                    "properties": { "Arn": {"type": "string"}, "QueueUrl": {"type": "string"}, "QueueName": {"type": "string"},
                        "VisibilityTimeout": {"type": "integer"}, "RedrivePolicy": {"type": ["object", "string"]},
                        "Tags": {"type": "array", "items": {"$ref": "#/definitions/Tag"}} },
                    "readOnlyProperties": ["/properties/Arn", "/properties/QueueUrl"],
                    "primaryIdentifier": ["/properties/QueueUrl"]
                }),
            ),
            (
                "AWS::Lambda::Function".to_string(),
                json!({
                    "typeName": "AWS::Lambda::Function",
                    "definitions": {
                        "Environment": { "type": "object", "properties": { "Variables": { "type": "object", "patternProperties": { ".+": {"type": "string"} } } } },
                        "Code": { "type": "object", "properties": { "S3Bucket": {"type": "string"}, "S3Key": {"type": "string"}, "ImageUri": {"type": "string"} } }
                    },
                    "properties": { "Arn": {"type": "string"}, "FunctionName": {"type": "string"}, "Role": {"type": "string"},
                        "Environment": {"$ref": "#/definitions/Environment"}, "Code": {"$ref": "#/definitions/Code"} },
                    "readOnlyProperties": ["/properties/Arn"],
                    "writeOnlyProperties": ["/properties/Code"],
                    "required": ["Code", "Role"],
                    "primaryIdentifier": ["/properties/FunctionName"]
                }),
            ),
        ])
    }

    const QUEUE_ARN: &str = "arn:aws:sqs:us-east-1:111122223333:orders";
    const QUEUE_URL: &str = "https://sqs.us-east-1.amazonaws.com/111122223333/orders";
    const FN_ARN: &str = "arn:aws:lambda:us-east-1:111122223333:function:invoice-processor";

    fn queue(props: Value) -> ResourceConfig {
        ResourceConfig { node: QUEUE_ARN.into(), type_name: "AWS::SQS::Queue".into(), identifier: QUEUE_URL.into(), properties: props }
    }

    fn function(props: Value) -> ResourceConfig {
        ResourceConfig {
            node: FN_ARN.into(),
            type_name: "AWS::Lambda::Function".into(),
            identifier: "invoice-processor".into(),
            properties: props,
        }
    }

    fn nodes() -> Vec<Node> {
        vec![
            node(QUEUE_ARN, NodeKind::SqsQueue, "orders", "billing"),
            node(FN_ARN, NodeKind::LambdaFunction, "invoice-processor", "billing"),
        ]
    }

    fn export_of(configs: Vec<ResourceConfig>) -> Export {
        let read = ConfigRead { configs, schemas: schemas(), not_read: vec![] };
        plan(&Scope::App("billing".into()), &nodes(), Selection::default(), read, "2026-11-27")
    }

    fn queue_props() -> Value {
        json!({ "Arn": QUEUE_ARN, "QueueUrl": QUEUE_URL, "QueueName": "orders", "VisibilityTimeout": 30,
                "Tags": [{"Key": "app", "Value": "billing"}, {"Key": "aws:cloudformation:logical-id", "Value": "Q"}] })
    }

    #[test]
    fn writes_what_a_template_sets_and_leaves_out_what_aws_sets() {
        let e = export_of(vec![queue(queue_props())]);
        let q = &e.resources[0];
        assert_eq!(q.properties.keys().collect::<Vec<_>>(), ["QueueName", "Tags", "VisibilityTimeout"]);
        assert_eq!((q.logical_id.as_str(), q.tf_name.as_str(), q.tf_type.as_str()), ("Orders", "orders", "awscc_sqs_queue"));
        assert_eq!(q.identifier_parts, [("QueueUrl".to_string(), QUEUE_URL.to_string())]);
        let Val::List(tags) = &q.properties["Tags"] else { panic!() };
        assert_eq!(tags.len(), 1, "tags AWS sets are not written");
    }

    #[test]
    fn never_writes_environment_values_but_refers_to_resources_they_name() {
        let f = function(json!({ "Arn": FN_ARN, "FunctionName": "invoice-processor", "Role": "arn:aws:iam::111122223333:role/r",
            "Environment": { "Variables": { "QUEUE_URL": QUEUE_URL, "STRIPE_API_KEY": "sk_live_abc", "MODE": "fast" } } }));
        let e = export_of(vec![queue(queue_props()), f]);
        let func = e.resources.iter().find(|r| r.type_name == "AWS::Lambda::Function").unwrap();
        let Val::Obj(Shape::Attrs, env) = &func.properties["Environment"] else { panic!() };
        let Val::Obj(Shape::Map, vars) = &env["Variables"] else { panic!() };
        assert!(matches!(&vars["QUEUE_URL"], Val::Ref(Reference { attr: Attr::ReadOnly(p), .. }) if p == "QueueUrl"));
        let reasons: Vec<&InputReason> = e.inputs.iter().map(|i| &i.reason).collect();
        assert!(reasons.contains(&&InputReason::Environment("STRIPE_API_KEY".into())));
        assert!(reasons.contains(&&InputReason::Environment("MODE".into())), "every value is left out, not only those that look secret");
        let stripe = e.inputs.iter().find(|i| i.reason == InputReason::Environment("STRIPE_API_KEY".into())).unwrap();
        assert_eq!(
            (stripe.parameter.as_str(), stripe.variable.as_str(), stripe.sensitive),
            ("InvoiceProcessorStripeApiKey", "invoice_processor_stripe_api_key", true)
        );
        assert!(!format!("{:?}", e.resources).contains("sk_live_abc"));
    }

    #[test]
    fn asks_for_the_code_location_aws_never_returns_and_says_terraform_must_not_compare_it() {
        let e = export_of(vec![function(json!({ "FunctionName": "invoice-processor", "Role": "arn:aws:iam::111122223333:role/r" }))]);
        let f = &e.resources[0];
        assert!(matches!(&f.properties["Code"], Val::Obj(Shape::Attrs, c) if c.contains_key("S3Bucket") && c.contains_key("S3Key")));
        assert_eq!(f.ignore_changes, ["Code"]);
        assert!(e.inputs.iter().all(|i| !i.sensitive));
    }

    #[test]
    fn values_named_like_secrets_and_user_data_become_inputs() {
        let e = export_of(vec![queue(
            json!({ "QueueName": "orders", "AuthToken": "t0k3n", "SecretArn": "arn:aws:secretsmanager:x", "UserData": "IyEv" }),
        )]);
        let q = &e.resources[0];
        assert!(matches!(q.properties["AuthToken"], Val::Input(_)));
        assert!(matches!(q.properties["UserData"], Val::Input(_)));
        assert_eq!(q.properties["SecretArn"], Val::Str("arn:aws:secretsmanager:x".into()), "an ARN only points at a secret");
    }

    #[test]
    fn policy_conditions_are_written_as_aws_returns_them() {
        let e = export_of(vec![queue(json!({ "QueueName": "orders", "RedrivePolicy": { "Condition": { "StringLike": {
            "token.actions.githubusercontent.com:sub": "repo:acme/storefront:*" }, "Password": "hunter2" } } }))]);
        let Val::Obj(Shape::Json, doc) = &e.resources[0].properties["RedrivePolicy"] else { panic!() };
        let Val::Obj(_, cond) = &doc["Condition"] else { panic!() };
        let Val::Obj(_, like) = &cond["StringLike"] else { panic!() };
        assert_eq!(like["token.actions.githubusercontent.com:sub"], Val::Str("repo:acme/storefront:*".into()));
        assert!(matches!(cond["Password"], Val::Input(_)), "a secret is still a secret inside a document");
    }

    #[test]
    fn a_reference_that_would_close_a_cycle_stays_a_literal() {
        let q = queue(
            json!({ "Arn": QUEUE_ARN, "QueueUrl": QUEUE_URL, "QueueName": "orders", "RedrivePolicy": { "deadLetterTargetArn": FN_ARN } }),
        );
        let f = function(
            json!({ "Arn": FN_ARN, "FunctionName": "invoice-processor", "Role": "r", "DeadLetterConfig": { "TargetArn": QUEUE_ARN } }),
        );
        let e = export_of(vec![q, f]);
        let refs = |r: &Planned| format!("{:?}", r.properties).matches("Ref(").count();
        assert_eq!(e.resources.iter().map(refs).sum::<usize>(), 1);
    }

    #[test]
    fn only_values_that_can_name_one_thing_refer_to_a_resource_by_themselves() {
        assert!(looks_like_aws_id("subnet-0a1b2c3d") && looks_like_aws_id("subnet-priv-a") && looks_like_aws_id("sg-api"));
        assert!(!looks_like_aws_id("my-thing"));
        assert!(host_name("checkout-alb-1234567890.us-east-1.elb.amazonaws.com") && !unmistakable("www.example.com"));
        assert!(!unmistakable("checkout-alb") && !host_name("checkout-alb"), "a label that matches a name is not a reference");
    }

    fn with_function(queue_props: Value, function_name: &str) -> Export {
        export_of(vec![queue(queue_props), function(json!({ "Arn": FN_ARN, "FunctionName": function_name, "Role": "r" }))])
    }

    fn queue_value(e: &Export, key: &str) -> Val {
        e.resources.iter().find(|r| r.type_name == "AWS::SQS::Queue").unwrap().properties[key].clone()
    }

    #[test]
    fn every_value_under_an_environment_is_left_out_whatever_its_shape() {
        // A schema-less holder with names that look like properties, and a JSON document.
        let e = export_of(vec![queue(json!({ "QueueName": "orders",
            "Environment": { "DatabaseUrl": "postgres://admin:hunter2@db/x" },
            "EnvironmentVariables": [{ "Name": "API_URL", "Type": "PLAINTEXT", "Value": "https://api.example.com" }] }))]);
        let Val::Obj(Shape::Map, env) = queue_value(&e, "Environment") else { panic!("names of variables are data, not attributes") };
        assert!(matches!(env["DatabaseUrl"], Val::Input(_)));
        let Val::List(items) = queue_value(&e, "EnvironmentVariables") else { panic!() };
        let Val::Obj(_, item) = &items[0] else { panic!() };
        assert_eq!((item["Name"].clone(), item["Type"].clone()), (Val::Str("API_URL".into()), Val::Str("PLAINTEXT".into())));
        assert!(matches!(item["Value"], Val::Input(_)));
        assert!(!format!("{:?}", e.resources).contains("hunter2"));
        let names: Vec<&InputReason> = e.inputs.iter().map(|i| &i.reason).collect();
        assert!(
            names.contains(&&InputReason::Environment("DatabaseUrl".into()))
                && names.contains(&&InputReason::Environment("API_URL".into()))
        );
    }

    #[test]
    fn personal_data_and_secret_headers_become_inputs() {
        let e = export_of(vec![queue(json!({ "QueueName": "orders",
            "Subscription": [{ "Endpoint": "jane.doe@example.com", "Protocol": "email" }, { "Endpoint": "+15555550100", "Protocol": "sms" }],
            "OriginCustomHeaders": [{ "HeaderName": "X-Origin-Verify", "HeaderValue": "abc123" }],
            "HeaderParameters": [{ "Key": "x-api-key", "Value": "k-123" }, { "Key": "Accept", "Value": "json" }] }))]);
        let text = format!("{:?}", e.resources);
        for v in ["jane.doe@example.com", "+15555550100", "abc123", "k-123"] {
            assert!(!text.contains(v), "{v} was written");
        }
        assert!(text.contains("\"json\""), "a header that isn't secret is kept");
        assert!(e.inputs.iter().any(|i| i.reason == InputReason::Personal("Endpoint".into())));
        assert!(e.inputs.iter().any(|i| i.reason == InputReason::Secret("header X-Origin-Verify".into())));
    }

    #[test]
    fn a_resource_s_own_name_never_refers_to_another_resource() {
        let e = with_function(json!({ "QueueName": "invoice-processor" }), "invoice-processor");
        assert_eq!(queue_value(&e, "QueueName"), Val::Str("invoice-processor".into()));
    }

    #[test]
    fn a_host_name_a_person_chose_refers_to_nothing() {
        let e = with_function(json!({ "QueueName": "orders", "Aliases": ["www.example.com"] }), "www.example.com");
        assert_eq!(queue_value(&e, "Aliases"), Val::List(vec![Val::Str("www.example.com".into())]));
    }

    #[test]
    fn a_type_aws_did_not_describe_is_left_out() {
        let topic =
            ResourceConfig { node: QUEUE_ARN.into(), type_name: "AWS::SNS::Topic".into(), identifier: "t".into(), properties: json!({}) };
        let e = export_of(vec![topic]);
        assert!(e.resources.is_empty());
        assert!(e.left_out[0].reason.starts_with("AWS did not describe its type, AWS::SNS::Topic"));
    }

    #[test]
    fn a_bucket_is_placed_in_its_own_region_and_no_app_takes_the_shared_group() {
        let b = ResourceConfig {
            node: "arn:aws:s3:::logs".into(),
            type_name: "AWS::S3::Bucket".into(),
            identifier: "logs".into(),
            properties: json!({ "RegionalDomainName": "logs.s3.eu-west-1.amazonaws.com" }),
        };
        assert_eq!(region_from(&b).as_deref(), Some("eu-west-1"));
        let mut ns = nodes();
        ns[0].app = Some(Inferred { value: "Shared".into(), method: InferenceMethod::Tag, confidence: 1.0 });
        ns[1].app = None;
        assert_eq!(shared_group(&ns), "no-app");
    }

    #[test]
    fn the_same_reads_give_the_same_plan() {
        let a = format!(
            "{:?}",
            export_of(vec![queue(queue_props()), function(json!({ "FunctionName": "invoice-processor", "Role": "r" }))]).resources
        );
        let b = format!(
            "{:?}",
            export_of(vec![function(json!({ "FunctionName": "invoice-processor", "Role": "r" })), queue(queue_props())]).resources
        );
        assert_eq!(a, b);
    }

    #[test]
    fn what_could_not_be_read_is_left_out_in_aws_s_words() {
        let read = ConfigRead {
            configs: vec![],
            schemas: schemas(),
            not_read: vec![NotRead {
                node: QUEUE_ARN.into(),
                reason: "AccessDeniedException: not authorized to perform sqs:GetQueueAttributes".into(),
            }],
        };
        let sel = Selection {
            wanted: vec![Wanted {
                node: QUEUE_ARN.into(),
                type_name: "AWS::SQS::Queue".into(),
                identifier: QUEUE_URL.into(),
                region: None,
            }],
            left_out: vec![],
        };
        let e = plan(&Scope::Account, &nodes(), sel, read, "2026-11-27");
        assert_eq!(e.left_out[0].name, "orders");
        assert!(e.left_out[0].reason.ends_with("not authorized to perform sqs:GetQueueAttributes"));
    }

    #[test]
    fn the_whole_account_is_grouped_by_app_with_the_rest_shared() {
        let mut ns = nodes();
        ns[0].app = None;
        let read = ConfigRead {
            configs: vec![queue(queue_props()), function(json!({ "FunctionName": "invoice-processor", "Role": "r" }))],
            schemas: schemas(),
            not_read: vec![],
        };
        let e = plan(&Scope::Account, &ns, Selection::default(), read, "2026-11-27");
        assert_eq!(e.groups(), ["billing", "shared"]);
        assert_eq!(slug("Checkout API"), "checkout-api");
    }
}
