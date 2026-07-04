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
