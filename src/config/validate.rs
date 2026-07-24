// Cross-domain and field-level validation.

use super::types::BasaltConfig;

pub fn validate(config: &BasaltConfig) -> Vec<String> {
    let mut errors = Vec::new();

    let Some(system) = &config.system else {
        errors.push("missing required top-level domain `system`".to_string());
        return errors;
    };

    if system.hostname.trim().is_empty() {
        errors.push("`system.hostname` cannot be empty".to_string());
    }

    if config.packages.is_none() {
        errors.push("missing required top-level domain `packages`".to_string());
    }
    if let Some(packages) = &config.packages {
        validate_package_names("packages.pacman", &packages.pacman, &mut errors);
        validate_package_names("packages.aur", &packages.aur, &mut errors);
        validate_package_names("packages.nix", &packages.nix, &mut errors);
    }

    if config.services.is_none() {
        errors.push("missing required top-level domain `services`".to_string());
    }

    if let Some(files) = &config.files {
        for managed in &files.managed {
            if managed.path.trim().is_empty() {
                errors.push("`files.managed[].path` cannot be empty".to_string());
            }
            if managed.content.contains('\0') {
                errors.push(format!(
                    "`files.managed` content for `{}` cannot contain NUL bytes",
                    managed.path
                ));
            }
            if let Some(mode) = &managed.mode {
                let valid_mode = mode.len() == 4
                    && mode.starts_with('0')
                    && mode.chars().all(|ch| matches!(ch, '0'..='7'));
                if !valid_mode {
                    errors.push(format!(
                        "`files.managed` mode for `{}` must be an octal string like `0644`",
                        managed.path
                    ));
                }
            }
        }
    }

    if let Some(workspaces) = &config.workspaces {
        for (name, workspace) in &workspaces.entries {
            if !is_valid_identifier(name) {
                errors.push(format!(
                    "`workspaces` name `{name}` must contain only letters, numbers, `_`, or `-`"
                ));
            }
            if workspace.path.trim().is_empty() {
                errors.push(format!("`workspaces.{name}.path` cannot be empty"));
            }
            if workspace.backend != "devenv" {
                errors.push(format!(
                    "`workspaces.{name}.backend` unsupported backend `{}`",
                    workspace.backend
                ));
            }
            validate_workspace_keys(
                &format!("workspaces.{name}.languages"),
                &workspace.languages,
                &mut errors,
            );
            validate_workspace_keys(
                &format!("workspaces.{name}.services"),
                &workspace.services,
                &mut errors,
            );
            validate_workspace_package_attrs(
                &format!("workspaces.{name}.packages"),
                &workspace.packages,
                &mut errors,
            );
            for task in workspace.tasks.keys() {
                if task.trim().is_empty() {
                    errors.push(format!(
                        "`workspaces.{name}.tasks` task names cannot be empty"
                    ));
                }
            }
        }
    }

    errors
}

fn validate_package_names(path: &str, packages: &[String], errors: &mut Vec<String>) {
    for package in packages {
        let package = package.trim();
        if package.is_empty() {
            errors.push(format!("`{path}` package names cannot be empty"));
            continue;
        }
        if package.contains(char::is_whitespace) {
            errors.push(format!(
                "`{path}` package `{package}` cannot contain whitespace"
            ));
        }
        if package.contains(['=', '<', '>']) {
            errors.push(format!(
                "`{path}` package `{package}` uses unsupported version constraint syntax"
            ));
        }
    }
}

fn validate_workspace_keys(
    path: &str,
    values: &std::collections::BTreeMap<String, bool>,
    errors: &mut Vec<String>,
) {
    for key in values.keys() {
        if !is_valid_identifier(key) {
            errors.push(format!(
                "`{path}` key `{key}` must contain only letters, numbers, `_`, or `-`"
            ));
        }
    }
}

fn is_valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
}

fn validate_workspace_package_attrs(path: &str, packages: &[String], errors: &mut Vec<String>) {
    for package in packages {
        if package.split('.').any(|part| !is_valid_identifier(part)) {
            errors.push(format!(
                "`{path}` package `{package}` must be a Nix attr path using letters, numbers, `_`, `-`, or `.`"
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::{BasaltConfig, PackagesConfig, ServicesConfig, SystemConfig};

    #[test]
    fn rejects_unsupported_package_version_constraints() {
        let config = BasaltConfig {
            system: Some(SystemConfig {
                hostname: "basalt-test".to_string(),
                timezone: None,
                locale: None,
                keymap: None,
            }),
            packages: Some(PackagesConfig {
                pacman: vec!["tree=2.3.2-1".to_string()],
                aur: Vec::new(),
                nix: Vec::new(),
            }),
            services: Some(ServicesConfig::default()),
            files: None,
            workspaces: None,
        };

        let errors = validate(&config);

        assert!(errors
            .iter()
            .any(|error| error.contains("unsupported version constraint syntax")));
    }

    #[test]
    fn rejects_unsupported_workspace_backend() {
        let config = BasaltConfig {
            system: Some(SystemConfig {
                hostname: "basalt-test".to_string(),
                timezone: None,
                locale: None,
                keymap: None,
            }),
            packages: Some(PackagesConfig::default()),
            services: Some(ServicesConfig::default()),
            files: None,
            workspaces: Some(crate::config::types::WorkspacesConfig {
                entries: std::collections::BTreeMap::from([(
                    "core".to_string(),
                    crate::config::types::WorkspaceConfig {
                        path: "~/Projects/basaltos/core".to_string(),
                        backend: "profile".to_string(),
                        ..Default::default()
                    },
                )]),
            }),
        };

        let errors = validate(&config);

        assert!(errors
            .iter()
            .any(|error| error.contains("unsupported backend `profile`")));
    }
}
