// Command definitions shared with docs, shell completions, and tests.

use std::path::PathBuf;

use crate::state::store::{HostStateReader, StateReader};

pub fn run(args: Vec<String>) -> i32 {
    match parse_args(&args) {
        Ok(Command::Validate { config_dir }) => {
            match crate::config::validate_config_dir(&config_dir) {
                Ok(config) => {
                    println!(
                    "Basalt config valid: {} domain(s), {} package declaration(s), {} enabled service(s)",
                    config.domain_count(),
                    config.package_count(),
                    config.service_count()
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
        Ok(Command::Diff { config_dir }) => match crate::config::validate_config_dir(&config_dir) {
            Ok(config) => match HostStateReader.read_current_state() {
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
        }) => match crate::config::validate_config_dir(&config_dir) {
            Ok(config) => match HostStateReader.read_current_state() {
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
        }) => match crate::config::validate_config_dir(&config_dir) {
            Ok(config) => match HostStateReader.read_current_state() {
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

enum Command {
    Validate {
        config_dir: PathBuf,
    },
    Diff {
        config_dir: PathBuf,
    },
    ApplyDryRun {
        config_dir: PathBuf,
        state_dir: PathBuf,
    },
    Apply {
        config_dir: PathBuf,
        state_dir: PathBuf,
        root_dir: PathBuf,
        package_executor: crate::apply::PackageExecutorMode,
        service_executor: crate::apply::ServiceExecutorMode,
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
        "schema" => Ok(Command::Schema),
        "history" => parse_history(args),
        "inspect-run" => parse_inspect_run(args),
        "package-history" => parse_package_history(args),
        "service-history" => parse_service_history(args),
        "restore" => parse_restore(args),
        "help" | "--help" | "-h" => Ok(Command::Help),
        other => Err(format!("unknown command `{other}`")),
    }
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
    let config_dir = parse_config_dir_arg(args, "diff")?;
    Ok(Command::Diff { config_dir })
}

fn parse_apply(args: &[String]) -> Result<Command, String> {
    let mut dry_run = false;
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

    if dry_run && yes {
        return Err("use either `apply --dry-run` or `apply --yes`, not both".to_string());
    }

    if !dry_run && !yes {
        return Err("real apply requires `--yes`; use `--dry-run` to preview".to_string());
    }

    let config_dir = config_dir.ok_or_else(|| "`apply` requires `--config <path>`".to_string())?;
    let state_dir = state_dir.unwrap_or_else(|| PathBuf::from("./target/basalt-state"));
    if dry_run {
        Ok(Command::ApplyDryRun {
            config_dir,
            state_dir,
        })
    } else {
        Ok(Command::Apply {
            config_dir,
            state_dir,
            root_dir: root_dir.unwrap_or_else(|| PathBuf::from("/")),
            package_executor,
            service_executor,
        })
    }
}

fn parse_config_dir_arg(args: &[String], command: &str) -> Result<PathBuf, String> {
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

    config_dir.ok_or_else(|| format!("`{command}` requires `--config <path>`"))
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
    println!("  basalt diff --config <path>");
    println!("  basalt apply --dry-run --config <path> [--state-dir <path>]");
    println!("  basalt apply --yes --config <path> [--state-dir <path>] [--root <path>] [--package-executor record|host] [--service-executor record|host]");
    println!("  basalt history [--state-dir <path>] [--limit <n>]");
    println!("  basalt inspect-run [--state-dir <path>] [--run latest|<id>]");
    println!("  basalt package-history --package <name> [--state-dir <path>] [--limit <n>]");
    println!("  basalt service-history --service <name> [--state-dir <path>] [--limit <n>]");
    println!("  basalt restore --backup <path> --yes [--root <path>]");
    println!("  basalt schema");
}
