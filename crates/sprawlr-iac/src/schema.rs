//! A CloudFormation resource type schema, read for what the code needs: which properties AWS
//! sets by itself (left out), which it never returns, which are required, how a resource is
//! identified, and the shape of each property (Terraform writes objects, maps and JSON
//! documents differently).

use serde_json::Value;

/// How a property's value is written in Terraform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// An object with known properties: awscc attribute names (`{ queue_name = ... }`).
    Attrs,
    /// An object whose keys are data (`{ "STRIPE_API_KEY" = ... }`).
    Map,
    /// A free-form JSON document: `jsonencode(...)` with the keys as AWS wrote them.
    Json,
    List,
    Scalar,
    /// No schema for it: guessed from the value.
    Unknown,
}

/// One step into a resource's properties: a key, or an item of a list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Key(String),
    Item,
}

#[derive(Debug, Clone)]
pub struct TypeSchema {
    pub type_name: String,
    read_only: Vec<Vec<String>>,
    write_only: Vec<Vec<String>>,
    create_only: Vec<Vec<String>>,
    /// Top-level properties a template must have.
    pub required: Vec<String>,
    /// Top-level properties that identify a resource, in the order of its Cloud Control identifier.
    pub primary_identifier: Vec<String>,
    root: Value,
}

/// `/properties/Endpoint/Address` → `[Endpoint, Address]`. List items (`*`) are dropped: a
/// path names the same property in every item.
fn pointer(p: &str) -> Option<Vec<String>> {
    let rest = p.strip_prefix("/properties/")?;
    Some(rest.split('/').filter(|s| *s != "*").map(String::from).collect())
}

fn pointers(raw: &Value, key: &str) -> Vec<Vec<String>> {
    raw.get(key).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).filter_map(pointer).collect()
}

fn keys_of(path: &[Step]) -> Vec<&str> {
    path.iter()
        .filter_map(|s| match s {
            Step::Key(k) => Some(k.as_str()),
            Step::Item => None,
        })
        .collect()
}

impl TypeSchema {
    /// The schema `DescribeType` returns, already parsed from its JSON string.
    pub fn parse(raw: &Value) -> Option<TypeSchema> {
        let type_name = raw.get("typeName")?.as_str()?.to_string();
        let top = |key: &str| -> Vec<String> {
            raw.get(key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(|s| s.strip_prefix("/properties/").unwrap_or(s).to_string())
                .collect()
        };
        Some(TypeSchema {
            type_name,
            read_only: pointers(raw, "readOnlyProperties"),
            write_only: pointers(raw, "writeOnlyProperties"),
            create_only: pointers(raw, "createOnlyProperties"),
            required: top("required"),
            primary_identifier: top("primaryIdentifier"),
            root: raw.clone(),
        })
    }

    /// Set by AWS, never by a template (`Arn`, `QueueUrl`, `Endpoint/Address`).
    pub fn is_read_only(&self, path: &[Step]) -> bool {
        let keys = keys_of(path);
        self.read_only.iter().any(|p| *p == keys)
    }

    /// Accepted by AWS and never returned (a function's code, a database password).
    pub fn is_write_only(&self, path: &[Step]) -> bool {
        let keys = keys_of(path);
        self.write_only.iter().any(|p| *p == keys)
    }

    /// Set when the resource is created; changing it replaces the resource.
    pub fn is_create_only(&self, path: &[Step]) -> bool {
        let keys = keys_of(path);
        self.create_only.iter().any(|p| *p == keys)
    }

    /// Top-level properties AWS never returns.
    pub fn write_only_top(&self) -> Vec<&str> {
        self.write_only.iter().filter(|p| p.len() == 1).map(|p| p[0].as_str()).collect()
    }

    pub fn top_level_read_only(&self) -> Vec<&str> {
        self.read_only.iter().filter(|p| p.len() == 1).map(|p| p[0].as_str()).collect()
    }

    fn resolve<'a>(&'a self, mut s: &'a Value) -> &'a Value {
        for _ in 0..16 {
            let Some(r) = s.get("$ref").and_then(Value::as_str) else { return s };
            let Some(name) = r.strip_prefix("#/definitions/") else { return s };
            match self.root.get("definitions").and_then(|d| d.get(name)) {
                Some(next) => s = next,
                None => return s,
            }
        }
        s
    }

    /// The schema of the value at `path`, if the schema describes it.
    fn at(&self, path: &[Step]) -> Option<&Value> {
        let mut s = &self.root;
        for step in path {
            s = self.resolve(s);
            s = match step {
                Step::Key(k) => s
                    .get("properties")
                    .and_then(|p| p.get(k))
                    .or_else(|| s.get("patternProperties").and_then(Value::as_object).and_then(|m| m.values().next()))
                    .or_else(|| s.get("additionalProperties").filter(|a| a.is_object()))?,
                Step::Item => s.get("items")?,
            };
        }
        Some(self.resolve(s))
    }

    pub fn shape(&self, path: &[Step]) -> Shape {
        let Some(s) = self.at(path) else { return Shape::Unknown };
        if s.get("properties").is_some() {
            return Shape::Attrs;
        }
        if s.get("patternProperties").is_some() || s.get("additionalProperties").is_some_and(Value::is_object) {
            return Shape::Map;
        }
        let types: Vec<&str> = match s.get("type") {
            Some(Value::String(t)) => vec![t.as_str()],
            Some(Value::Array(ts)) => ts.iter().filter_map(Value::as_str).collect(),
            _ => vec![],
        };
        if types.contains(&"object") {
            Shape::Json
        } else if types.contains(&"array") || s.get("items").is_some() {
            Shape::List
        } else if types.is_empty() {
            Shape::Unknown
        } else {
            Shape::Scalar
        }
    }
}

/// The shape of an object the schema does not describe: AWS names its own properties in
/// PascalCase, so other keys are data. Policy documents are JSON whatever their keys.
pub fn guess_shape(key: Option<&str>, value: &Value) -> Shape {
    let Some(obj) = value.as_object() else {
        return if value.is_array() { Shape::List } else { Shape::Scalar };
    };
    let document = key.is_some_and(|k| {
        let k = k.to_ascii_lowercase();
        k.ends_with("document") || k.ends_with("policy") || k == "definition" || k == "accesspolicies" || k == "policytext"
    });
    if document {
        Shape::Json
    } else if !obj.is_empty() && obj.keys().all(|k| pascal_case(k)) {
        Shape::Attrs
    } else {
        Shape::Map
    }
}

/// `VisibilityTimeout`, not `STRIPE_API_KEY` or `app`.
fn pascal_case(k: &str) -> bool {
    k.starts_with(|c: char| c.is_ascii_uppercase())
        && k.chars().all(|c| c.is_ascii_alphanumeric())
        && k.chars().any(|c| c.is_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn queue() -> TypeSchema {
        TypeSchema::parse(&json!({
            "typeName": "AWS::SQS::Queue",
            "definitions": { "Tag": { "type": "object", "properties": { "Key": {"type": "string"}, "Value": {"type": "string"} } } },
            "properties": {
                "Arn": {"type": "string"}, "QueueUrl": {"type": "string"}, "QueueName": {"type": "string"},
                "RedrivePolicy": {"type": ["object", "string"]},
                "Tags": {"type": "array", "items": {"$ref": "#/definitions/Tag"}}
            },
            "readOnlyProperties": ["/properties/Arn", "/properties/QueueUrl"],
            "primaryIdentifier": ["/properties/QueueUrl"]
        }))
        .unwrap()
    }

    fn key(k: &str) -> Step {
        Step::Key(k.into())
    }

    #[test]
    fn knows_what_aws_sets_and_how_a_resource_is_identified() {
        let s = queue();
        assert!(s.is_read_only(&[key("Arn")]));
        assert!(!s.is_read_only(&[key("QueueName")]));
        assert_eq!(s.primary_identifier, ["QueueUrl"]);
        assert_eq!(s.top_level_read_only(), ["Arn", "QueueUrl"]);
    }

    #[test]
    fn tells_objects_from_maps_and_json_documents() {
        let s = queue();
        assert_eq!(s.shape(&[key("Tags")]), Shape::List);
        assert_eq!(s.shape(&[key("Tags"), Step::Item]), Shape::Attrs);
        assert_eq!(s.shape(&[key("RedrivePolicy")]), Shape::Json);
        assert_eq!(s.shape(&[key("QueueName")]), Shape::Scalar);
        assert_eq!(s.shape(&[key("Nope")]), Shape::Unknown);

        let f = TypeSchema::parse(&json!({
            "typeName": "AWS::Lambda::Function",
            "definitions": { "Environment": { "type": "object", "properties": {
                "Variables": { "type": "object", "patternProperties": { "[a-zA-Z][a-zA-Z0-9_]+": {"type": "string"} } } } } },
            "properties": { "Environment": {"$ref": "#/definitions/Environment"}, "Code": {"type": "object", "properties": {}} },
            "writeOnlyProperties": ["/properties/Code", "/properties/Code/ZipFile"],
            "required": ["Code", "Role"]
        }))
        .unwrap();
        assert_eq!(f.shape(&[key("Environment")]), Shape::Attrs);
        assert_eq!(f.shape(&[key("Environment"), key("Variables")]), Shape::Map);
        assert_eq!(f.shape(&[key("Environment"), key("Variables"), key("STRIPE_API_KEY")]), Shape::Scalar);
        assert!(f.is_write_only(&[key("Code")]));
        assert_eq!(f.write_only_top(), ["Code"]);
        assert_eq!(f.required, ["Code", "Role"]);
    }

    #[test]
    fn paths_through_lists_name_the_property_in_every_item() {
        let s = TypeSchema::parse(&json!({
            "typeName": "AWS::X::Y",
            "properties": {},
            "readOnlyProperties": ["/properties/Rules/*/Id"]
        }))
        .unwrap();
        assert!(s.is_read_only(&[key("Rules"), Step::Item, key("Id")]));
    }

    #[test]
    fn without_a_schema_pascal_case_keys_are_properties_and_documents_stay_json() {
        assert_eq!(guess_shape(Some("Environment"), &json!({"Variables": {}})), Shape::Attrs);
        assert_eq!(guess_shape(Some("Variables"), &json!({"STRIPE_API_KEY": "x"})), Shape::Map);
        assert_eq!(guess_shape(Some("AssumeRolePolicyDocument"), &json!({"Version": "2012-10-17"})), Shape::Json);
        assert_eq!(guess_shape(Some("Tags"), &json!([])), Shape::List);
    }
}
