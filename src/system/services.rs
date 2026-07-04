// systemd unit, timer, tmpfiles, and sysusers management.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::process::command::run_capture;

pub fn read_enabled_services() -> BTreeSet<String> {
    let Ok(output) = run_capture(
        "systemctl",
        &[
            "list-unit-files",
            "--state=enabled",
            "--no-legend",
            "--no-pager",
        ],
    ) else {
        return BTreeSet::new();
    };

    if output.status_code != Some(0) {
        let _ = output.stderr;
        return BTreeSet::new();
    }

    let mut services = BTreeSet::new();
    for line in output.stdout.lines() {
        let Some(unit) = line.split_whitespace().next() else {
            continue;
        };
        if unit.is_empty() {
            continue;
        }
        services.insert(unit.to_string());
        if let Some(stripped) = unit.strip_suffix(".service") {
            services.insert(stripped.to_string());
        }
    }

    services
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceOperation {
    pub action: ServiceAction,
    pub service: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceAction {
    Enable,
    Disable,
}

impl ServiceAction {
    pub fn as_str(self) -> &'static str {
        match self {
            ServiceAction::Enable => "enable",
            ServiceAction::Disable => "disable",
        }
    }
}

pub trait ServiceExecutor {
    fn prepare(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn enable(&mut self, service: &str) -> Result<(), String>;
    fn disable(&mut self, service: &str) -> Result<(), String>;
}

#[derive(Debug, Default)]
pub struct RecordingServiceExecutor {
    operations: Vec<ServiceOperation>,
}

impl RecordingServiceExecutor {
    pub fn operations(&self) -> &[ServiceOperation] {
        &self.operations
    }
}

impl ServiceExecutor for RecordingServiceExecutor {
    fn enable(&mut self, service: &str) -> Result<(), String> {
        self.operations.push(ServiceOperation {
            action: ServiceAction::Enable,
            service: service.to_string(),
        });
        Ok(())
    }

    fn disable(&mut self, service: &str) -> Result<(), String> {
        self.operations.push(ServiceOperation {
            action: ServiceAction::Disable,
            service: service.to_string(),
        });
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HostServiceExecutor;

impl HostServiceExecutor {
    fn run_systemctl(args: &[&str]) -> Result<(), String> {
        let output = run_capture("systemctl", args)?;
        if output.status_code == Some(0) {
            return Ok(());
        }

        let stderr = output.stderr.trim();
        if stderr.is_empty() {
            Err(format!("systemctl {} failed", args.join(" ")))
        } else {
            Err(format!("systemctl {} failed: {stderr}", args.join(" ")))
        }
    }
}

impl ServiceExecutor for HostServiceExecutor {
    fn prepare(&mut self) -> Result<(), String> {
        Self::run_systemctl(&["daemon-reload"])
    }

    fn enable(&mut self, service: &str) -> Result<(), String> {
        Self::run_systemctl(&["enable", service])
    }

    fn disable(&mut self, service: &str) -> Result<(), String> {
        Self::run_systemctl(&["disable", service])
    }
}

pub fn write_service_operations_log(
    state_dir: &Path,
    operations: &[ServiceOperation],
) -> Result<Option<PathBuf>, String> {
    if operations.is_empty() {
        return Ok(None);
    }

    fs::create_dir_all(state_dir).map_err(|err| format!("{}: {err}", state_dir.display()))?;
    let path = state_dir.join("service-operations.log");
    let mut out = String::new();
    for operation in operations {
        out.push_str(operation.action.as_str());
        out.push(' ');
        out.push_str(&operation.service);
        out.push('\n');
    }
    fs::write(&path, out).map_err(|err| format!("{}: {err}", path.display()))?;
    Ok(Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_executor_captures_operations() {
        let mut executor = RecordingServiceExecutor::default();
        executor.enable("NetworkManager").unwrap();
        executor.disable("sshd").unwrap();

        assert_eq!(executor.operations().len(), 2);
        assert_eq!(executor.operations()[0].action, ServiceAction::Enable);
        assert_eq!(executor.operations()[0].service, "NetworkManager");
        assert_eq!(executor.operations()[1].action, ServiceAction::Disable);
    }

    #[test]
    fn writes_service_operations_log() {
        let base =
            std::env::temp_dir().join(format!("basalt-service-log-test-{}", std::process::id()));
        let operations = vec![
            ServiceOperation {
                action: ServiceAction::Enable,
                service: "basalt-example".to_string(),
            },
            ServiceOperation {
                action: ServiceAction::Disable,
                service: "old-example".to_string(),
            },
        ];

        let path = write_service_operations_log(&base, &operations)
            .unwrap()
            .unwrap();
        let log = fs::read_to_string(&path).unwrap();
        assert!(log.contains("enable basalt-example"));
        assert!(log.contains("disable old-example"));

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn host_executor_reports_failed_systemctl() {
        let err =
            HostServiceExecutor::run_systemctl(&["--definitely-not-a-systemctl-flag"]).unwrap_err();
        assert!(err.contains("systemctl"));
    }
}
