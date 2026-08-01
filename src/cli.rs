// Command definitions shared with docs, shell completions, and tests.

use std::path::{Path, PathBuf};

use crate::state::store::{
    read_configured_managed_files, CurrentState, HostStateReader, StateReader,
    TargetRootStateReader,
};

const INSTALLED_CONFIG_DIR: &str = "/etc/basalt/install-config";

pub fn run(args: Vec<String>) -> i32 {
    match parse_args(&args) {
        Ok(Command::Validate { config_dir }) => {
            match crate::config::validate_config_dir(&config_dir) {
                Ok(config) => {
                    println!(
                    "Basalt config valid: {} domain(s), {} package declaration(s), {} enabled service(s), {} workspace(s)",
                    config.domain_count(),
                    config.package_count(),
                    config.service_count(),
                    config.workspace_count()
                );
                    0
                }
                Err(errs) => {
                    eprintln!("Basalt config invalid:");
                    for err in errs {
                        eprintln!("- {err}");
                    }
                    1
                }
            }
        }
        Ok(Command::Diff {
            config_dir,
            root_dir,
        }) => match crate::config::validate_config_dir(&config_dir) {
            Ok(config) => match read_apply_current_state(&root_dir, &config) {
                Ok(current) => {
                    print!(
                        "{}",
                        crate::planning::report::render_diff(&config, &current)
                    );
                    0
                }
                Err(err) => {
                    eprintln!("failed to read current state: {err}");
                    1
                }
            },
            Err(errs) => {
                eprintln!("Basalt config invalid:");
                for err in errs {
                    eprintln!("- {err}");
                }
                1
            }
        },
        Ok(Command::ApplyDryRun {
            config_dir,
            state_dir,
            rebuild,
        }) => match crate::config::validate_config_dir(&config_dir) {
            Ok(config) => match validate_rebuild_safety_policy_if_needed(rebuild, &config) {
                Ok(()) => match HostStateReader.read_current_state() {
                    Ok(current) => {
                        let lock = match crate::apply::acquire_apply_lock(&state_dir, "dry-run") {
                            Ok(lock) => lock,
                            Err(err) => {
                                eprintln!("failed to acquire apply lock: {err}");
                                return 1;
                            }
                        };
                        let actions = crate::apply::dry_run_actions(&config, &current);
                        print!("{}", crate::planning::report::render_dry_run(&actions));
                        match crate::apply::write_dry_run_record(
                            &state_dir, config_dir, &config, actions, &current,
                        ) {
                            Ok((run_path, latest_path)) => {
                                println!();
                                println!("Run record written:");
                                println!("- {}", run_path.display());
                                println!("- {}", latest_path.display());
                                println!("State index written:");
                                println!("- {}", state_dir.join("state.db").display());
                                println!("Apply lock path: {}", lock.path().display());
                            }
                            Err(err) => {
                                eprintln!("failed to write run record: {err}");
                                return 1;
                            }
                        }
                        0
                    }
                    Err(err) => {
                        eprintln!("failed to read current state: {err}");
                        1
                    }
                },
                Err(errs) => {
                    eprintln!("Basalt rebuild rejected:");
                    for err in errs {
                        eprintln!("- {err}");
                    }
                    1
                }
            },
            Err(errs) => {
                eprintln!("Basalt config invalid:");
                for err in errs {
                    eprintln!("- {err}");
                }
                1
            }
        },
        Ok(Command::ApplyCheck {
            config_dir,
            root_dir,
        }) => match crate::config::validate_config_dir(&config_dir) {
            Ok(config) => match read_apply_current_state(&root_dir, &config) {
                Ok(current) => {
                    let actions = crate::apply::dry_run_actions(&config, &current);
                    print!("{}", crate::planning::report::render_check(&actions));
                    if actions.is_empty() {
                        0
                    } else {
                        1
                    }
                }
                Err(err) => {
                    eprintln!("failed to read current state: {err}");
                    1
                }
            },
            Err(errs) => {
                eprintln!("Basalt config invalid:");
                for err in errs {
                    eprintln!("- {err}");
                }
                1
            }
        },
        Ok(Command::Apply {
            config_dir,
            state_dir,
            root_dir,
            package_executor,
            service_executor,
            rebuild,
        }) => match crate::config::validate_config_dir(&config_dir) {
            Ok(config) => match validate_rebuild_safety_policy_if_needed(rebuild, &config) {
                Ok(()) => match read_apply_current_state(&root_dir, &config) {
                    Ok(current) => match crate::apply::apply_supported_config(
                        &state_dir,
                        config_dir,
                        &root_dir,
                        &config,
                        &current,
                        package_executor,
                        service_executor,
                    ) {
                        Ok(summary) => {
                            println!("Basalt apply");
                            println!();
                            println!("Applied {} action(s).", summary.actions.len());
                            println!("Written files:");
                            if summary.written_files.is_empty() {
                                println!("- none");
                            } else {
                                for path in summary.written_files {
                                    println!("- {}", path.display());
                                }
                            }
                            println!("Backup directory: {}", summary.backup_dir.display());
                            if let Some(path) = summary.package_operations_path {
                                println!("Package operations recorded:");
                                println!("- {}", path.display());
                            }
                            if let Some(path) = summary.service_operations_path {
                                println!("Service operations recorded:");
                                println!("- {}", path.display());
                            }
                            println!("Run record written:");
                            println!("- {}", summary.run_path.display());
                            println!("- {}", summary.latest_path.display());
                            println!("State index written:");
                            println!("- {}", state_dir.join("state.db").display());
                            0
                        }
                        Err(err) => {
                            eprintln!("apply failed: {err}");
                            1
                        }
                    },
                    Err(err) => {
                        eprintln!("failed to read current state: {err}");
                        1
                    }
                },
                Err(errs) => {
                    eprintln!("Basalt rebuild rejected:");
                    for err in errs {
                        eprintln!("- {err}");
                    }
                    1
                }
            },
            Err(errs) => {
                eprintln!("Basalt config invalid:");
                for err in errs {
                    eprintln!("- {err}");
                }
                1
            }
        },
        Ok(Command::Schema) => match std::env::current_dir()
            .map_err(|err| err.to_string())
            .and_then(|cwd| crate::config::schema::generate_schema_artifacts(&cwd))
        {
            Ok(paths) => {
                println!("Generated schema artifacts:");
                for path in paths {
                    println!("- {}", path.display());
                }
                0
            }
            Err(err) => {
                eprintln!("schema generation failed: {err}");
                1
            }
        },
        Ok(Command::History { state_dir, limit }) => {
            match crate::state::db::history_rows(&state_dir, limit) {
                Ok(rows) => {
                    print!("{}", crate::state::db::render_history(&rows));
                    0
                }
                Err(err) => {
                    eprintln!("history failed: {err}");
                    1
                }
            }
        }
        Ok(Command::InspectRun { state_dir, run_id }) => {
            match crate::state::db::inspect_run(&state_dir, run_id.as_deref()) {
                Ok(inspection) => {
                    print!("{}", crate::state::db::render_run_inspection(&inspection));
                    0
                }
                Err(err) => {
                    eprintln!("inspect-run failed: {err}");
                    1
                }
            }
        }
        Ok(Command::PackageHistory {
            state_dir,
            package,
            limit,
        }) => match crate::state::db::package_history_rows(&state_dir, &package, limit) {
            Ok(rows) => {
                print!(
                    "{}",
                    crate::state::db::render_package_history(&package, &rows)
                );
                0
            }
            Err(err) => {
                eprintln!("package-history failed: {err}");
                1
            }
        },
        Ok(Command::ServiceHistory {
            state_dir,
            service,
            limit,
        }) => match crate::state::db::service_history_rows(&state_dir, &service, limit) {
            Ok(rows) => {
                print!(
                    "{}",
                    crate::state::db::render_service_history(&service, &rows)
                );
                0
            }
            Err(err) => {
                eprintln!("service-history failed: {err}");
                1
            }
        },
        Ok(Command::Doctor) => {
            let report = doctor_report();
            print!("{}", render_doctor_report(&report));
            if report.iter().any(|check| check.required && !check.present) {
                1
            } else {
                0
            }
        }
        Ok(Command::WorkspaceGenerate {
            config_dir,
            output_dir,
        }) => match crate::config::validate_config_dir(&config_dir) {
            Ok(config) => match crate::workspaces::generate_workspace_artifacts(
                config.workspaces.as_ref(),
                &output_dir,
            ) {
                Ok(summary) => {
                    println!("Basalt workspace generate");
                    println!();
                    if summary.artifacts.is_empty() {
                        println!("Generated workspaces: none");
                    } else {
                        println!("Generated workspaces:");
                        for artifact in summary.artifacts {
                            println!(
                                "- {}: {} -> {}",
                                artifact.name,
                                artifact.workspace_path,
                                artifact.devenv_nix.display()
                            );
                        }
                    }
                    println!("Workspace state manifest:");
                    println!("- {}", summary.state_manifest.display());
                    0
                }
                Err(err) => {
                    eprintln!("workspace generation failed: {err}");
                    1
                }
            },
            Err(errs) => {
                eprintln!("Basalt config invalid:");
                for err in errs {
                    eprintln!("- {err}");
                }
                1
            }
        },
        Ok(Command::WorkspaceCheck { manifest }) => {
            match crate::workspaces::check_workspace_state_manifest(&manifest) {
                Ok(check) => {
                    print!(
                        "{}",
                        crate::workspaces::render_workspace_manifest_check(&check)
                    );
                    if check.is_ok() {
                        0
                    } else {
                        1
                    }
                }
                Err(err) => {
                    eprintln!("workspace manifest check failed: {err}");
                    1
                }
            }
        }
        Ok(Command::Restore {
            backup_dir,
            root_dir,
            yes,
        }) => {
            if !yes {
                eprintln!("restore requires `--yes`");
                return 1;
            }
            match crate::recovery::restore::restore_backup(&root_dir, &backup_dir) {
                Ok(summary) => {
                    println!("Basalt restore");
                    println!();
                    println!("Backup directory: {}", backup_dir.display());
                    println!("Restored files:");
                    if summary.restored.is_empty() {
                        println!("- none");
                    } else {
                        for path in summary.restored {
                            println!("- {}", path.display());
                        }
                    }
                    println!("Removed files:");
                    if summary.removed.is_empty() {
                        println!("- none");
                    } else {
                        for path in summary.removed {
                            println!("- {}", path.display());
                        }
                    }
                    0
                }
                Err(err) => {
                    eprintln!("restore failed: {err}");
                    1
                }
            }
        }
        Ok(Command::Help) => {
            print_help();
            0
        }
        Err(err) => {
            eprintln!("error: {err}");
            eprintln!();
            print_help();
            2
        }
    }
}

#[derive(Debug)]
enum Command {
    Validate {
        config_dir: PathBuf,
    },
    Diff {
        config_dir: PathBuf,
        root_dir: PathBuf,
    },
    ApplyDryRun {
        config_dir: PathBuf,
        state_dir: PathBuf,
        rebuild: bool,
    },
    ApplyCheck {
        config_dir: PathBuf,
        root_dir: PathBuf,
    },
    Apply {
        config_dir: PathBuf,
        state_dir: PathBuf,
        root_dir: PathBuf,
        package_executor: crate::apply::PackageExecutorMode,
        service_executor: crate::apply::ServiceExecutorMode,
        rebuild: bool,
    },
    Schema,
    History {
        state_dir: PathBuf,
        limit: usize,
    },
    InspectRun {
        state_dir: PathBuf,
        run_id: Option<String>,
    },
    PackageHistory {
        state_dir: PathBuf,
        package: String,
        limit: usize,
    },
    ServiceHistory {
        state_dir: PathBuf,
        service: String,
        limit: usize,
    },
    Doctor,
    WorkspaceGenerate {
        config_dir: PathBuf,
        output_dir: PathBuf,
    },
    WorkspaceCheck {
        manifest: PathBuf,
    },
    Restore {
        backup_dir: PathBuf,
        root_dir: PathBuf,
        yes: bool,
    },
    Help,
}

fn parse_args(args: &[String]) -> Result<Command, String> {
    let Some(command) = args.get(1).map(String::as_str) else {
        return Ok(Command::Help);
    };

    match command {
        "validate" => parse_validate(args),
        "diff" => parse_diff(args),
        "apply" => parse_apply(args),
        "rebuild" => parse_rebuild(args),
        "schema" => Ok(Command::Schema),
        "history" => parse_history(args),
        "inspect-run" => parse_inspect_run(args),
        "package-history" => parse_package_history(args),
        "service-history" => parse_service_history(args),
        "doctor" => parse_doctor(args),
        "workspace" => parse_workspace(args),
        "restore" => parse_restore(args),
        "help" | "--help" | "-h" => Ok(Command::Help),
        other => Err(format!("unknown command `{other}`")),
    }
}

fn parse_workspace(args: &[String]) -> Result<Command, String> {
    let Some(subcommand) = args.get(2).map(String::as_str) else {
        return Err("workspace requires subcommand `generate`".to_string());
    };
    match subcommand {
        "generate" => parse_workspace_generate(args),
        "check" => parse_workspace_check(args),
        other => Err(format!("unknown workspace subcommand `{other}`")),
    }
}

fn parse_workspace_generate(args: &[String]) -> Result<Command, String> {
    let mut config_dir = None;
    let mut output_dir = None;
    let mut i = 3;

    while i < args.len() {
        match args[i].as_str() {
            "--config" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "`--config` requires a directory path".to_string())?;
                config_dir = Some(PathBuf::from(value));
            }
            "--output" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "`--output` requires a directory path".to_string())?;
                output_dir = Some(PathBuf::from(value));
            }
            other => return Err(format!("unexpected argument `{other}`")),
        }
        i += 1;
    }

    Ok(Command::WorkspaceGenerate {
        config_dir: config_dir
            .ok_or_else(|| "`workspace generate` requires `--config <path>`".to_string())?,
        output_dir: output_dir
            .ok_or_else(|| "`workspace generate` requires `--output <path>`".to_string())?,
    })
}

fn parse_workspace_check(args: &[String]) -> Result<Command, String> {
    let mut manifest = None;
    let mut i = 3;

    while i < args.len() {
        match args[i].as_str() {
            "--manifest" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "`--manifest` requires a file path".to_string())?;
                manifest = Some(PathBuf::from(value));
            }
            other => return Err(format!("unexpected argument `{other}`")),
        }
        i += 1;
    }

    Ok(Command::WorkspaceCheck {
        manifest: manifest
            .ok_or_else(|| "`workspace check` requires `--manifest <path>`".to_string())?,
    })
}

fn parse_validate(args: &[String]) -> Result<Command, String> {
    let mut config_dir = None;
    let mut i = 2;

    while i < args.len() {
        match args[i].as_str() {
            "--config" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "`--config` requires a directory path".to_string())?;
                config_dir = Some(PathBuf::from(value));
            }
            other => return Err(format!("unexpected argument `{other}`")),
        }
        i += 1;
    }

    let config_dir =
        config_dir.ok_or_else(|| "`validate` requires `--config <path>`".to_string())?;
    Ok(Command::Validate { config_dir })
}

fn parse_diff(args: &[String]) -> Result<Command, String> {
    let mut config_dir = None;
    let mut root_dir = None;
    let mut i = 2;

    while i < args.len() {
        match args[i].as_str() {
            "--config" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "`--config` requires a directory path".to_string())?;
                config_dir = Some(PathBuf::from(value));
            }
            "--root" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "`--root` requires a directory path".to_string())?;
                root_dir = Some(PathBuf::from(value));
            }
            other => return Err(format!("unexpected argument `{other}`")),
        }
        i += 1;
    }

    Ok(Command::Diff {
        config_dir: config_dir.ok_or_else(|| "`diff` requires `--config <path>`".to_string())?,
        root_dir: root_dir.unwrap_or_else(|| PathBuf::from("/")),
    })
}

fn parse_apply(args: &[String]) -> Result<Command, String> {
    let mut dry_run = false;
    let mut check = false;
    let mut yes = false;
    let mut config_dir = None;
    let mut state_dir = None;
    let mut root_dir = None;
    let mut package_executor = crate::apply::PackageExecutorMode::Record;
    let mut service_executor = crate::apply::ServiceExecutorMode::Record;
    let mut i = 2;

    while i < args.len() {
        match args[i].as_str() {
            "--dry-run" => dry_run = true,
            "--check" => check = true,
            "--yes" => yes = true,
            "--config" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "`--config` requires a directory path".to_string())?;
                config_dir = Some(PathBuf::from(value));
            }
            "--state-dir" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "`--state-dir` requires a directory path".to_string())?;
                state_dir = Some(PathBuf::from(value));
            }
            "--root" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "`--root` requires a directory path".to_string())?;
                root_dir = Some(PathBuf::from(value));
            }
            "--service-executor" => {
                i += 1;
                let value = args.get(i).ok_or_else(|| {
                    "`--service-executor` requires `record` or `host`".to_string()
                })?;
                service_executor = crate::apply::ServiceExecutorMode::parse(value)?;
            }
            "--package-executor" => {
                i += 1;
                let value = args.get(i).ok_or_else(|| {
                    "`--package-executor` requires `record` or `host`".to_string()
                })?;
                package_executor = crate::apply::PackageExecutorMode::parse(value)?;
            }
            other => return Err(format!("unexpected argument `{other}`")),
        }
        i += 1;
    }

    if [dry_run, check, yes]
        .iter()
        .filter(|enabled| **enabled)
        .count()
        > 1
    {
        return Err(
            "use only one of `apply --dry-run`, `apply --check`, or `apply --yes`".to_string(),
        );
    }

    if !dry_run && !check && !yes {
        return Err("apply requires `--dry-run`, `--check`, or `--yes`".to_string());
    }

    let config_dir = config_dir.ok_or_else(|| "`apply` requires `--config <path>`".to_string())?;
    let state_dir = state_dir.unwrap_or_else(|| PathBuf::from("./target/basalt-state"));
    if dry_run {
        Ok(Command::ApplyDryRun {
            config_dir,
            state_dir,
            rebuild: false,
        })
    } else if check {
        Ok(Command::ApplyCheck {
            config_dir,
            root_dir: root_dir.unwrap_or_else(|| PathBuf::from("/")),
        })
    } else {
        Ok(Command::Apply {
            config_dir,
            state_dir,
            root_dir: root_dir.unwrap_or_else(|| PathBuf::from("/")),
            package_executor,
            service_executor,
            rebuild: false,
        })
    }
}

fn parse_rebuild(args: &[String]) -> Result<Command, String> {
    parse_rebuild_with_default(args, Path::new(INSTALLED_CONFIG_DIR))
}

fn parse_rebuild_with_default(
    args: &[String],
    default_config_dir: &Path,
) -> Result<Command, String> {
    let mut dry_run = false;
    let mut config_dir = None;
    let mut state_dir = None;
    let mut root_dir = None;
    let mut package_executor = crate::apply::PackageExecutorMode::Host;
    let mut service_executor = crate::apply::ServiceExecutorMode::Host;
    let mut i = 2;

    while i < args.len() {
        match args[i].as_str() {
            "--dry-run" => dry_run = true,
            "--config" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "`--config` requires a directory path".to_string())?;
                config_dir = Some(PathBuf::from(value));
            }
            "--state-dir" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "`--state-dir` requires a directory path".to_string())?;
                state_dir = Some(PathBuf::from(value));
            }
            "--root" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "`--root` requires a directory path".to_string())?;
                root_dir = Some(PathBuf::from(value));
            }
            "--service-executor" => {
                i += 1;
                let value = args.get(i).ok_or_else(|| {
                    "`--service-executor` requires `record` or `host`".to_string()
                })?;
                service_executor = crate::apply::ServiceExecutorMode::parse(value)?;
            }
            "--package-executor" => {
                i += 1;
                let value = args.get(i).ok_or_else(|| {
                    "`--package-executor` requires `record` or `host`".to_string()
                })?;
                package_executor = crate::apply::PackageExecutorMode::parse(value)?;
            }
            other => return Err(format!("unexpected argument `{other}`")),
        }
        i += 1;
    }

    let config_dir = match config_dir {
        Some(config_dir) => config_dir,
        None => {
            if !default_config_dir.is_dir() {
                return Err(format!(
                    "`rebuild` default config directory `{}` does not exist; pass `--config <path>` to rebuild from another config",
                    default_config_dir.display()
                ));
            }
            default_config_dir.to_path_buf()
        }
    };
    let state_dir = state_dir.unwrap_or_else(|| PathBuf::from("./target/basalt-state"));

    if dry_run {
        Ok(Command::ApplyDryRun {
            config_dir,
            state_dir,
            rebuild: true,
        })
    } else {
        Ok(Command::Apply {
            config_dir,
            state_dir,
            root_dir: root_dir.unwrap_or_else(|| PathBuf::from("/")),
            package_executor,
            service_executor,
            rebuild: true,
        })
    }
}

fn parse_history(args: &[String]) -> Result<Command, String> {
    let mut state_dir = None;
    let mut limit = 20usize;
    let mut i = 2;

    while i < args.len() {
        match args[i].as_str() {
            "--state-dir" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "`--state-dir` requires a directory path".to_string())?;
                state_dir = Some(PathBuf::from(value));
            }
            "--limit" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "`--limit` requires a positive integer".to_string())?;
                limit = value
                    .parse()
                    .map_err(|_| format!("invalid history limit `{value}`"))?;
                if limit == 0 {
                    return Err("history limit must be greater than zero".to_string());
                }
            }
            other => return Err(format!("unexpected argument `{other}`")),
        }
        i += 1;
    }

    Ok(Command::History {
        state_dir: state_dir.unwrap_or_else(|| PathBuf::from("./target/basalt-state")),
        limit,
    })
}

fn validate_rebuild_safety_policy_if_needed(
    rebuild: bool,
    config: &crate::config::BasaltConfig,
) -> Result<(), Vec<String>> {
    if rebuild {
        validate_rebuild_safety_policy(config)
    } else {
        Ok(())
    }
}

fn validate_rebuild_safety_policy(config: &crate::config::BasaltConfig) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();

    if config.workspaces.is_some() {
        errors.push(
            "`rebuild` supports system, packages, services, files, and installed storage history; run `basalt workspace generate` for workspace config"
                .to_string(),
        );
    }

    if let Some(storage) = &config.storage {
        if storage.layout == "installed" {
            if !storage.partitions.is_empty() || storage.disk.is_some() {
                errors.push(
                    "`storage.layout = \"installed\"` must not include disks or partitions"
                        .to_string(),
                );
            }
        } else {
            errors.push(format!(
                "`storage.layout = \"{}\"` is install-only and cannot be used by `basalt rebuild`; use `storage.layout = \"installed\"` for installed-system history",
                storage.layout
            ));
        }

        if storage.disk.is_some() {
            errors.push("`storage.disk` describes target-disk mutation and is blocked during `basalt rebuild`".to_string());
        }

        for (index, partition) in storage.partitions.iter().enumerate() {
            let path = format!("storage.partitions[{}]", index + 1);
            errors.push(format!(
                "`{path}` describes partitioning/remounting intent and is blocked during `basalt rebuild`"
            ));
            if partition.format {
                errors.push(format!(
                    "`{path}.format = true` describes formatting intent and is blocked during `basalt rebuild`"
                ));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn parse_inspect_run(args: &[String]) -> Result<Command, String> {
    let mut state_dir = None;
    let mut run_id = None;
    let mut i = 2;

    while i < args.len() {
        match args[i].as_str() {
            "--state-dir" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "`--state-dir` requires a directory path".to_string())?;
                state_dir = Some(PathBuf::from(value));
            }
            "--run" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "`--run` requires `latest` or a run id".to_string())?;
                run_id = Some(value.to_string());
            }
            other => return Err(format!("unexpected argument `{other}`")),
        }
        i += 1;
    }

    Ok(Command::InspectRun {
        state_dir: state_dir.unwrap_or_else(|| PathBuf::from("./target/basalt-state")),
        run_id,
    })
}

fn parse_package_history(args: &[String]) -> Result<Command, String> {
    let mut state_dir = None;
    let mut package = None;
    let mut limit = 20usize;
    let mut i = 2;

    while i < args.len() {
        match args[i].as_str() {
            "--state-dir" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "`--state-dir` requires a directory path".to_string())?;
                state_dir = Some(PathBuf::from(value));
            }
            "--package" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "`--package` requires a package name".to_string())?;
                package = Some(value.to_string());
            }
            "--limit" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "`--limit` requires a positive integer".to_string())?;
                limit = value
                    .parse()
                    .map_err(|_| format!("invalid package history limit `{value}`"))?;
                if limit == 0 {
                    return Err("package history limit must be greater than zero".to_string());
                }
            }
            other => return Err(format!("unexpected argument `{other}`")),
        }
        i += 1;
    }

    Ok(Command::PackageHistory {
        state_dir: state_dir.unwrap_or_else(|| PathBuf::from("./target/basalt-state")),
        package: package
            .ok_or_else(|| "`package-history` requires `--package <name>`".to_string())?,
        limit,
    })
}

fn parse_service_history(args: &[String]) -> Result<Command, String> {
    let mut state_dir = None;
    let mut service = None;
    let mut limit = 20usize;
    let mut i = 2;

    while i < args.len() {
        match args[i].as_str() {
            "--state-dir" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "`--state-dir` requires a directory path".to_string())?;
                state_dir = Some(PathBuf::from(value));
            }
            "--service" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "`--service` requires a service name".to_string())?;
                service = Some(value.to_string());
            }
            "--limit" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "`--limit` requires a positive integer".to_string())?;
                limit = value
                    .parse()
                    .map_err(|_| format!("invalid service history limit `{value}`"))?;
                if limit == 0 {
                    return Err("service history limit must be greater than zero".to_string());
                }
            }
            other => return Err(format!("unexpected argument `{other}`")),
        }
        i += 1;
    }

    Ok(Command::ServiceHistory {
        state_dir: state_dir.unwrap_or_else(|| PathBuf::from("./target/basalt-state")),
        service: service
            .ok_or_else(|| "`service-history` requires `--service <name>`".to_string())?,
        limit,
    })
}

fn parse_doctor(args: &[String]) -> Result<Command, String> {
    if args.len() > 2 {
        return Err(format!("unexpected argument `{}`", args[2]));
    }
    Ok(Command::Doctor)
}

fn parse_restore(args: &[String]) -> Result<Command, String> {
    let mut backup_dir = None;
    let mut root_dir = None;
    let mut yes = false;
    let mut i = 2;

    while i < args.len() {
        match args[i].as_str() {
            "--backup" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "`--backup` requires a backup directory path".to_string())?;
                backup_dir = Some(PathBuf::from(value));
            }
            "--root" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "`--root` requires a directory path".to_string())?;
                root_dir = Some(PathBuf::from(value));
            }
            "--yes" => yes = true,
            other => return Err(format!("unexpected argument `{other}`")),
        }
        i += 1;
    }

    Ok(Command::Restore {
        backup_dir: backup_dir.ok_or_else(|| "`restore` requires `--backup <path>`".to_string())?,
        root_dir: root_dir.unwrap_or_else(|| PathBuf::from("/")),
        yes,
    })
}

fn print_help() {
    println!("basalt");
    println!();
    println!("Usage:");
    println!("  basalt validate --config <path>");
    println!("  basalt diff --config <path> [--root <path>]");
    println!("  basalt apply --dry-run --config <path> [--state-dir <path>]");
    println!("  basalt apply --check --config <path> [--root <path>]");
    println!("  basalt apply --yes --config <path> [--state-dir <path>] [--root <path>] [--package-executor record|host] [--service-executor record|host]");
    println!("  basalt rebuild [--dry-run] [--config <path>] [--state-dir <path>] [--root <path>]");
    println!("  basalt history [--state-dir <path>] [--limit <n>]");
    println!("  basalt inspect-run [--state-dir <path>] [--run latest|<id>]");
    println!("  basalt package-history --package <name> [--state-dir <path>] [--limit <n>]");
    println!("  basalt service-history --service <name> [--state-dir <path>] [--limit <n>]");
    println!("  basalt doctor");
    println!("  basalt workspace generate --config <path> --output <path>");
    println!("  basalt workspace check --manifest <path>");
    println!("  basalt restore --backup <path> --yes [--root <path>]");
    println!("  basalt schema");
    println!();
    println!("Rebuild policy:");
    println!(
        "  basalt rebuild defaults to /etc/basalt/install-config and treats storage as historical only."
    );
    println!("  Rebuild accepts storage.layout = \"installed\" without disk or partition intent.");
    println!(
        "  Rebuild rejects install-time storage such as whole-disk/manual layouts, storage.disk, storage.partitions, and format = true before writing state."
    );
}

fn read_apply_current_state(
    root_dir: &std::path::Path,
    config: &crate::config::BasaltConfig,
) -> Result<CurrentState, String> {
    if root_dir == std::path::Path::new("/") {
        let mut current = HostStateReader.read_current_state()?;
        current.managed_files = read_configured_managed_files(root_dir, config);
        Ok(current)
    } else {
        TargetRootStateReader::new(root_dir, config).read_current_state()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DoctorCheck {
    name: &'static str,
    required: bool,
    present: bool,
    detail: String,
}

fn doctor_report() -> Vec<DoctorCheck> {
    [
        ("cargo", true, "required for local Rust builds and tests"),
        ("rustc", true, "required for local Rust builds and tests"),
        ("git", true, "required for multi-repo development"),
        ("pacman", false, "required for host package apply on Arch"),
        ("systemctl", false, "required for host service apply"),
        ("qemu-system-x86_64", false, "required for VM smoke tests"),
        ("ssh", false, "required for VM smoke tests"),
    ]
    .into_iter()
    .map(
        |(name, required, detail)| match find_command_on_path(name) {
            Some(path) => DoctorCheck {
                name,
                required,
                present: true,
                detail: path.display().to_string(),
            },
            None => DoctorCheck {
                name,
                required,
                present: false,
                detail: detail.to_string(),
            },
        },
    )
    .collect()
}

fn render_doctor_report(checks: &[DoctorCheck]) -> String {
    let mut output = String::from("Basalt doctor\n\nRequired tools:\n");
    for check in checks.iter().filter(|check| check.required) {
        output.push_str(&render_doctor_check(check));
    }
    output.push_str("\nOptional tools:\n");
    for check in checks.iter().filter(|check| !check.required) {
        output.push_str(&render_doctor_check(check));
    }

    if checks.iter().any(|check| check.required && !check.present) {
        output.push_str("\nStatus: missing required tools\n");
    } else {
        output.push_str("\nStatus: ok\n");
    }
    output
}

fn render_doctor_check(check: &DoctorCheck) -> String {
    if check.present {
        format!("- {}: ok ({})\n", check.name, check.detail)
    } else {
        format!("- {}: missing ({})\n", check.name, check.detail)
    }
}

fn find_command_on_path(command: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|path| path.join(command))
        .find(|candidate| is_executable_file(candidate))
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    fn temp_test_dir(name: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("basalt-cli-{name}-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_minimal_rebuild_config(root: &Path, storage: &str) {
        std::fs::write(
            root.join("system.lua"),
            "return { system = { hostname = \"basalt-vm\" } }\n",
        )
        .unwrap();
        std::fs::write(
            root.join("packages.lua"),
            "return { packages = { pacman = {}, aur = {}, nix = {} } }\n",
        )
        .unwrap();
        std::fs::write(
            root.join("services.lua"),
            "return { services = { enable = {}, disable = {} } }\n",
        )
        .unwrap();
        if !storage.is_empty() {
            std::fs::write(root.join("storage.lua"), storage).unwrap();
        }
    }

    #[test]
    fn rebuild_defaults_to_installed_config_dir() {
        let default_config = temp_test_dir("default-config");
        let args = vec!["basalt".to_string(), "rebuild".to_string()];

        let command = parse_rebuild_with_default(&args, &default_config).unwrap();

        match command {
            Command::Apply {
                config_dir,
                state_dir,
                root_dir,
                package_executor,
                service_executor,
                rebuild,
            } => {
                assert_eq!(config_dir, default_config);
                assert_eq!(state_dir, PathBuf::from("./target/basalt-state"));
                assert_eq!(root_dir, PathBuf::from("/"));
                assert_eq!(package_executor, crate::apply::PackageExecutorMode::Host);
                assert_eq!(service_executor, crate::apply::ServiceExecutorMode::Host);
                assert!(rebuild);
            }
            _ => panic!("expected rebuild to parse as real apply"),
        }
    }

    #[test]
    fn rebuild_dry_run_defaults_to_installed_config_dir() {
        let default_config = temp_test_dir("dry-run-default-config");
        let args = vec![
            "basalt".to_string(),
            "rebuild".to_string(),
            "--dry-run".to_string(),
        ];

        let command = parse_rebuild_with_default(&args, &default_config).unwrap();

        match command {
            Command::ApplyDryRun {
                config_dir,
                state_dir,
                rebuild,
            } => {
                assert_eq!(config_dir, default_config);
                assert_eq!(state_dir, PathBuf::from("./target/basalt-state"));
                assert!(rebuild);
            }
            _ => panic!("expected rebuild --dry-run to parse as apply dry-run"),
        }
    }

    #[test]
    fn rebuild_explicit_config_overrides_missing_default() {
        let explicit_config = PathBuf::from("/tmp/explicit-basalt-config");
        let missing_default = std::env::temp_dir().join("basalt-cli-missing-default-config");
        let args = vec![
            "basalt".to_string(),
            "rebuild".to_string(),
            "--dry-run".to_string(),
            "--config".to_string(),
            explicit_config.display().to_string(),
        ];

        let command = parse_rebuild_with_default(&args, &missing_default).unwrap();

        match command {
            Command::ApplyDryRun {
                config_dir,
                rebuild,
                ..
            } => {
                assert_eq!(config_dir, explicit_config);
                assert!(rebuild);
            }
            _ => panic!("expected explicit rebuild --config to parse as apply dry-run"),
        }
    }

    #[test]
    fn rebuild_rejects_destructive_storage_config() {
        let config_dir = temp_test_dir("destructive-storage");
        write_minimal_rebuild_config(
            &config_dir,
            r#"return {
  storage = {
    layout = "manual",
    disk = "/dev/vda",
    target = "/mnt",
    partitions = {
      {
        disk = "/dev/vda",
        number = 1,
        mountpoint = "/",
        filesystem = "btrfs",
        format = true,
      },
    },
  },
}
"#,
        );
        let config = crate::config::validate_config_dir(&config_dir).unwrap();

        let errors = validate_rebuild_safety_policy(&config).unwrap_err();

        assert!(errors
            .iter()
            .any(|err| err.contains("install-only") && err.contains("manual")));
        assert!(errors.iter().any(|err| err.contains("format = true")));
    }

    #[test]
    fn rebuild_allows_non_storage_config() {
        let config_dir = temp_test_dir("non-storage");
        write_minimal_rebuild_config(&config_dir, "");
        let config = crate::config::validate_config_dir(&config_dir).unwrap();

        validate_rebuild_safety_policy(&config).unwrap();
    }

    #[test]
    fn rebuild_allows_installed_storage_history() {
        let config_dir = temp_test_dir("installed-storage-history");
        write_minimal_rebuild_config(
            &config_dir,
            r#"return {
  storage = {
    layout = "installed",
    root_filesystem = "btrfs",
  },
}
"#,
        );
        let config = crate::config::validate_config_dir(&config_dir).unwrap();

        validate_rebuild_safety_policy(&config).unwrap();
    }

    #[test]
    fn real_rebuild_with_explicit_config_is_guarded() {
        let explicit_config = PathBuf::from("/tmp/explicit-basalt-config");
        let missing_default = std::env::temp_dir().join("basalt-cli-missing-default-config");
        let args = vec![
            "basalt".to_string(),
            "rebuild".to_string(),
            "--config".to_string(),
            explicit_config.display().to_string(),
        ];

        let command = parse_rebuild_with_default(&args, &missing_default).unwrap();

        match command {
            Command::Apply {
                config_dir,
                rebuild,
                ..
            } => {
                assert_eq!(config_dir, explicit_config);
                assert!(rebuild);
            }
            _ => panic!("expected explicit rebuild --config to parse as guarded apply"),
        }
    }

    #[test]
    fn rebuild_missing_default_config_has_useful_error() {
        let missing_default = std::env::temp_dir().join(format!(
            "basalt-cli-missing-default-config-{}",
            std::process::id()
        ));
        let args = vec!["basalt".to_string(), "rebuild".to_string()];

        let err = parse_rebuild_with_default(&args, &missing_default).unwrap_err();

        assert!(err.contains("default config directory"));
        assert!(err.contains(&missing_default.display().to_string()));
        assert!(err.contains("--config <path>"));
    }

    #[test]
    fn renders_doctor_success_when_required_tools_exist() {
        let report = vec![
            DoctorCheck {
                name: "cargo",
                required: true,
                present: true,
                detail: "/bin/cargo".to_string(),
            },
            DoctorCheck {
                name: "pacman",
                required: false,
                present: false,
                detail: "required for host package apply on Arch".to_string(),
            },
        ];

        let rendered = render_doctor_report(&report);
        assert!(rendered.contains("- cargo: ok (/bin/cargo)"));
        assert!(rendered.contains("- pacman: missing"));
        assert!(rendered.contains("Status: ok"));
    }

    #[test]
    fn renders_doctor_failure_when_required_tool_is_missing() {
        let report = vec![DoctorCheck {
            name: "cargo",
            required: true,
            present: false,
            detail: "required for local Rust builds and tests".to_string(),
        }];

        let rendered = render_doctor_report(&report);
        assert!(rendered.contains("- cargo: missing"));
        assert!(rendered.contains("Status: missing required tools"));
    }
}
