// Apply pipeline orchestration.

use crate::backends::aur::resolve_aur_install_transaction;
use crate::backends::nix::resolve_nix_install_transaction;
use crate::backends::pacman::{
    read_installed_package_snapshot, resolve_pacman_install_transaction,
    write_package_operations_log, HostPackageExecutor, PackageBackend, PackageExecutor,
    PackageSnapshot, PackageTransaction, RecordingPackageExecutor,
};
use crate::config::BasaltConfig;
use crate::planning::action::{plan_actions, Action};
use crate::state::db::{index_run, PackageIntent, ServiceIntent, StateDbArtifacts};
use crate::state::store::{
    write_run_record, CurrentState, HostStateReader, RunRecord, StateLock, StateReader,
};
use crate::system::services::{
    write_service_operations_log, HostServiceExecutor, RecordingServiceExecutor, ServiceExecutor,
};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs as unix_fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct ApplySummary {
    pub actions: Vec<Action>,
    pub written_files: Vec<PathBuf>,
    pub package_operations_path: Option<PathBuf>,
    pub service_operations_path: Option<PathBuf>,
    pub backup_dir: PathBuf,
    pub run_path: PathBuf,
    pub latest_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceExecutorMode {
    Record,
    Host,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageExecutorMode {
    Record,
    Host,
}

impl PackageExecutorMode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "record" => Ok(Self::Record),
            "host" => Ok(Self::Host),
            other => Err(format!(
                "unknown package executor `{other}`; expected `record` or `host`"
            )),
        }
    }
}

impl ServiceExecutorMode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "record" => Ok(Self::Record),
            "host" => Ok(Self::Host),
            other => Err(format!(
                "unknown service executor `{other}`; expected `record` or `host`"
            )),
        }
    }
}

pub fn dry_run_actions(config: &BasaltConfig, current: &CurrentState) -> Vec<Action> {
    plan_actions(config, current)
}

pub fn write_dry_run_record(
    state_dir: &Path,
    config_dir: PathBuf,
    config: &BasaltConfig,
    actions: Vec<Action>,
    current: &CurrentState,
) -> Result<(std::path::PathBuf, std::path::PathBuf), String> {
    let record = RunRecord::dry_run(config_dir, actions, current);
    let transactions = resolve_package_transactions_for_actions(&record.actions)?;
    let (run_path, latest_path) = write_run_record(state_dir, &record)?;
    index_run(
        state_dir,
        &record,
        &StateDbArtifacts {
            run_json_path: run_path.clone(),
            latest_json_path: latest_path.clone(),
            package_intent: package_intent_from_config(config),
            service_intent: service_intent_from_config(config),
            pacman_snapshot_before: Some(PackageSnapshot::from_names(
                current.pacman_packages.clone(),
            )),
            pacman_snapshot_after: Some(PackageSnapshot::from_names(
                current.pacman_packages.clone(),
            )),
            enabled_services_before: Some(current.enabled_services.clone()),
            enabled_services_after: Some(current.enabled_services.clone()),
            pacman_transaction: Some(transactions.pacman),
            aur_transaction: Some(transactions.aur),
            nix_transaction: Some(transactions.nix),
            ..StateDbArtifacts::default()
        },
    )?;
    Ok((run_path, latest_path))
}

pub fn acquire_apply_lock(state_dir: &Path, mode: &str) -> Result<StateLock, String> {
    StateLock::acquire(state_dir, mode)
}

pub fn apply_supported_config(
    state_dir: &Path,
    config_dir: PathBuf,
    root_dir: &Path,
    config: &BasaltConfig,
    current: &CurrentState,
    package_executor: PackageExecutorMode,
    service_executor: ServiceExecutorMode,
) -> Result<ApplySummary, String> {
    if service_executor == ServiceExecutorMode::Host && root_dir != Path::new("/") {
        return Err("`--service-executor host` requires `--root /`".to_string());
    }

    let actions = plan_actions(config, current);
    validate_package_executor_policy(&actions, root_dir, package_executor)?;
    let unsupported: Vec<&Action> = actions
        .iter()
        .filter(|action| {
            !matches!(
                action.domain.as_str(),
                "system" | "files" | "services" | "packages"
            )
        })
        .collect();
    if !unsupported.is_empty() {
        let mut message = String::from("real apply has no executor for these actions yet:");
        for action in unsupported {
            message.push_str("\n- ");
            message.push_str(&action.id);
        }
        message.push_str("\nRun `basalt apply --dry-run` for the full plan.");
        return Err(message);
    }

    let lock = acquire_apply_lock(state_dir, "apply")?;
    let backup_dir = state_dir
        .join("backups")
        .join(format!("apply-{}", millis_since_epoch()));
    fs::create_dir_all(&backup_dir).map_err(|err| format!("{}: {err}", backup_dir.display()))?;

    let mut written_files = Vec::new();
    if let Some(system) = &config.system {
        if has_action(&actions, "system.hostname") {
            write_with_backup(
                root_dir,
                &backup_dir,
                "etc/hostname",
                format!("{}\n", system.hostname).as_bytes(),
                &mut written_files,
            )?;
        }

        if has_action(&actions, "system.locale") {
            let locale = system
                .locale
                .as_ref()
                .expect("system.locale action requires locale config");
            write_with_backup(
                root_dir,
                &backup_dir,
                "etc/locale.conf",
                format!("LANG={locale}\n").as_bytes(),
                &mut written_files,
            )?;
        }

        if has_action(&actions, "system.keymap") {
            let keymap = system
                .keymap
                .as_ref()
                .expect("system.keymap action requires keymap config");
            write_with_backup(
                root_dir,
                &backup_dir,
                "etc/vconsole.conf",
                format!("KEYMAP={keymap}\n").as_bytes(),
                &mut written_files,
            )?;
        }

        if has_action(&actions, "system.timezone") {
            let timezone = system
                .timezone
                .as_ref()
                .expect("system.timezone action requires timezone config");
            set_timezone(root_dir, &backup_dir, timezone, &mut written_files)?;
        }
    }

    if let Some(files) = &config.files {
        for file in &files.managed {
            let relative_path = managed_file_relative_path(&file.path)?;
            if !has_action(&actions, &format!("files.managed.{relative_path}")) {
                continue;
            }
            write_with_backup(
                root_dir,
                &backup_dir,
                &relative_path,
                file.content.as_bytes(),
                &mut written_files,
            )?;
            if let Some(mode) = &file.mode {
                set_mode(root_dir, &relative_path, mode)?;
            }
        }
    }

    let package_operations_path = apply_package_operations(state_dir, &actions, package_executor)?;
    let pacman_snapshot_after = pacman_snapshot_after(current, package_executor);
    let transactions = resolve_package_transactions_for_actions(&actions)?;
    let service_operations_path = apply_service_operations(state_dir, &actions, service_executor)?;
    let enabled_services_after = service_snapshot_after(current, service_executor)?;

    let record = RunRecord::apply(config_dir, actions.clone(), current);
    let (run_path, latest_path) = write_run_record(state_dir, &record)?;
    index_run(
        state_dir,
        &record,
        &StateDbArtifacts {
            run_json_path: run_path.clone(),
            latest_json_path: latest_path.clone(),
            package_intent: package_intent_from_config(config),
            service_intent: service_intent_from_config(config),
            package_operations_path: package_operations_path.clone(),
            service_operations_path: service_operations_path.clone(),
            backup_dir: Some(backup_dir.clone()),
            pacman_snapshot_before: Some(PackageSnapshot::from_names(
                current.pacman_packages.clone(),
            )),
            pacman_snapshot_after: Some(pacman_snapshot_after),
            enabled_services_before: Some(current.enabled_services.clone()),
            enabled_services_after: Some(enabled_services_after),
            pacman_transaction: Some(transactions.pacman),
            aur_transaction: Some(transactions.aur),
            nix_transaction: Some(transactions.nix),
        },
    )?;
    let _ = lock.path();
    drop(lock);

    Ok(ApplySummary {
        actions,
        written_files,
        package_operations_path,
        service_operations_path,
        backup_dir,
        run_path,
        latest_path,
    })
}

fn package_intent_from_config(config: &BasaltConfig) -> Vec<PackageIntent> {
    let mut intent = Vec::new();
    if let Some(packages) = &config.packages {
        intent.extend(packages.pacman.iter().map(|package| PackageIntent {
            backend: PackageBackend::Pacman,
            package: package.to_string(),
        }));
        intent.extend(packages.aur.iter().map(|package| PackageIntent {
            backend: PackageBackend::Aur,
            package: package.to_string(),
        }));
        intent.extend(packages.nix.iter().map(|package| PackageIntent {
            backend: PackageBackend::Nix,
            package: package.to_string(),
        }));
    }
    intent
}

fn service_intent_from_config(config: &BasaltConfig) -> Vec<ServiceIntent> {
    let mut intent = Vec::new();
    if let Some(services) = &config.services {
        intent.extend(services.enable.iter().map(|service| ServiceIntent {
            action: "enable".to_string(),
            service: service.to_string(),
        }));
        intent.extend(services.disable.iter().map(|service| ServiceIntent {
            action: "disable".to_string(),
            service: service.to_string(),
        }));
    }
    intent
}

struct PackageTransactions {
    pacman: PackageTransaction,
    aur: PackageTransaction,
    nix: PackageTransaction,
}

fn resolve_package_transactions_for_actions(
    actions: &[Action],
) -> Result<PackageTransactions, String> {
    Ok(PackageTransactions {
        pacman: resolve_pacman_install_transaction(&packages_for_backend(
            actions,
            "packages.pacman.",
        ))?,
        aur: resolve_aur_install_transaction(&packages_for_backend(actions, "packages.aur."))?,
        nix: resolve_nix_install_transaction(&packages_for_backend(actions, "packages.nix."))?,
    })
}

fn packages_for_backend(actions: &[Action], prefix: &str) -> Vec<String> {
    actions
        .iter()
        .filter_map(|action| action.id.strip_prefix(prefix).map(ToOwned::to_owned))
        .collect()
}

fn apply_package_operations(
    state_dir: &Path,
    actions: &[Action],
    mode: PackageExecutorMode,
) -> Result<Option<PathBuf>, String> {
    match mode {
        PackageExecutorMode::Record => {
            let mut executor = RecordingPackageExecutor::default();
            apply_package_actions(actions, &mut executor)?;
            write_package_operations_log(state_dir, executor.operations())
        }
        PackageExecutorMode::Host => {
            let mut executor = HostPackageExecutor::default();
            apply_package_actions(actions, &mut executor)?;
            write_package_operations_log(state_dir, executor.operations())
        }
    }
}

fn apply_package_actions(
    actions: &[Action],
    executor: &mut dyn PackageExecutor,
) -> Result<(), String> {
    for action in actions {
        if let Some(package) = action.id.strip_prefix("packages.pacman.") {
            executor.ensure_installed(PackageBackend::Pacman, package)?;
        } else if let Some(package) = action.id.strip_prefix("packages.aur.") {
            executor.ensure_installed(PackageBackend::Aur, package)?;
        } else if let Some(package) = action.id.strip_prefix("packages.nix.") {
            executor.ensure_installed(PackageBackend::Nix, package)?;
        }
    }
    Ok(())
}

fn validate_package_executor_policy(
    actions: &[Action],
    root_dir: &Path,
    mode: PackageExecutorMode,
) -> Result<(), String> {
    if mode == PackageExecutorMode::Record {
        return Ok(());
    }

    if !actions.iter().any(|action| action.domain == "packages") {
        return Ok(());
    }

    if root_dir != Path::new("/") {
        return Err("`--package-executor host` requires `--root /`".to_string());
    }

    let unsupported: Vec<&str> = actions
        .iter()
        .filter_map(|action| {
            if action.id.starts_with("packages.aur.") {
                Some("AUR")
            } else if action.id.starts_with("packages.nix.") {
                Some("Nix")
            } else {
                None
            }
        })
        .collect();
    if !unsupported.is_empty() {
        return Err(
            "`--package-executor host` currently supports pacman packages only; AUR and Nix host execution are not implemented yet"
                .to_string(),
        );
    }

    Ok(())
}

fn pacman_snapshot_after(current: &CurrentState, mode: PackageExecutorMode) -> PackageSnapshot {
    match mode {
        PackageExecutorMode::Record => PackageSnapshot::from_names(current.pacman_packages.clone()),
        PackageExecutorMode::Host => read_installed_package_snapshot(),
    }
}

fn service_snapshot_after(
    current: &CurrentState,
    mode: ServiceExecutorMode,
) -> Result<std::collections::BTreeSet<String>, String> {
    match mode {
        ServiceExecutorMode::Record => Ok(current.enabled_services.clone()),
        ServiceExecutorMode::Host => Ok(HostStateReader.read_current_state()?.enabled_services),
    }
}

fn write_with_backup(
    root_dir: &Path,
    backup_dir: &Path,
    relative_path: &str,
    contents: &[u8],
    written_files: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let target = root_dir.join(relative_path);
    crate::recovery::restore::backup_target(&target, backup_dir, relative_path)?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
    }
    fs::write(&target, contents).map_err(|err| format!("{}: {err}", target.display()))?;
    written_files.push(target);
    Ok(())
}

fn has_action(actions: &[Action], id: &str) -> bool {
    actions.iter().any(|action| action.id == id)
}

fn set_timezone(
    root_dir: &Path,
    backup_dir: &Path,
    timezone: &str,
    written_files: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let zoneinfo = root_dir.join("usr/share/zoneinfo").join(timezone);
    if !zoneinfo.exists() {
        return Err(format!(
            "{}: timezone `{timezone}` is not available in target root",
            zoneinfo.display()
        ));
    }

    let target = root_dir.join("etc/localtime");
    crate::recovery::restore::backup_target(&target, backup_dir, "etc/localtime")?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
    }
    if target.exists() || target.is_symlink() {
        fs::remove_file(&target).map_err(|err| format!("{}: {err}", target.display()))?;
    }

    #[cfg(unix)]
    {
        unix_fs::symlink(format!("/usr/share/zoneinfo/{timezone}"), &target)
            .map_err(|err| format!("{}: {err}", target.display()))?;
    }
    #[cfg(not(unix))]
    {
        fs::copy(&zoneinfo, &target).map_err(|err| format!("{}: {err}", target.display()))?;
    }

    written_files.push(target);
    Ok(())
}

fn managed_file_relative_path(path: &str) -> Result<String, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("managed file path cannot be empty".to_string());
    }
    let relative = path.trim_start_matches('/');
    let candidate = Path::new(relative);
    if candidate
        .components()
        .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(format!(
            "managed file path `{path}` must stay inside the target root"
        ));
    }
    Ok(relative.to_string())
}

fn set_mode(root_dir: &Path, relative_path: &str, mode: &str) -> Result<(), String> {
    let target = root_dir.join(relative_path);
    #[cfg(unix)]
    {
        let parsed = u32::from_str_radix(mode, 8)
            .map_err(|err| format!("invalid mode `{mode}` for {}: {err}", target.display()))?;
        let permissions = fs::Permissions::from_mode(parsed);
        fs::set_permissions(&target, permissions)
            .map_err(|err| format!("{}: {err}", target.display()))?;
    }
    #[cfg(not(unix))]
    {
        let _ = (target, mode);
    }
    Ok(())
}

fn apply_service_operations(
    state_dir: &Path,
    actions: &[Action],
    mode: ServiceExecutorMode,
) -> Result<Option<PathBuf>, String> {
    let mut operations = RecordingServiceExecutor::default();
    for action in actions {
        if let Some(service) = action.id.strip_prefix("services.enable.") {
            operations.enable(service)?;
        } else if let Some(service) = action.id.strip_prefix("services.disable.") {
            operations.disable(service)?;
        }
    }

    let path = write_service_operations_log(state_dir, operations.operations())?;
    if operations.operations().is_empty() {
        return Ok(path);
    }

    match mode {
        ServiceExecutorMode::Record => {}
        ServiceExecutorMode::Host => {
            let mut executor = HostServiceExecutor;
            executor.prepare()?;
            for operation in operations.operations() {
                match operation.action {
                    crate::system::services::ServiceAction::Enable => {
                        executor.enable(&operation.service)?
                    }
                    crate::system::services::ServiceAction::Disable => {
                        executor.disable(&operation.service)?
                    }
                }
            }
        }
    }

    Ok(path)
}

fn millis_since_epoch() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::validate_config_dir;

    #[test]
    fn applies_system_identity_to_target_root() {
        let base = std::env::temp_dir().join(format!("basalt-apply-test-{}", millis_since_epoch()));
        let root = base.join("root");
        let state = base.join("state");
        fs::create_dir_all(root.join("usr/share/zoneinfo")).unwrap();
        fs::write(root.join("usr/share/zoneinfo/UTC"), "UTC").unwrap();
        fs::create_dir_all(root.join("etc")).unwrap();
        fs::write(root.join("etc/hostname"), "old-host\n").unwrap();

        let config_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("configs/fixtures/valid-system-apply");
        let config = validate_config_dir(&config_dir).unwrap();
        let summary = apply_supported_config(
            &state,
            config_dir,
            &root,
            &config,
            &CurrentState::default(),
            PackageExecutorMode::Record,
            ServiceExecutorMode::Record,
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(root.join("etc/hostname")).unwrap(),
            "basalt-vm\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("etc/locale.conf")).unwrap(),
            "LANG=en_US.UTF-8\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("etc/vconsole.conf")).unwrap(),
            "KEYMAP=us\n"
        );
        assert!(root.join("etc/localtime").exists() || root.join("etc/localtime").is_symlink());
        assert!(summary.backup_dir.join("etc__hostname").exists());
        assert!(summary
            .backup_dir
            .join(crate::recovery::restore::BACKUP_MANIFEST)
            .exists());
        assert!(summary.run_path.exists());
        assert!(summary.latest_path.exists());
        assert!(!state.join("basalt.lock").exists());

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn records_package_actions_from_config() {
        let base = std::env::temp_dir().join(format!(
            "basalt-package-config-test-{}",
            millis_since_epoch()
        ));
        let root = base.join("root");
        let state = base.join("state");
        let config_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("configs/fixtures/valid-package-recording");
        let config = validate_config_dir(&config_dir).unwrap();
        let summary = apply_supported_config(
            &state,
            config_dir,
            &root,
            &config,
            &CurrentState::default(),
            PackageExecutorMode::Record,
            ServiceExecutorMode::Record,
        )
        .unwrap();

        let package_log =
            fs::read_to_string(summary.package_operations_path.expect("package log")).unwrap();
        assert!(package_log.contains("pacman ensure-installed basalt-test-package"));
        assert!(package_log.contains("aur ensure-installed basalt-test-aur"));
        assert!(package_log.contains("nix ensure-installed hello"));

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn applies_managed_files_to_target_root() {
        let base = std::env::temp_dir().join(format!("basalt-files-test-{}", millis_since_epoch()));
        let root = base.join("root");
        let state = base.join("state");
        fs::create_dir_all(root.join("usr/share/zoneinfo")).unwrap();
        fs::write(root.join("usr/share/zoneinfo/UTC"), "UTC").unwrap();
        fs::create_dir_all(root.join("etc/basalt")).unwrap();
        fs::write(root.join("etc/basalt/motd"), "old\n").unwrap();

        let config_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("configs/fixtures/valid-managed-files");
        let config = validate_config_dir(&config_dir).unwrap();
        let summary = apply_supported_config(
            &state,
            config_dir,
            &root,
            &config,
            &CurrentState::default(),
            PackageExecutorMode::Record,
            ServiceExecutorMode::Record,
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(root.join("etc/basalt/motd")).unwrap(),
            "Basalt managed file\n"
        );
        assert!(summary.backup_dir.join("etc__basalt__motd").exists());
        assert!(summary
            .actions
            .iter()
            .any(|action| action.id == "files.managed.etc/basalt/motd"));
        assert!(summary.service_operations_path.is_some());

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn skips_managed_file_write_when_current_state_matches() {
        let base = std::env::temp_dir().join(format!(
            "basalt-files-idempotent-test-{}",
            millis_since_epoch()
        ));
        let root = base.join("root");
        let state = base.join("state");
        fs::create_dir_all(root.join("usr/share/zoneinfo")).unwrap();
        fs::write(root.join("usr/share/zoneinfo/UTC"), "UTC").unwrap();
        fs::create_dir_all(root.join("etc/basalt")).unwrap();
        fs::write(root.join("etc/basalt/motd"), "Basalt managed file\n").unwrap();

        let config_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("configs/fixtures/valid-managed-files");
        let config = validate_config_dir(&config_dir).unwrap();
        let current = CurrentState {
            hostname: Some("basalt-vm".to_string()),
            timezone: Some("UTC".to_string()),
            locale: Some("en_US.UTF-8".to_string()),
            keymap: Some("us".to_string()),
            managed_files: std::collections::BTreeMap::from([(
                "/etc/basalt/motd".to_string(),
                "Basalt managed file\n".to_string(),
            )]),
            ..CurrentState::default()
        };
        let summary = apply_supported_config(
            &state,
            config_dir,
            &root,
            &config,
            &current,
            PackageExecutorMode::Record,
            ServiceExecutorMode::Record,
        )
        .unwrap();

        assert!(summary.written_files.is_empty());
        assert!(!summary
            .actions
            .iter()
            .any(|action| action.id == "files.managed.etc/basalt/motd"));

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn rejects_managed_file_path_traversal() {
        let err = managed_file_relative_path("../etc/passwd").unwrap_err();
        assert!(err.contains("target root"));
    }

    #[test]
    fn records_service_operations_without_running_systemctl() {
        let base = std::env::temp_dir().join(format!(
            "basalt-service-apply-test-{}",
            millis_since_epoch()
        ));
        let state = base.join("state");
        let actions = vec![
            Action {
                id: "services.enable.basalt-example".to_string(),
                domain: "services".to_string(),
                description: "enable service `basalt-example`".to_string(),
                risk: crate::planning::action::Risk::Medium,
            },
            Action {
                id: "services.disable.old-example".to_string(),
                domain: "services".to_string(),
                description: "disable service `old-example`".to_string(),
                risk: crate::planning::action::Risk::High,
            },
        ];

        let path = apply_service_operations(&state, &actions, ServiceExecutorMode::Record)
            .unwrap()
            .unwrap();
        let log = fs::read_to_string(path).unwrap();
        assert!(log.contains("enable basalt-example"));
        assert!(log.contains("disable old-example"));

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn host_service_executor_requires_real_root() {
        let err = apply_supported_config(
            Path::new("/tmp/basalt-unused-state"),
            PathBuf::from("config"),
            Path::new("/tmp/basalt-root"),
            &BasaltConfig::default(),
            &CurrentState::default(),
            PackageExecutorMode::Record,
            ServiceExecutorMode::Host,
        )
        .unwrap_err();

        assert!(err.contains("requires `--root /`"));
    }

    #[test]
    fn host_package_executor_requires_real_root_for_package_actions() {
        let base = std::env::temp_dir().join(format!(
            "basalt-package-host-root-test-{}",
            millis_since_epoch()
        ));
        let root = base.join("root");
        let state = base.join("state");
        let config_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("configs/fixtures/valid-package-recording");
        let config = validate_config_dir(&config_dir).unwrap();

        let err = apply_supported_config(
            &state,
            config_dir,
            &root,
            &config,
            &CurrentState::default(),
            PackageExecutorMode::Host,
            ServiceExecutorMode::Record,
        )
        .unwrap_err();

        assert!(err.contains("`--package-executor host` requires `--root /`"));
        assert!(!root.join("etc/hostname").exists());

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn host_package_executor_rejects_aur_and_nix_until_real_mutation_exists() {
        let config_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("configs/fixtures/valid-package-recording");
        let config = validate_config_dir(&config_dir).unwrap();

        let err = apply_supported_config(
            Path::new("/tmp/basalt-unused-state"),
            config_dir,
            Path::new("/"),
            &config,
            &CurrentState::default(),
            PackageExecutorMode::Host,
            ServiceExecutorMode::Record,
        )
        .unwrap_err();

        assert!(err.contains("not implemented yet"));
        assert!(err.contains("AUR and Nix"));
    }

    #[test]
    fn records_package_operations_without_running_pacman() {
        let base = std::env::temp_dir().join(format!(
            "basalt-package-apply-test-{}",
            millis_since_epoch()
        ));
        let state = base.join("state");
        let actions = vec![
            Action {
                id: "packages.pacman.basalt-test".to_string(),
                domain: "packages".to_string(),
                description: "ensure pacman package `basalt-test` is installed".to_string(),
                risk: crate::planning::action::Risk::High,
            },
            Action {
                id: "packages.aur.example-aur".to_string(),
                domain: "packages".to_string(),
                description: "ensure AUR package `example-aur` is installed".to_string(),
                risk: crate::planning::action::Risk::Medium,
            },
        ];

        let path = apply_package_operations(&state, &actions, PackageExecutorMode::Record)
            .unwrap()
            .unwrap();
        let log = fs::read_to_string(path).unwrap();
        assert!(log.contains("pacman ensure-installed basalt-test"));
        assert!(log.contains("aur ensure-installed example-aur"));

        let _ = fs::remove_dir_all(base);
    }
}
