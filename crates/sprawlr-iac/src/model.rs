//! What an export reads and what it leaves out.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sprawlr_domain::NodeId;
use std::collections::BTreeMap;

/// What to export: the whole account, or one app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "name", rename_all = "snake_case")]
pub enum Scope {
    Account,
    App(String),
}

/// A resource to read: its CloudFormation type and its Cloud Control identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wanted {
    pub node: NodeId,
    pub type_name: String,
    pub identifier: String,
    pub region: Option<String>,
}

/// A resource's configuration as AWS returns it: the CloudFormation resource model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceConfig {
    pub node: NodeId,
    pub type_name: String,
    pub identifier: String,
    pub properties: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotRead {
    pub node: NodeId,
    /// AWS's own words.
    pub reason: String,
}

/// What a configuration source returns for a list of [`Wanted`] resources.
#[derive(Debug, Clone, Default)]
pub struct ConfigRead {
    pub configs: Vec<ResourceConfig>,
    /// Each type's schema, as `DescribeType` returns it (parsed JSON).
    pub schemas: BTreeMap<String, Value>,
    pub not_read: Vec<NotRead>,
}

/// A resource the export does not write, and why, for the person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeftOut {
    pub node: NodeId,
    pub name: String,
    pub reason: String,
}

/// One file of the export, relative to the folder the person chose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeFile {
    pub path: String,
    pub content: String,
}
