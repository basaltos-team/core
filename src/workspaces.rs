use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::types::{WorkspaceConfig, WorkspacesConfig};

const WORKSPACE_STATE_FILE: &str = "workspace-state.json";
const WORKSPACE_STATE_SCHEMA_VERSION: &str = "basalt-workspace-state-v0";
const WORKSPACE_ARTIFACT_HASH_ALGORITHM: &str = "fnv1a64";
const WORKSPACE_BACKEND_DEVENV: &str = "devenv";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceArtifact {
    pub name: String,
    pub workspace_path: String,
    pub backend: String,
    pub languages: BTreeMap<String, bool>,
    pub packages: Vec<String>,
    pub services: BTreeMap<String, bool>,
    pub tasks: BTreeMap<String, String>,
    pub devenv_nix: PathBuf,
    pub manifest_devenv_nix: PathBuf,
    pub config_size_bytes: usize,
    pub config_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceGenerationSummary {
    pub artifacts: Vec<WorkspaceArtifact>,
    pub state_manifest: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceManifestCheck {
    pub checked_artifacts: usize,
    pub checked_workspaces: Vec<String>,
    pub failures: Vec<String>,
}

impl WorkspaceManifestCheck {
    pub fn is_ok(&self) -> bool {
        self.failures.is_empty()
    }
}

pub fn generate_workspace_artifacts(
    workspaces: Option<&WorkspacesConfig>,
    output_dir: &Path,
) -> Result<WorkspaceGenerationSummary, String> {
    let Some(workspaces) = workspaces else {
        fs::create_dir_all(output_dir).map_err(|err| format!("{}: {err}", output_dir.display()))?;
        let state_manifest = output_dir.join(WORKSPACE_STATE_FILE);
        fs::write(&state_manifest, render_workspace_state_manifest(&[]))
            .map_err(|err| format!("{}: {err}", state_manifest.display()))?;
        return Ok(WorkspaceGenerationSummary {
            artifacts: Vec::new(),
            state_manifest,
        });
    };

    let mut artifacts = Vec::new();
    for (name, workspace) in &workspaces.entries {
        let workspace_dir = output_dir.join(name);
        fs::create_dir_all(&workspace_dir)
            .map_err(|err| format!("{}: {err}", workspace_dir.display()))?;
        let devenv_nix = workspace_dir.join("devenv.nix");
        let rendered = render_devenv_nix(workspace);
        fs::write(&devenv_nix, &rendered)
            .map_err(|err| format!("{}: {err}", devenv_nix.display()))?;
        artifacts.push(WorkspaceArtifact {
            name: name.clone(),
            workspace_path: workspace.path.clone(),
            backend: workspace.backend.clone(),
            languages: workspace.languages.clone(),
            packages: workspace.packages.clone(),
            services: workspace.services.clone(),
            tasks: workspace.tasks.clone(),
            manifest_devenv_nix: PathBuf::from(format!("./{name}/devenv.nix")),
            devenv_nix,
            config_size_bytes: rendered.len(),
            config_hash: stable_hash_hex(&rendered),
        });
    }

    let state_manifest = output_dir.join(WORKSPACE_STATE_FILE);
    fs::write(&state_manifest, render_workspace_state_manifest(&artifacts))
        .map_err(|err| format!("{}: {err}", state_manifest.display()))?;

    Ok(WorkspaceGenerationSummary {
        artifacts,
        state_manifest,
    })
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

pub fn render_workspace_state_manifest(artifacts: &[WorkspaceArtifact]) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&format!(
        "  \"schema_version\": {},\n",
        json_string(WORKSPACE_STATE_SCHEMA_VERSION)
    ));
    out.push_str(&format!("  \"workspace_count\": {},\n", artifacts.len()));
    out.push_str("  \"workspaces\": [\n");
    for (index, artifact) in artifacts.iter().enumerate() {
        out.push_str("    {\n");
        out.push_str(&format!(
            "      \"name\": {},\n",
            json_string(&artifact.name)
        ));
        out.push_str(&format!(
            "      \"backend\": {},\n",
            json_string(&artifact.backend)
        ));
        out.push_str(&format!(
            "      \"workspace_path\": {},\n",
            json_string(&artifact.workspace_path)
        ));
        out.push_str(&format!(
            "      \"devenv_nix\": {},\n",
            json_string(&artifact.manifest_devenv_nix.display().to_string())
        ));
        out.push_str("      \"inputs\": {\n");
        out.push_str("        \"languages\": ");
        push_json_bool_map(&mut out, &artifact.languages);
        out.push_str(",\n");
        out.push_str("        \"packages\": ");
        push_json_string_list(&mut out, &artifact.packages);
        out.push_str(",\n");
        out.push_str("        \"services\": ");
        push_json_bool_map(&mut out, &artifact.services);
        out.push_str(",\n");
        out.push_str("        \"tasks\": ");
        push_json_string_map(&mut out, &artifact.tasks);
        out.push_str("\n");
        out.push_str("      },\n");
        out.push_str(&format!(
            "      \"config_hash_algorithm\": {},\n",
            json_string(WORKSPACE_ARTIFACT_HASH_ALGORITHM)
        ));
        out.push_str(&format!(
            "      \"config_size_bytes\": {},\n",
            artifact.config_size_bytes
        ));
        out.push_str(&format!(
            "      \"config_hash\": {}\n",
            json_string(&artifact.config_hash)
        ));
        out.push_str("    }");
        if index + 1 != artifacts.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("  ]\n");
    out.push_str("}\n");
    out
}

pub fn check_workspace_state_manifest(manifest: &Path) -> Result<WorkspaceManifestCheck, String> {
    let contents =
        fs::read_to_string(manifest).map_err(|err| format!("{}: {err}", manifest.display()))?;
    validate_manifest_schema_version(&contents)?;
    let manifest_dir = manifest.parent().unwrap_or_else(|| Path::new("."));
    let entries = parse_manifest_artifact_entries(&contents)?;
    validate_manifest_workspace_count(&contents, entries.len())?;
    validate_workspace_names(&entries)?;
    validate_unique_workspace_names(&entries)?;
    validate_unique_artifact_paths(&entries)?;
    validate_devenv_artifact_paths(&entries)?;
    validate_supported_workspace_backends(&entries)?;
    validate_workspace_paths(&entries)?;
    validate_config_hash_algorithms(&entries)?;
    validate_config_hash_formats(&entries)?;
    let mut failures = Vec::new();

    for entry in &entries {
        let artifact_path = match resolve_manifest_path(manifest_dir, &entry.devenv_nix) {
            Ok(path) => path,
            Err(err) => {
                failures.push(err);
                continue;
            }
        };
        match fs::read_to_string(&artifact_path) {
            Ok(contents) => {
                let observed_size = contents.len();
                if observed_size != entry.config_size_bytes {
                    failures.push(format!(
                        "{}: config size mismatch, expected {}, observed {}",
                        artifact_path.display(),
                        entry.config_size_bytes,
                        observed_size
                    ));
                    continue;
                }
                let observed_hash = stable_hash_hex(&contents);
                if observed_hash != entry.config_hash {
                    failures.push(format!(
                        "{}: config hash mismatch, expected {}, observed {}",
                        artifact_path.display(),
                        entry.config_hash,
                        observed_hash
                    ));
                }
            }
            Err(err) => failures.push(format!("{}: {err}", artifact_path.display())),
        }
    }

    Ok(WorkspaceManifestCheck {
        checked_artifacts: entries.len(),
        checked_workspaces: entries.iter().map(|entry| entry.name.clone()).collect(),
        failures,
    })
}

pub fn render_workspace_manifest_check(check: &WorkspaceManifestCheck) -> String {
    let mut out = String::from("Basalt workspace manifest check\n\n");
    out.push_str(&format!("Checked artifacts: {}\n", check.checked_artifacts));
    if check.checked_workspaces.is_empty() {
        out.push_str("Checked workspaces: none\n");
    } else {
        out.push_str("Checked workspaces:\n");
        for workspace in &check.checked_workspaces {
            out.push_str("- ");
            out.push_str(workspace);
            out.push('\n');
        }
    }
    if check.failures.is_empty() {
        out.push_str("Status: ok\n");
    } else {
        out.push_str("Status: failed\n");
        for failure in &check.failures {
            out.push_str("- ");
            out.push_str(failure);
            out.push('\n');
        }
    }
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

fn stable_hash_hex(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for character in value.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if character.is_control() => {
                out.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

fn push_json_bool_map(out: &mut String, values: &BTreeMap<String, bool>) {
    out.push('{');
    for (index, (key, value)) in values.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(&json_string(key));
        out.push_str(": ");
        out.push_str(if *value { "true" } else { "false" });
    }
    out.push('}');
}

fn push_json_string_map(out: &mut String, values: &BTreeMap<String, String>) {
    out.push('{');
    for (index, (key, value)) in values.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(&json_string(key));
        out.push_str(": ");
        out.push_str(&json_string(value));
    }
    out.push('}');
}

fn push_json_string_list(out: &mut String, values: &[String]) {
    out.push('[');
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(&json_string(value));
    }
    out.push(']');
}

fn validate_manifest_schema_version(contents: &str) -> Result<(), String> {
    let Some(line) = contents
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("\"schema_version\":"))
    else {
        return Err("workspace manifest missing schema_version".to_string());
    };
    let observed = extract_json_string_field(line, "schema_version")?;
    if observed != WORKSPACE_STATE_SCHEMA_VERSION {
        return Err(format!(
            "workspace manifest schema_version `{observed}` is unsupported; expected `{WORKSPACE_STATE_SCHEMA_VERSION}`"
        ));
    }
    Ok(())
}

fn validate_manifest_workspace_count(contents: &str, observed_count: usize) -> Result<(), String> {
    let Some(line) = contents
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("\"workspace_count\":"))
    else {
        return Err("workspace manifest missing workspace_count".to_string());
    };
    let expected_count = extract_json_usize_field(line, "workspace_count")?;
    if expected_count != observed_count {
        return Err(format!(
            "workspace manifest workspace_count `{expected_count}` does not match parsed workspace entries `{observed_count}`"
        ));
    }
    Ok(())
}

fn validate_workspace_names(entries: &[ManifestArtifactEntry]) -> Result<(), String> {
    for entry in entries {
        if entry.name.trim().is_empty() {
            return Err("workspace manifest contains empty workspace name".to_string());
        }
    }
    Ok(())
}

fn validate_unique_workspace_names(entries: &[ManifestArtifactEntry]) -> Result<(), String> {
    let mut names = BTreeSet::new();
    for entry in entries {
        if !names.insert(entry.name.as_str()) {
            return Err(format!(
                "workspace manifest contains duplicate workspace name `{}`",
                entry.name
            ));
        }
    }
    Ok(())
}

fn validate_unique_artifact_paths(entries: &[ManifestArtifactEntry]) -> Result<(), String> {
    let mut paths = BTreeSet::new();
    for entry in entries {
        if !paths.insert(entry.devenv_nix.as_str()) {
            return Err(format!(
                "workspace manifest contains duplicate artifact path `{}`",
                entry.devenv_nix
            ));
        }
    }
    Ok(())
}

fn validate_devenv_artifact_paths(entries: &[ManifestArtifactEntry]) -> Result<(), String> {
    for entry in entries {
        let relative = entry
            .devenv_nix
            .strip_prefix("./")
            .unwrap_or(&entry.devenv_nix);
        let file_name = Path::new(relative)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if file_name != "devenv.nix" {
            return Err(format!(
                "workspace manifest entry `{}` has invalid devenv_nix `{}`; expected a devenv.nix artifact",
                entry.name, entry.devenv_nix
            ));
        }
    }
    Ok(())
}

fn validate_supported_workspace_backends(entries: &[ManifestArtifactEntry]) -> Result<(), String> {
    for entry in entries {
        if entry.backend != WORKSPACE_BACKEND_DEVENV {
            return Err(format!(
                "workspace manifest entry `{}` has unsupported backend `{}`; expected `{}`",
                entry.name, entry.backend, WORKSPACE_BACKEND_DEVENV
            ));
        }
    }
    Ok(())
}

fn validate_workspace_paths(entries: &[ManifestArtifactEntry]) -> Result<(), String> {
    for entry in entries {
        if entry.workspace_path.trim().is_empty() {
            return Err(format!(
                "workspace manifest entry `{}` has empty workspace_path",
                entry.name
            ));
        }
    }
    Ok(())
}

fn validate_config_hash_algorithms(entries: &[ManifestArtifactEntry]) -> Result<(), String> {
    for entry in entries {
        if entry.config_hash_algorithm != WORKSPACE_ARTIFACT_HASH_ALGORITHM {
            return Err(format!(
                "workspace manifest entry `{}` has unsupported config hash algorithm `{}`; expected `{}`",
                entry.name, entry.config_hash_algorithm, WORKSPACE_ARTIFACT_HASH_ALGORITHM
            ));
        }
    }
    Ok(())
}

fn validate_config_hash_formats(entries: &[ManifestArtifactEntry]) -> Result<(), String> {
    for entry in entries {
        if entry.config_hash.len() != 16
            || !entry
                .config_hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(format!(
                "workspace manifest entry `{}` has invalid config_hash `{}`; expected 16 lowercase hex characters",
                entry.name, entry.config_hash
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ManifestArtifactEntry {
    name: String,
    backend: String,
    workspace_path: String,
    devenv_nix: String,
    config_hash_algorithm: String,
    config_size_bytes: usize,
    config_hash: String,
}

fn parse_manifest_artifact_entries(contents: &str) -> Result<Vec<ManifestArtifactEntry>, String> {
    let mut entries = Vec::new();
    let mut current_name = None;
    let mut current_backend = None;
    let mut current_workspace_path = None;
    let mut current_devenv_nix = None;
    let mut current_config_hash_algorithm = None;
    let mut current_config_size_bytes = None;

    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with("\"name\":") {
            current_name = Some(extract_json_string_field(line, "name")?);
        } else if line.starts_with("\"backend\":") {
            current_backend = Some(extract_json_string_field(line, "backend")?);
        } else if line.starts_with("\"workspace_path\":") {
            current_workspace_path = Some(extract_json_string_field(line, "workspace_path")?);
        } else if line.starts_with("\"devenv_nix\":") {
            current_devenv_nix = Some(extract_json_string_field(line, "devenv_nix")?);
        } else if line.starts_with("\"config_hash_algorithm\":") {
            current_config_hash_algorithm =
                Some(extract_json_string_field(line, "config_hash_algorithm")?);
        } else if line.starts_with("\"config_size_bytes\":") {
            current_config_size_bytes = Some(extract_json_usize_field(line, "config_size_bytes")?);
        } else if line.starts_with("\"config_hash\":") {
            let config_hash = extract_json_string_field(line, "config_hash")?;
            let name = current_name.take().ok_or_else(|| {
                "workspace manifest has config_hash before workspace name".to_string()
            })?;
            let backend = current_backend.take().ok_or_else(|| {
                "workspace manifest has config_hash before workspace backend".to_string()
            })?;
            let workspace_path = current_workspace_path.take().ok_or_else(|| {
                "workspace manifest has config_hash before workspace_path".to_string()
            })?;
            let devenv_nix = current_devenv_nix.take().ok_or_else(|| {
                "workspace manifest has config_hash before devenv_nix".to_string()
            })?;
            let config_hash_algorithm = current_config_hash_algorithm.take().ok_or_else(|| {
                "workspace manifest has config_hash before config_hash_algorithm".to_string()
            })?;
            let config_size_bytes = current_config_size_bytes.take().ok_or_else(|| {
                "workspace manifest has config_hash before config_size_bytes".to_string()
            })?;
            entries.push(ManifestArtifactEntry {
                name,
                backend,
                workspace_path,
                devenv_nix,
                config_hash_algorithm,
                config_size_bytes,
                config_hash,
            });
        }
    }

    if current_name.is_some() {
        return Err("workspace manifest has workspace name without config_hash".to_string());
    }
    if current_backend.is_some() {
        return Err("workspace manifest has workspace backend without config_hash".to_string());
    }
    if current_workspace_path.is_some() {
        return Err("workspace manifest has workspace_path without config_hash".to_string());
    }
    if current_devenv_nix.is_some() {
        return Err("workspace manifest has devenv_nix without config_hash".to_string());
    }
    if current_config_hash_algorithm.is_some() {
        return Err("workspace manifest has config_hash_algorithm without config_hash".to_string());
    }
    if current_config_size_bytes.is_some() {
        return Err("workspace manifest has config_size_bytes without config_hash".to_string());
    }

    Ok(entries)
}

fn extract_json_string_field(line: &str, field: &str) -> Result<String, String> {
    let prefix = format!("\"{field}\":");
    let value = line
        .strip_prefix(&prefix)
        .ok_or_else(|| format!("workspace manifest line is not `{field}`"))?
        .trim()
        .trim_end_matches(',')
        .trim();
    parse_json_string(value).ok_or_else(|| format!("workspace manifest `{field}` must be a string"))
}

fn extract_json_usize_field(line: &str, field: &str) -> Result<usize, String> {
    let prefix = format!("\"{field}\":");
    let value = line
        .strip_prefix(&prefix)
        .ok_or_else(|| format!("workspace manifest line is not `{field}`"))?
        .trim()
        .trim_end_matches(',')
        .trim();
    value.parse::<usize>().map_err(|err| {
        format!("workspace manifest `{field}` must be a non-negative integer: {err}")
    })
}

fn parse_json_string(value: &str) -> Option<String> {
    let value = value.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        match chars.next()? {
            '\\' => out.push('\\'),
            '"' => out.push('"'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            _ => return None,
        }
    }
    Some(out)
}

fn resolve_manifest_path(manifest_dir: &Path, value: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        return Err(format!(
            "workspace manifest artifact path `{value}` must be relative"
        ));
    }

    let relative = value.strip_prefix("./").unwrap_or(value);
    let relative_path = Path::new(relative);
    if relative_path
        .components()
        .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(format!(
            "workspace manifest artifact path `{value}` must stay inside the manifest directory"
        ));
    }

    Ok(manifest_dir.join(relative_path))
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

    #[test]
    fn renders_workspace_state_manifest() {
        let artifacts = vec![WorkspaceArtifact {
            name: "basalt_core".to_string(),
            workspace_path: "~/Projects/basaltos/core".to_string(),
            backend: "devenv".to_string(),
            languages: BTreeMap::from([("rust".to_string(), true)]),
            packages: vec!["pkg-config".to_string(), "openssl".to_string()],
            services: BTreeMap::from([("postgres".to_string(), true)]),
            tasks: BTreeMap::from([("test".to_string(), "cargo test".to_string())]),
            devenv_nix: PathBuf::from("/tmp/out/basalt_core/devenv.nix"),
            manifest_devenv_nix: PathBuf::from("./basalt_core/devenv.nix"),
            config_size_bytes: 123,
            config_hash: "0123456789abcdef".to_string(),
        }];

        let rendered = render_workspace_state_manifest(&artifacts);

        assert!(rendered.contains("\"schema_version\": \"basalt-workspace-state-v0\""));
        assert!(rendered.contains("\"workspace_count\": 1"));
        assert!(rendered.contains("\"name\": \"basalt_core\""));
        assert!(rendered.contains("\"backend\": \"devenv\""));
        assert!(rendered.contains("\"languages\": {\"rust\": true}"));
        assert!(rendered.contains("\"packages\": [\"pkg-config\", \"openssl\"]"));
        assert!(rendered.contains("\"tasks\": {\"test\": \"cargo test\"}"));
        assert!(rendered.contains("\"config_hash_algorithm\": \"fnv1a64\""));
        assert!(rendered.contains("\"config_size_bytes\": 123"));
        assert!(rendered.contains("\"config_hash\": \"0123456789abcdef\""));
    }

    #[test]
    fn checks_workspace_state_manifest_hashes() {
        let base = std::env::temp_dir().join(format!(
            "basalt-workspace-check-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("basalt_core")).unwrap();
        let artifact = base.join("basalt_core/devenv.nix");
        let contents = "{ pkgs, ... }:\n{}\n";
        fs::write(&artifact, contents).unwrap();
        let manifest = base.join(WORKSPACE_STATE_FILE);
        fs::write(
            &manifest,
            render_workspace_state_manifest(&[WorkspaceArtifact {
                name: "basalt_core".to_string(),
                workspace_path: "~/Projects/basaltos/core".to_string(),
                backend: "devenv".to_string(),
                languages: BTreeMap::new(),
                packages: Vec::new(),
                services: BTreeMap::new(),
                tasks: BTreeMap::new(),
                devenv_nix: PathBuf::from("./basalt_core/devenv.nix"),
                manifest_devenv_nix: PathBuf::from("./basalt_core/devenv.nix"),
                config_size_bytes: contents.len(),
                config_hash: stable_hash_hex(contents),
            }]),
        )
        .unwrap();

        let check = check_workspace_state_manifest(&manifest).unwrap();

        assert!(check.is_ok());
        assert_eq!(check.checked_artifacts, 1);
        assert_eq!(check.checked_workspaces, vec!["basalt_core"]);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn rejects_escaping_workspace_manifest_artifact_paths() {
        let base =
            std::env::temp_dir().join(format!("basalt-workspace-path-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let manifest = base.join(WORKSPACE_STATE_FILE);
        fs::write(
            &manifest,
            render_workspace_state_manifest(&[WorkspaceArtifact {
                name: "bad".to_string(),
                workspace_path: "~/bad".to_string(),
                backend: "devenv".to_string(),
                languages: BTreeMap::new(),
                packages: Vec::new(),
                services: BTreeMap::new(),
                tasks: BTreeMap::new(),
                devenv_nix: PathBuf::from("../outside/devenv.nix"),
                manifest_devenv_nix: PathBuf::from("../outside/devenv.nix"),
                config_size_bytes: 0,
                config_hash: "0123456789abcdef".to_string(),
            }]),
        )
        .unwrap();

        let check = check_workspace_state_manifest(&manifest).unwrap();

        assert!(!check.is_ok());
        assert!(check.failures[0].contains("must stay inside the manifest directory"));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn rejects_wrong_workspace_manifest_schema_version() {
        let manifest = std::env::temp_dir().join(format!(
            "basalt-workspace-schema-test-{}.json",
            std::process::id()
        ));
        fs::write(
            &manifest,
            "{\n  \"schema_version\": \"wrong\",\n  \"workspace_count\": 0,\n  \"workspaces\": []\n}\n",
        )
        .unwrap();

        let err = check_workspace_state_manifest(&manifest).unwrap_err();

        assert!(err.contains("schema_version `wrong` is unsupported"));
        let _ = fs::remove_file(&manifest);
    }

    #[test]
    fn rejects_wrong_workspace_manifest_hash_algorithm() {
        let manifest = std::env::temp_dir().join(format!(
            "basalt-workspace-hash-algorithm-test-{}.json",
            std::process::id()
        ));
        fs::write(
            &manifest,
            "{\n  \"schema_version\": \"basalt-workspace-state-v0\",\n  \"workspace_count\": 1,\n  \"workspaces\": [\n    {\n      \"name\": \"basalt_core\",\n      \"backend\": \"devenv\",\n      \"workspace_path\": \"~/Projects/basaltos/core\",\n      \"devenv_nix\": \"./basalt_core/devenv.nix\",\n      \"config_hash_algorithm\": \"sha256\",\n      \"config_size_bytes\": 18,\n      \"config_hash\": \"0123456789abcdef\"\n    }\n  ]\n}\n",
        )
        .unwrap();

        let err = check_workspace_state_manifest(&manifest).unwrap_err();

        assert!(err.contains("unsupported config hash algorithm `sha256`"));
        let _ = fs::remove_file(&manifest);
    }

    #[test]
    fn rejects_unsupported_workspace_manifest_backend() {
        let manifest = std::env::temp_dir().join(format!(
            "basalt-workspace-backend-test-{}.json",
            std::process::id()
        ));
        fs::write(
            &manifest,
            "{\n  \"schema_version\": \"basalt-workspace-state-v0\",\n  \"workspace_count\": 1,\n  \"workspaces\": [\n    {\n      \"name\": \"basalt_core\",\n      \"backend\": \"profile\",\n      \"workspace_path\": \"~/Projects/basaltos/core\",\n      \"devenv_nix\": \"./basalt_core/devenv.nix\",\n      \"config_hash_algorithm\": \"fnv1a64\",\n      \"config_size_bytes\": 1,\n      \"config_hash\": \"0123456789abcdef\"\n    }\n  ]\n}\n",
        )
        .unwrap();

        let err = check_workspace_state_manifest(&manifest).unwrap_err();

        assert!(err.contains("unsupported backend `profile`"));
        let _ = fs::remove_file(&manifest);
    }

    #[test]
    fn rejects_empty_workspace_manifest_workspace_path() {
        let manifest = std::env::temp_dir().join(format!(
            "basalt-workspace-empty-path-test-{}.json",
            std::process::id()
        ));
        fs::write(
            &manifest,
            "{\n  \"schema_version\": \"basalt-workspace-state-v0\",\n  \"workspace_count\": 1,\n  \"workspaces\": [\n    {\n      \"name\": \"basalt_core\",\n      \"backend\": \"devenv\",\n      \"workspace_path\": \"  \",\n      \"devenv_nix\": \"./basalt_core/devenv.nix\",\n      \"config_hash_algorithm\": \"fnv1a64\",\n      \"config_size_bytes\": 1,\n      \"config_hash\": \"0123456789abcdef\"\n    }\n  ]\n}\n",
        )
        .unwrap();

        let err = check_workspace_state_manifest(&manifest).unwrap_err();

        assert!(err.contains("empty workspace_path"));
        let _ = fs::remove_file(&manifest);
    }

    #[test]
    fn rejects_malformed_workspace_manifest_config_hash() {
        let manifest = std::env::temp_dir().join(format!(
            "basalt-workspace-hash-format-test-{}.json",
            std::process::id()
        ));
        fs::write(
            &manifest,
            "{\n  \"schema_version\": \"basalt-workspace-state-v0\",\n  \"workspace_count\": 1,\n  \"workspaces\": [\n    {\n      \"name\": \"basalt_core\",\n      \"backend\": \"devenv\",\n      \"workspace_path\": \"~/Projects/basaltos/core\",\n      \"devenv_nix\": \"./basalt_core/devenv.nix\",\n      \"config_hash_algorithm\": \"fnv1a64\",\n      \"config_size_bytes\": 1,\n      \"config_hash\": \"not-a-hash\"\n    }\n  ]\n}\n",
        )
        .unwrap();

        let err = check_workspace_state_manifest(&manifest).unwrap_err();

        assert!(err.contains("invalid config_hash `not-a-hash`"));
        let _ = fs::remove_file(&manifest);
    }

    #[test]
    fn rejects_wrong_workspace_manifest_artifact_name() {
        let manifest = std::env::temp_dir().join(format!(
            "basalt-workspace-artifact-name-test-{}.json",
            std::process::id()
        ));
        fs::write(
            &manifest,
            "{\n  \"schema_version\": \"basalt-workspace-state-v0\",\n  \"workspace_count\": 1,\n  \"workspaces\": [\n    {\n      \"name\": \"basalt_core\",\n      \"backend\": \"devenv\",\n      \"workspace_path\": \"~/Projects/basaltos/core\",\n      \"devenv_nix\": \"./basalt_core/profile.nix\",\n      \"config_hash_algorithm\": \"fnv1a64\",\n      \"config_size_bytes\": 1,\n      \"config_hash\": \"0123456789abcdef\"\n    }\n  ]\n}\n",
        )
        .unwrap();

        let err = check_workspace_state_manifest(&manifest).unwrap_err();

        assert!(err.contains("invalid devenv_nix `./basalt_core/profile.nix`"));
        let _ = fs::remove_file(&manifest);
    }

    #[test]
    fn rejects_empty_workspace_manifest_name() {
        let manifest = std::env::temp_dir().join(format!(
            "basalt-workspace-empty-name-test-{}.json",
            std::process::id()
        ));
        fs::write(
            &manifest,
            "{\n  \"schema_version\": \"basalt-workspace-state-v0\",\n  \"workspace_count\": 1,\n  \"workspaces\": [\n    {\n      \"name\": \"  \",\n      \"backend\": \"devenv\",\n      \"workspace_path\": \"~/Projects/basaltos/core\",\n      \"devenv_nix\": \"./basalt_core/devenv.nix\",\n      \"config_hash_algorithm\": \"fnv1a64\",\n      \"config_size_bytes\": 1,\n      \"config_hash\": \"0123456789abcdef\"\n    }\n  ]\n}\n",
        )
        .unwrap();

        let err = check_workspace_state_manifest(&manifest).unwrap_err();

        assert!(err.contains("empty workspace name"));
        let _ = fs::remove_file(&manifest);
    }

    #[test]
    fn rejects_wrong_workspace_manifest_count() {
        let manifest = std::env::temp_dir().join(format!(
            "basalt-workspace-count-test-{}.json",
            std::process::id()
        ));
        fs::write(
            &manifest,
            "{\n  \"schema_version\": \"basalt-workspace-state-v0\",\n  \"workspace_count\": 2,\n  \"workspaces\": []\n}\n",
        )
        .unwrap();

        let err = check_workspace_state_manifest(&manifest).unwrap_err();

        assert!(err.contains("workspace_count `2` does not match parsed workspace entries `0`"));
        let _ = fs::remove_file(&manifest);
    }

    #[test]
    fn rejects_duplicate_workspace_manifest_names() {
        let manifest = std::env::temp_dir().join(format!(
            "basalt-workspace-duplicate-test-{}.json",
            std::process::id()
        ));
        fs::write(
            &manifest,
            "{\n  \"schema_version\": \"basalt-workspace-state-v0\",\n  \"workspace_count\": 2,\n  \"workspaces\": [\n    {\n      \"name\": \"dup\",\n      \"backend\": \"devenv\",\n      \"workspace_path\": \"~/one\",\n      \"devenv_nix\": \"./one/devenv.nix\",\n      \"config_hash_algorithm\": \"fnv1a64\",\n      \"config_size_bytes\": 1,\n      \"config_hash\": \"1111111111111111\"\n    },\n    {\n      \"name\": \"dup\",\n      \"backend\": \"devenv\",\n      \"workspace_path\": \"~/two\",\n      \"devenv_nix\": \"./two/devenv.nix\",\n      \"config_hash_algorithm\": \"fnv1a64\",\n      \"config_size_bytes\": 1,\n      \"config_hash\": \"2222222222222222\"\n    }\n  ]\n}\n",
        )
        .unwrap();

        let err = check_workspace_state_manifest(&manifest).unwrap_err();

        assert!(err.contains("duplicate workspace name `dup`"));
        let _ = fs::remove_file(&manifest);
    }

    #[test]
    fn rejects_duplicate_workspace_manifest_artifact_paths() {
        let manifest = std::env::temp_dir().join(format!(
            "basalt-workspace-duplicate-path-test-{}.json",
            std::process::id()
        ));
        fs::write(
            &manifest,
            "{\n  \"schema_version\": \"basalt-workspace-state-v0\",\n  \"workspace_count\": 2,\n  \"workspaces\": [\n    {\n      \"name\": \"one\",\n      \"backend\": \"devenv\",\n      \"workspace_path\": \"~/one\",\n      \"devenv_nix\": \"./shared/devenv.nix\",\n      \"config_hash_algorithm\": \"fnv1a64\",\n      \"config_size_bytes\": 1,\n      \"config_hash\": \"1111111111111111\"\n    },\n    {\n      \"name\": \"two\",\n      \"backend\": \"devenv\",\n      \"workspace_path\": \"~/two\",\n      \"devenv_nix\": \"./shared/devenv.nix\",\n      \"config_hash_algorithm\": \"fnv1a64\",\n      \"config_size_bytes\": 1,\n      \"config_hash\": \"2222222222222222\"\n    }\n  ]\n}\n",
        )
        .unwrap();

        let err = check_workspace_state_manifest(&manifest).unwrap_err();

        assert!(err.contains("duplicate artifact path `./shared/devenv.nix`"));
        let _ = fs::remove_file(&manifest);
    }

    #[test]
    fn reports_workspace_manifest_size_mismatch() {
        let base =
            std::env::temp_dir().join(format!("basalt-workspace-size-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("basalt_core")).unwrap();
        let artifact = base.join("basalt_core/devenv.nix");
        let contents = "{ pkgs, ... }:\n{}\n";
        fs::write(&artifact, contents).unwrap();
        let manifest = base.join(WORKSPACE_STATE_FILE);
        fs::write(
            &manifest,
            "{\n  \"schema_version\": \"basalt-workspace-state-v0\",\n  \"workspace_count\": 1,\n  \"workspaces\": [\n    {\n      \"name\": \"basalt_core\",\n      \"backend\": \"devenv\",\n      \"workspace_path\": \"~/Projects/basaltos/core\",\n      \"devenv_nix\": \"./basalt_core/devenv.nix\",\n      \"config_hash_algorithm\": \"fnv1a64\",\n      \"config_size_bytes\": 1,\n      \"config_hash\": \"0123456789abcdef\"\n    }\n  ]\n}\n",
        )
        .unwrap();

        let check = check_workspace_state_manifest(&manifest).unwrap();

        assert!(!check.is_ok());
        assert!(check.failures[0].contains("config size mismatch"));
        let _ = fs::remove_dir_all(&base);
    }
}
