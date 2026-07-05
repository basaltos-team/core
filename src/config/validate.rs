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
        };

        let errors = validate(&config);

        assert!(errors
            .iter()
            .any(|error| error.contains("unsupported version constraint syntax")));
    }
}
