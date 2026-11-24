//! AWS profile names, for the profile picker.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Profile names from ~/.aws/config and ~/.aws/credentials (honouring AWS_CONFIG_FILE /
/// AWS_SHARED_CREDENTIALS_FILE). Only section headers are read, never keys.
pub fn list() -> Vec<String> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    let path =
        |var: &str, file: &str| std::env::var_os(var).map(PathBuf::from).or_else(|| home.as_ref().map(|h| h.join(".aws").join(file)));

    let mut out = BTreeSet::new();
    if let Some(p) = path("AWS_CONFIG_FILE", "config") {
        out.extend(sections(&p).into_iter().filter_map(|s| match s.as_str() {
            "default" => Some(s),
            _ => s.strip_prefix("profile ").map(|n| n.trim().to_string()),
        }));
    }
    if let Some(p) = path("AWS_SHARED_CREDENTIALS_FILE", "credentials") {
        out.extend(sections(&p));
    }
    out.into_iter().collect()
}

fn sections(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.trim().strip_prefix('[')?.strip_suffix(']').map(|s| s.trim().to_string()))
        .filter(|s| !s.starts_with("sso-session") && !s.starts_with("services"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_profile_names_only() {
        let dir = std::env::temp_dir().join(format!("sprawlr-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = dir.join("config");
        std::fs::write(&cfg, "[default]\nregion=us-east-1\n[profile prod-admin]\nsso_session=x\n[sso-session x]\n").unwrap();
        let names: Vec<String> = sections(&cfg)
            .into_iter()
            .filter_map(|s| if s == "default" { Some(s) } else { s.strip_prefix("profile ").map(str::to_string) })
            .collect();
        assert_eq!(names, vec!["default", "prod-admin"]);
    }
}
