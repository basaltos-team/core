// pacman query, plan, install, remove, and hold handling.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::process::command::run_capture;

pub fn read_installed_packages() -> BTreeSet<String> {
    let Ok(output) = run_capture("pacman", &["-Qq"]) else {
        return BTreeSet::new();
    };

    if output.status_code != Some(0) {
        let _ = output.stderr;
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
}
