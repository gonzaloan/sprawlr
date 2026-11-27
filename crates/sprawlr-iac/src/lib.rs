//! Code from an account: Terraform (awscc provider) and CloudFormation written from each
//! resource's live configuration, with a README that says how to check it. Pure: the
//! configuration comes in through `sprawlr_app`'s `ConfigSource` port.
//!
//! 1. [`select`] picks the resources of a scope and how AWS identifies each, and leaves out
//!    what a stack already manages or AWS owns.
//! 2. [`plan`] names them, groups them by app, turns identifiers of other resources into
//!    references, and turns every value that could be a secret into an input (ADR 0006).
//! 3. [`render`] writes the files. The same reads give the same files.

pub mod cloudformation;
pub mod model;
pub mod naming;
pub mod plan;
pub mod readme;
pub mod schema;
pub mod select;
pub mod terraform;

pub use model::{CodeFile, ConfigRead, LeftOut, NotRead, ResourceConfig, Scope, Wanted};
pub use plan::{plan, Export};
pub use readme::InputLine;
pub use select::{select, Selection};

/// Every file of an export except the documentation, which the caller adds under `docs/`.
pub fn render(e: &Export) -> Vec<CodeFile> {
    let mut files = vec![readme::file(e)];
    files.extend(terraform::files(e));
    files.extend(cloudformation::files(e));
    files
}

/// What the person sees once an export is written.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Summary {
    /// "checkout", or "the whole account".
    pub title: String,
    pub resources: usize,
    /// Of them, those written as Terraform too (the awscc provider lacks a few types).
    pub terraform: usize,
    pub groups: Vec<String>,
    pub inputs: Vec<InputLine>,
    pub left_out: Vec<LeftOut>,
    /// What to do before CloudFormation takes a template (it is too big).
    pub notes: Vec<String>,
}

pub fn summary(e: &Export) -> Summary {
    Summary {
        title: e.title(),
        resources: e.resources.len(),
        terraform: e.in_terraform().len(),
        groups: e.groups(),
        inputs: readme::input_lines(e),
        left_out: e.left_out.clone(),
        notes: e.notes(),
    }
}
