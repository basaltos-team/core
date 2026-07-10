// /var/lib/basalt state persistence.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::BasaltConfig;
use crate::planning::action::Action;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CurrentState {
    pub hostname: Option<String>,
    pub timezone: Option<String>,
    pub locale: Option<String>,
    pub keymap: Option<String>,
    pub pacman_packages: BTreeSet<String>,
    pub enabled_services: BTreeSet<String>,
    pub managed_files: BTreeMap<String, String>,
}

pub trait StateReader {
    fn read_current_state(&self) -> Result<CurrentState, String>;
}

#[derive(Debug)]
pub struct StateLock {
    path: PathBuf,
}

impl StateLock {
    pub fn acquire(state_dir: &Path, mode: &str) -> Result<Self, String> {
        fs::create_dir_all(state_dir).map_err(|err| format!("{}: {err}", state_dir.display()))?;
        let path = state_dir.join("basalt.lock");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|err| {
                if err.kind() == std::io::ErrorKind::AlreadyExists {
                    format!(
                        "{}: another basalt apply run is already in progress",
                        path.display()
                    )
                } else {
                    format!("{}: {err}", path.display())
                }
            })?;

        writeln!(file, "pid={}", std::process::id())
            .map_err(|err| format!("{}: {err}", path.display()))?;
        writeln!(file, "mode={mode}").map_err(|err| format!("{}: {err}", path.display()))?;
        writeln!(file, "created_at={}", now_millis())
            .map_err(|err| format!("{}: {err}", path.display()))?;

        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for StateLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HostStateReader;

impl StateReader for HostStateReader {
    fn read_current_state(&self) -> Result<CurrentState, String> {
        Ok(CurrentState {
            hostname: crate::system::locale::read_hostname(),
            timezone: crate::system::locale::read_timezone(Path::new("/")),
            locale: crate::system::locale::read_locale(Path::new("/")),
            keymap: crate::system::locale::read_keymap(Path::new("/")),
            pacman_packages: crate::backends::pacman::read_installed_packages(),
            enabled_services: crate::system::services::read_enabled_services(),
            managed_files: BTreeMap::new(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct TargetRootStateReader<'a> {
    root_dir: &'a Path,
    config: &'a BasaltConfig,
}

impl<'a> TargetRootStateReader<'a> {
    pub fn new(root_dir: &'a Path, config: &'a BasaltConfig) -> Self {
        Self { root_dir, config }
    }
}

impl StateReader for TargetRootStateReader<'_> {
    fn read_current_state(&self) -> Result<CurrentState, String> {
        Ok(CurrentState {
            hostname: crate::system::locale::read_hostname_from_root(self.root_dir),
            timezone: crate::system::locale::read_timezone(self.root_dir),
            locale: crate::system::locale::read_locale(self.root_dir),
            keymap: crate::system::locale::read_keymap(self.root_dir),
            pacman_packages: BTreeSet::new(),
            enabled_services: BTreeSet::new(),
            managed_files: read_configured_managed_files(self.root_dir, self.config),
        })
    }
}

pub fn read_configured_managed_files(
    root_dir: &Path,
    config: &BasaltConfig,
) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    let Some(files_config) = &config.files else {
        return files;
    };

    for managed in &files_config.managed {
        let relative_path = managed.path.trim_start_matches('/');
        let path = root_dir.join(relative_path);
        if let Ok(contents) = fs::read_to_string(path) {
            files.insert(managed.path.clone(), contents);
        }
    }
    files
}

#[derive(Debug, Clone)]
pub struct RunRecord {
    pub id: String,
    pub mode: String,
    pub config_path: PathBuf,
    pub schema_version: String,
    pub action_count: usize,
    pub actions: Vec<Action>,
    pub current_hostname: Option<String>,
    pub pacman_package_count: usize,
    pub enabled_service_count: usize,
}

impl RunRecord {
    pub fn dry_run(config_path: PathBuf, actions: Vec<Action>, current: &CurrentState) -> Self {
        Self::new("dry-run", config_path, actions, current)
    }

    pub fn apply(config_path: PathBuf, actions: Vec<Action>, current: &CurrentState) -> Self {
        Self::new("apply", config_path, actions, current)
    }

    pub fn apply_failed(
        config_path: PathBuf,
        actions: Vec<Action>,
        current: &CurrentState,
    ) -> Self {
        Self::new("apply-failed", config_path, actions, current)
    }

    fn new(mode: &str, config_path: PathBuf, actions: Vec<Action>, current: &CurrentState) -> Self {
        Self {
            id: new_run_id(),
            mode: mode.to_string(),
            config_path,
            schema_version: crate::config::schema::SCHEMA_VERSION.to_string(),
            action_count: actions.len(),
            actions,
            current_hostname: current.hostname.clone(),
            pacman_package_count: current.pacman_packages.len(),
            enabled_service_count: current.enabled_services.len(),
        }
    }
}

pub fn write_run_record(
    state_dir: &Path,
    record: &RunRecord,
) -> Result<(PathBuf, PathBuf), String> {
    let run_dir = state_dir.join("runs").join(&record.id);
    fs::create_dir_all(&run_dir).map_err(|err| format!("{}: {err}", run_dir.display()))?;

    let run_path = run_dir.join("run.json");
    let latest_path = state_dir.join("latest-run.json");
    let json = render_run_record_json(record);

    fs::write(&run_path, &json).map_err(|err| format!("{}: {err}", run_path.display()))?;
    fs::write(&latest_path, json).map_err(|err| format!("{}: {err}", latest_path.display()))?;

    Ok((run_path, latest_path))
}

fn new_run_id() -> String {
    format!("run-{}", now_millis())
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn render_run_record_json(record: &RunRecord) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    push_json_field(&mut out, 1, "id", &record.id, true);
    push_json_field(&mut out, 1, "mode", &record.mode, true);
    push_json_field(
        &mut out,
        1,
        "config_path",
        &record.config_path.display().to_string(),
        true,
    );
    push_json_field(&mut out, 1, "schema_version", &record.schema_version, true);
    out.push_str(&format!("  \"action_count\": {},\n", record.action_count));
    out.push_str("  \"actions\": [\n");

    for (index, action) in record.actions.iter().enumerate() {
        out.push_str("    {\n");
        push_json_field(&mut out, 3, "id", &action.id, true);
        push_json_field(&mut out, 3, "domain", &action.domain, true);
        push_json_field(&mut out, 3, "risk", action.risk.as_str(), true);
        push_json_field(&mut out, 3, "description", &action.description, false);
        out.push_str("    }");
        if index + 1 != record.actions.len() {
            out.push(',');
        }
        out.push('\n');
    }

    out.push_str("  ],\n");
    out.push_str("  \"current_state\": {\n");
    match &record.current_hostname {
        Some(hostname) => push_json_field(&mut out, 2, "hostname", hostname, true),
        None => out.push_str("    \"hostname\": null,\n"),
    }
    out.push_str(&format!(
        "    \"pacman_package_count\": {},\n",
        record.pacman_package_count
    ));
    out.push_str(&format!(
        "    \"enabled_service_count\": {}\n",
        record.enabled_service_count
    ));
    out.push_str("  }\n");
    out.push_str("}\n");
    out
}

fn push_json_field(out: &mut String, indent: usize, key: &str, value: &str, comma: bool) {
    out.push_str(&"  ".repeat(indent));
    out.push('"');
    out.push_str(key);
    out.push_str("\": \"");
    out.push_str(&escape_json(value));
    out.push('"');
    if comma {
        out.push(',');
    }
    out.push('\n');
}

fn escape_json(value: &str) -> String {
    let mut escaped = String::new();
    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch => escaped.push(ch),
        }
    }
    escaped
}

#[derive(Debug, Clone, Default)]
#[cfg(test)]
pub struct MockStateReader {
    pub state: CurrentState,
}

#[cfg(test)]
impl StateReader for MockStateReader {
    fn read_current_state(&self) -> Result<CurrentState, String> {
        Ok(self.state.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_state_reader_returns_configured_state() {
        let reader = MockStateReader {
            state: CurrentState {
                hostname: Some("basalt-vm".to_string()),
                ..CurrentState::default()
            },
        };

        let state = reader.read_current_state().unwrap();
        assert_eq!(state.hostname.as_deref(), Some("basalt-vm"));
    }

    #[test]
    fn target_root_reader_reads_system_and_configured_files() {
        let base =
            std::env::temp_dir().join(format!("basalt-target-root-state-{}", std::process::id()));
        let root = base.join("root");
        fs::create_dir_all(root.join("etc/basalt")).unwrap();
        fs::create_dir_all(root.join("usr/share/zoneinfo")).unwrap();
        fs::write(root.join("etc/hostname"), "basalt-vm\n").unwrap();
        fs::write(root.join("etc/locale.conf"), "LANG=en_US.UTF-8\n").unwrap();
        fs::write(root.join("etc/vconsole.conf"), "KEYMAP=us\n").unwrap();
        fs::write(root.join("usr/share/zoneinfo/UTC"), "UTC\n").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/usr/share/zoneinfo/UTC", root.join("etc/localtime")).unwrap();
        fs::write(root.join("etc/basalt/motd"), "Basalt managed file\n").unwrap();

        let config = BasaltConfig {
            files: Some(crate::config::types::FilesConfig {
                managed: vec![crate::config::types::ManagedFileConfig {
                    path: "/etc/basalt/motd".to_string(),
                    content: "Basalt managed file\n".to_string(),
                    mode: None,
                }],
            }),
            ..BasaltConfig::default()
        };

        let state = TargetRootStateReader::new(&root, &config)
            .read_current_state()
            .unwrap();
        assert_eq!(state.hostname.as_deref(), Some("basalt-vm"));
        assert_eq!(state.locale.as_deref(), Some("en_US.UTF-8"));
        assert_eq!(state.keymap.as_deref(), Some("us"));
        #[cfg(unix)]
        assert_eq!(state.timezone.as_deref(), Some("UTC"));
        assert_eq!(
            state
                .managed_files
                .get("/etc/basalt/motd")
                .map(String::as_str),
            Some("Basalt managed file\n")
        );

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn renders_run_record_json() {
        let action = Action {
            id: "system.hostname".to_string(),
            domain: "system".to_string(),
            description: "set hostname to `basalt-vm`".to_string(),
            risk: crate::planning::action::Risk::Medium,
        };
        let state = CurrentState {
            hostname: Some("omega".to_string()),
            ..CurrentState::default()
        };
        let record = RunRecord::dry_run(
            PathBuf::from("../configs/examples/minimal"),
            vec![action],
            &state,
        );
        let json = render_run_record_json(&record);

        assert!(json.contains("\"mode\": \"dry-run\""));
        assert!(json.contains("\"schema_version\": \"0\""));
        assert!(json.contains("\"action_count\": 1"));
        assert!(json.contains("\"hostname\": \"omega\""));
    }

    #[test]
    fn renders_apply_run_record_json() {
        let action = Action {
            id: "system.hostname".to_string(),
            domain: "system".to_string(),
            description: "set hostname to `basalt-vm`".to_string(),
            risk: crate::planning::action::Risk::Medium,
        };
        let record = RunRecord::apply(
            PathBuf::from("../configs/fixtures/valid-system-apply"),
            vec![action],
            &CurrentState::default(),
        );
        let json = render_run_record_json(&record);

        assert!(json.contains("\"mode\": \"apply\""));
        assert!(json.contains("\"action_count\": 1"));
    }

    #[test]
    fn renders_apply_failed_run_record_json() {
        let record = RunRecord::apply_failed(
            PathBuf::from("../configs/fixtures/invalid-pacman-missing-package"),
            Vec::new(),
            &CurrentState::default(),
        );
        let json = render_run_record_json(&record);

        assert!(json.contains("\"mode\": \"apply-failed\""));
    }

    #[test]
    fn state_lock_blocks_second_acquire_and_cleans_up() {
        let base = std::env::temp_dir().join(format!("basalt-lock-test-{}", now_millis()));
        let lock = StateLock::acquire(&base, "dry-run").unwrap();
        assert!(lock.path().exists());
        let second = StateLock::acquire(&base, "dry-run").unwrap_err();
        assert!(second.contains("already in progress"));
        drop(lock);
        assert!(!base.join("basalt.lock").exists());
        let _ = fs::remove_dir_all(base);
    }
}
