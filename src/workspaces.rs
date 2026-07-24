use std::fs;
use std::path::{Path, PathBuf};

use crate::config::types::{WorkspaceConfig, WorkspacesConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceArtifact {
    pub name: String,
    pub workspace_path: String,
    pub devenv_nix: PathBuf,
}

pub fn generate_workspace_artifacts(
    workspaces: Option<&WorkspacesConfig>,
    output_dir: &Path,
) -> Result<Vec<WorkspaceArtifact>, String> {
    let Some(workspaces) = workspaces else {
        return Ok(Vec::new());
    };

    let mut artifacts = Vec::new();
    for (name, workspace) in &workspaces.entries {
        let workspace_dir = output_dir.join(name);
        fs::create_dir_all(&workspace_dir)
            .map_err(|err| format!("{}: {err}", workspace_dir.display()))?;
        let devenv_nix = workspace_dir.join("devenv.nix");
        fs::write(&devenv_nix, render_devenv_nix(workspace))
            .map_err(|err| format!("{}: {err}", devenv_nix.display()))?;
        artifacts.push(WorkspaceArtifact {
            name: name.clone(),
            workspace_path: workspace.path.clone(),
            devenv_nix,
        });
    }

    Ok(artifacts)
}

pub fn render_devenv_nix(workspace: &WorkspaceConfig) -> String {
    let mut out = String::new();
    out.push_str("{ pkgs, ... }:\n");
    out.push_str("{\n");

    if !workspace.packages.is_empty() {
        out.push_str("  packages = [\n");
        for package in &workspace.packages {
            out.push_str("    ");
            out.push_str(&nix_attr_path("pkgs", package));
            out.push('\n');
        }
        out.push_str("  ];\n\n");
    }

    for (language, enabled) in &workspace.languages {
        out.push_str("  languages.");
        out.push_str(language);
        out.push_str(".enable = ");
        out.push_str(if *enabled { "true" } else { "false" });
        out.push_str(";\n");
    }
    if !workspace.languages.is_empty() {
        out.push('\n');
    }

    for (service, enabled) in &workspace.services {
        out.push_str("  services.");
        out.push_str(service);
        out.push_str(".enable = ");
        out.push_str(if *enabled { "true" } else { "false" });
        out.push_str(";\n");
    }
    if !workspace.services.is_empty() {
        out.push('\n');
    }

    for (task, command) in &workspace.tasks {
        out.push_str("  tasks.");
        out.push_str(&nix_quoted_attr(task));
        out.push_str(".exec = ");
        out.push_str(&nix_string(command));
        out.push_str(";\n");
    }

    out.push_str("}\n");
    out
}

fn nix_quoted_attr(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn nix_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn nix_attr_path(root: &str, value: &str) -> String {
    let mut out = String::from(root);
    for part in value.split('.') {
        out.push('.');
        out.push_str(&nix_quoted_attr(part));
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn renders_devenv_workspace_artifact() {
        let workspace = WorkspaceConfig {
            path: "~/Projects/basaltos/core".to_string(),
            backend: "devenv".to_string(),
            languages: BTreeMap::from([("rust".to_string(), true)]),
            packages: vec!["pkg-config".to_string(), "openssl".to_string()],
            services: BTreeMap::from([("postgres".to_string(), true), ("redis".to_string(), true)]),
            tasks: BTreeMap::from([("test".to_string(), "cargo test".to_string())]),
        };

        let rendered = render_devenv_nix(&workspace);

        assert!(rendered.contains("packages = ["));
        assert!(rendered.contains("pkgs.\"pkg-config\""));
        assert!(rendered.contains("languages.rust.enable = true;"));
        assert!(rendered.contains("services.postgres.enable = true;"));
        assert!(rendered.contains("tasks.\"test\".exec = \"cargo test\";"));
    }
}
