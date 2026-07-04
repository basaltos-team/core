// pacman query, plan, install, remove, and hold handling.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::process::command::run_capture;

pub fn read_installed_packages() -> BTreeSet<String> {
    read_installed_package_snapshot().package_names()
}

pub fn read_installed_package_snapshot() -> PackageSnapshot {
    let Ok(output) = run_capture("pacman", &["-Q"]) else {
        return PackageSnapshot::default();
    };

    if output.status_code != Some(0) {
        let _ = output.stderr;
        return PackageSnapshot::default();
    }

    let explicit = read_explicit_package_names();
    let mut packages = BTreeMap::new();
    for line in output.stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(2, ' ');
        let Some(name) = parts.next().filter(|value| !value.is_empty()) else {
            continue;
        };
        let version = parts
            .next()
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let reason = if explicit.contains(name) {
            PackageReason::Explicit
        } else {
            PackageReason::Dependency
        };
        packages.insert(
            name.to_string(),
            InstalledPackage {
                name: name.to_string(),
                version,
                reason,
            },
        );
    }

    PackageSnapshot { packages }
}

pub fn resolve_pacman_install_transaction(
    packages: &[String],
) -> Result<PackageTransaction, String> {
    let requested: BTreeSet<String> = packages
        .iter()
        .map(|package| package.trim())
        .filter(|package| !package.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    if requested.is_empty() {
        return Ok(PackageTransaction::skipped("no pacman packages requested"));
    }

    let mut args = vec!["-Sp", "--needed", "--print-format", "%n\t%v"];
    args.extend(requested.iter().map(String::as_str));

    let output = match run_capture("pacman", &args) {
        Ok(output) => output,
        Err(err) => {
            return Ok(PackageTransaction::unavailable(format!(
                "pacman transaction resolution unavailable: {err}"
            )))
        }
    };
    if output.status_code != Some(0) {
        let detail = if output.stderr.trim().is_empty() {
            output.stdout.trim()
        } else {
            output.stderr.trim()
        };
        return Ok(PackageTransaction::failed(format!(
            "pacman transaction resolution failed: {detail}"
        )));
    }

    Ok(PackageTransaction {
        status: PackageResolutionStatus::Resolved,
        message: None,
        rows: parse_pacman_print_format(&output.stdout, &requested),
    })
}

fn read_explicit_package_names() -> BTreeSet<String> {
    let Ok(output) = run_capture("pacman", &["-Qqe"]) else {
        return BTreeSet::new();
    };
    if output.status_code != Some(0) {
        return BTreeSet::new();
    }
    output
        .stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageOperation {
    pub backend: PackageBackend,
    pub action: PackageAction,
    pub package: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageSnapshot {
    pub packages: BTreeMap<String, InstalledPackage>,
}

impl PackageSnapshot {
    pub fn from_names(names: BTreeSet<String>) -> Self {
        let packages = names
            .into_iter()
            .map(|name| {
                (
                    name.clone(),
                    InstalledPackage {
                        name,
                        version: None,
                        reason: PackageReason::Unknown,
                    },
                )
            })
            .collect();
        Self { packages }
    }

    pub fn package_names(&self) -> BTreeSet<String> {
        self.packages.keys().cloned().collect()
    }

    pub fn diff(&self, after: &Self) -> PackageSnapshotDiff {
        let before_names = self.package_names();
        let after_names = after.package_names();
        PackageSnapshotDiff {
            added: after_names.difference(&before_names).cloned().collect(),
            removed: before_names.difference(&after_names).cloned().collect(),
            unchanged: before_names.intersection(&after_names).cloned().collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPackage {
    pub name: String,
    pub version: Option<String>,
    pub reason: PackageReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageReason {
    Explicit,
    Dependency,
    Unknown,
}

impl PackageReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::Dependency => "dependency",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageSnapshotDiff {
    pub added: BTreeSet<String>,
    pub removed: BTreeSet<String>,
    pub unchanged: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageTransaction {
    pub status: PackageResolutionStatus,
    pub message: Option<String>,
    pub rows: Vec<PackageTransactionRow>,
}

impl PackageTransaction {
    pub fn skipped(message: impl Into<String>) -> Self {
        Self {
            status: PackageResolutionStatus::Skipped,
            message: Some(message.into()),
            rows: Vec::new(),
        }
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            status: PackageResolutionStatus::Unavailable,
            message: Some(message.into()),
            rows: Vec::new(),
        }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            status: PackageResolutionStatus::Failed,
            message: Some(message.into()),
            rows: Vec::new(),
        }
    }
}

impl Default for PackageTransaction {
    fn default() -> Self {
        Self::skipped("no transaction resolution attempted")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageResolutionStatus {
    Skipped,
    Unavailable,
    Failed,
    Resolved,
}

impl PackageResolutionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Skipped => "skipped",
            Self::Unavailable => "unavailable",
            Self::Failed => "failed",
            Self::Resolved => "resolved",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageTransactionRow {
    pub package: String,
    pub version: Option<String>,
    pub change: PackageTransactionChange,
    pub reason: PackageReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageTransactionChange {
    Install,
}

impl PackageTransactionChange {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Install => "install",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageBackend {
    Pacman,
    Aur,
    Nix,
}

impl PackageBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            PackageBackend::Pacman => "pacman",
            PackageBackend::Aur => "aur",
            PackageBackend::Nix => "nix",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageAction {
    EnsureInstalled,
}

impl PackageAction {
    pub fn as_str(self) -> &'static str {
        match self {
            PackageAction::EnsureInstalled => "ensure-installed",
        }
    }
}

pub trait PackageExecutor {
    fn ensure_installed(&mut self, backend: PackageBackend, package: &str) -> Result<(), String>;
}

#[derive(Debug, Default)]
pub struct RecordingPackageExecutor {
    operations: Vec<PackageOperation>,
}

impl RecordingPackageExecutor {
    pub fn operations(&self) -> &[PackageOperation] {
        &self.operations
    }
}

impl PackageExecutor for RecordingPackageExecutor {
    fn ensure_installed(&mut self, backend: PackageBackend, package: &str) -> Result<(), String> {
        self.operations.push(PackageOperation {
            backend,
            action: PackageAction::EnsureInstalled,
            package: package.to_string(),
        });
        Ok(())
    }
}

pub fn write_package_operations_log(
    state_dir: &Path,
    operations: &[PackageOperation],
) -> Result<Option<PathBuf>, String> {
    if operations.is_empty() {
        return Ok(None);
    }

    fs::create_dir_all(state_dir).map_err(|err| format!("{}: {err}", state_dir.display()))?;
    let path = state_dir.join("package-operations.log");
    let mut out = String::new();
    for operation in operations {
        out.push_str(operation.backend.as_str());
        out.push(' ');
        out.push_str(operation.action.as_str());
        out.push(' ');
        out.push_str(&operation.package);
        out.push('\n');
    }
    fs::write(&path, out).map_err(|err| format!("{}: {err}", path.display()))?;
    Ok(Some(path))
}

fn parse_pacman_print_format(
    stdout: &str,
    requested: &BTreeSet<String>,
) -> Vec<PackageTransactionRow> {
    let mut rows = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let mut fields = line.splitn(2, '\t');
        let Some(name) = fields.next().filter(|value| !value.is_empty()) else {
            continue;
        };
        let version = fields
            .next()
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let reason = if requested.contains(name) {
            PackageReason::Explicit
        } else {
            PackageReason::Dependency
        };
        rows.push(PackageTransactionRow {
            package: name.to_string(),
            version,
            change: PackageTransactionChange::Install,
            reason,
        });
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_package_executor_captures_operations() {
        let mut executor = RecordingPackageExecutor::default();
        executor
            .ensure_installed(PackageBackend::Pacman, "basalt-test")
            .unwrap();

        assert_eq!(executor.operations().len(), 1);
        assert_eq!(executor.operations()[0].backend, PackageBackend::Pacman);
        assert_eq!(executor.operations()[0].package, "basalt-test");
    }

    #[test]
    fn writes_package_operations_log() {
        let base =
            std::env::temp_dir().join(format!("basalt-package-log-test-{}", std::process::id()));
        let operations = vec![PackageOperation {
            backend: PackageBackend::Pacman,
            action: PackageAction::EnsureInstalled,
            package: "basalt-test".to_string(),
        }];

        let path = write_package_operations_log(&base, &operations)
            .unwrap()
            .unwrap();
        let log = fs::read_to_string(path).unwrap();
        assert!(log.contains("pacman ensure-installed basalt-test"));

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn package_snapshot_diff_tracks_added_and_removed_packages() {
        let before =
            PackageSnapshot::from_names(BTreeSet::from(["git".to_string(), "openssl".to_string()]));
        let after =
            PackageSnapshot::from_names(BTreeSet::from(["git".to_string(), "rust".to_string()]));

        let diff = before.diff(&after);

        assert_eq!(diff.added, BTreeSet::from(["rust".to_string()]));
        assert_eq!(diff.removed, BTreeSet::from(["openssl".to_string()]));
        assert_eq!(diff.unchanged, BTreeSet::from(["git".to_string()]));
    }

    #[test]
    fn parses_pacman_print_format_transaction_rows() {
        let rows = parse_pacman_print_format(
            "git\t2.50.0-1\nperl-error\t0.17030-1\n",
            &BTreeSet::from(["git".to_string()]),
        );

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].package, "git");
        assert_eq!(rows[0].version.as_deref(), Some("2.50.0-1"));
        assert_eq!(rows[0].change, PackageTransactionChange::Install);
        assert_eq!(rows[0].reason, PackageReason::Explicit);
        assert_eq!(rows[1].reason, PackageReason::Dependency);
    }

    #[test]
    fn default_transaction_records_skipped_status() {
        let transaction = PackageTransaction::default();

        assert_eq!(transaction.status, PackageResolutionStatus::Skipped);
        assert!(transaction.message.unwrap().contains("no transaction"));
    }
}
