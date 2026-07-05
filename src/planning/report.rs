// Human and machine-readable plan reports.

use crate::config::BasaltConfig;
use crate::state::store::CurrentState;

use super::action::Action;

pub fn render_diff(config: &BasaltConfig, current: &CurrentState) -> String {
    let mut out = String::new();
    out.push_str("Basalt diff plan\n\n");
    render_system(config, current, &mut out);
    out.push('\n');
    render_packages(config, current, &mut out);
    out.push('\n');
    render_services(config, current, &mut out);
    out.push('\n');
    render_files(config, current, &mut out);
    out
}

pub fn render_dry_run(actions: &[Action]) -> String {
    let mut out = String::new();
    out.push_str("Basalt apply dry-run\n\n");
    out.push_str("No changes will be made.\n\n");
    out.push_str("actions:\n");

    if actions.is_empty() {
        out.push_str("  none\n");
        return out;
    }

    for action in actions {
        out.push_str("  - id: ");
        out.push_str(&action.id);
        out.push('\n');
        out.push_str("    domain: ");
        out.push_str(&action.domain);
        out.push('\n');
        out.push_str("    risk: ");
        out.push_str(action.risk.as_str());
        out.push('\n');
        out.push_str("    plan: ");
        out.push_str(&action.description);
        out.push('\n');
    }

    out
}

pub fn render_check(actions: &[Action]) -> String {
    let mut out = String::new();
    out.push_str("Basalt apply check\n\n");

    if actions.is_empty() {
        out.push_str("No changes needed.\n");
        return out;
    }

    out.push_str("Pending action(s):\n");
    for action in actions {
        out.push_str("  - id: ");
        out.push_str(&action.id);
        out.push('\n');
        out.push_str("    domain: ");
        out.push_str(&action.domain);
        out.push('\n');
        out.push_str("    risk: ");
        out.push_str(action.risk.as_str());
        out.push('\n');
        out.push_str("    plan: ");
        out.push_str(&action.description);
        out.push('\n');
    }

    out
}

fn render_system(config: &BasaltConfig, current: &CurrentState, out: &mut String) {
    out.push_str("system:\n");
    if let Some(system) = &config.system {
        if current.hostname.as_deref() == Some(system.hostname.as_str()) {
            push_status(out, "=", "hostname", &system.hostname);
        } else {
            let current = current.hostname.as_deref().unwrap_or("unknown");
            push_status(
                out,
                "+",
                "hostname",
                &format!("{} (current: {current})", system.hostname),
            );
        }
        push_optional_system_status(
            out,
            "timezone",
            system.timezone.as_deref(),
            &current.timezone,
        );
        push_optional_system_status(out, "locale", system.locale.as_deref(), &current.locale);
        push_optional_system_status(out, "keymap", system.keymap.as_deref(), &current.keymap);
    } else {
        out.push_str("  none\n");
    }
}

fn render_packages(config: &BasaltConfig, current: &CurrentState, out: &mut String) {
    out.push_str("packages:\n");
    if let Some(packages) = &config.packages {
        push_package_list(out, "pacman", &packages.pacman, &current.pacman_packages);
        push_list(out, "aur", &packages.aur);
        push_list(out, "nix", &packages.nix);
    } else {
        out.push_str("  none\n");
    }
}

fn render_services(config: &BasaltConfig, current: &CurrentState, out: &mut String) {
    out.push_str("services:\n");
    if let Some(services) = &config.services {
        push_service_list(out, "enable", &services.enable, &current.enabled_services);
        push_service_disable_list(out, "disable", &services.disable, &current.enabled_services);
    } else {
        out.push_str("  none\n");
    }
}

fn render_files(config: &BasaltConfig, current: &CurrentState, out: &mut String) {
    out.push_str("files:\n");
    if let Some(files) = &config.files {
        if files.managed.is_empty() {
            out.push_str("  managed:\n    none\n");
            return;
        }
        out.push_str("  managed:\n");
        for file in &files.managed {
            let marker = if current.managed_files.get(&file.path) == Some(&file.content) {
                "="
            } else {
                "+"
            };
            out.push_str("    ");
            out.push_str(marker);
            out.push(' ');
            out.push_str(&file.path);
            if let Some(mode) = &file.mode {
                out.push_str(" mode=");
                out.push_str(mode);
            }
            out.push('\n');
        }
    } else {
        out.push_str("  none\n");
    }
}

fn push_status(out: &mut String, marker: &str, key: &str, value: &str) {
    out.push_str("  ");
    out.push_str(marker);
    out.push(' ');
    out.push_str(key);
    out.push_str(": ");
    out.push_str(value);
    out.push('\n');
}

fn push_optional_system_status(
    out: &mut String,
    key: &str,
    desired: Option<&str>,
    current: &Option<String>,
) {
    let Some(desired) = desired else {
        push_status(out, "=", key, "unset");
        return;
    };
    let marker = if current.as_deref() == Some(desired) {
        "="
    } else {
        "+"
    };
    push_status(out, marker, key, desired);
}

fn push_list(out: &mut String, key: &str, values: &[String]) {
    out.push_str("  ");
    out.push_str(key);
    out.push_str(":\n");
    if values.is_empty() {
        out.push_str("    none\n");
        return;
    }

    for value in values {
        out.push_str("    + ");
        out.push_str(value);
        out.push('\n');
    }
}

fn push_package_list(
    out: &mut String,
    key: &str,
    values: &[String],
    current: &std::collections::BTreeSet<String>,
) {
    out.push_str("  ");
    out.push_str(key);
    out.push_str(":\n");
    if values.is_empty() {
        out.push_str("    none\n");
        return;
    }

    for value in values {
        let marker = if current.contains(value) { "=" } else { "+" };
        out.push_str("    ");
        out.push_str(marker);
        out.push(' ');
        out.push_str(value);
        out.push('\n');
    }
}

fn push_service_list(
    out: &mut String,
    key: &str,
    values: &[String],
    current: &std::collections::BTreeSet<String>,
) {
    out.push_str("  ");
    out.push_str(key);
    out.push_str(":\n");
    if values.is_empty() {
        out.push_str("    none\n");
        return;
    }

    for value in values {
        let service_unit = format!("{value}.service");
        let marker = if current.contains(value) || current.contains(&service_unit) {
            "="
        } else {
            "+"
        };
        out.push_str("    ");
        out.push_str(marker);
        out.push(' ');
        out.push_str(value);
        out.push('\n');
    }
}

fn push_service_disable_list(
    out: &mut String,
    key: &str,
    values: &[String],
    current: &std::collections::BTreeSet<String>,
) {
    out.push_str("  ");
    out.push_str(key);
    out.push_str(":\n");
    if values.is_empty() {
        out.push_str("    none\n");
        return;
    }

    for value in values {
        let service_unit = format!("{value}.service");
        let marker = if current.contains(value) || current.contains(&service_unit) {
            "+"
        } else {
            "="
        };
        out.push_str("    ");
        out.push_str(marker);
        out.push(' ');
        out.push_str(value);
        out.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::validate_config_dir;
    use std::path::Path;

    #[test]
    fn renders_minimal_diff_plan() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("configs/examples/minimal");
        let config = validate_config_dir(&root).unwrap();
        let rendered = render_diff(&config, &CurrentState::default());

        assert!(rendered.contains("Basalt diff plan"));
        assert!(rendered.contains("hostname: basalt-vm"));
        assert!(rendered.contains("+ base-devel"));
        assert!(rendered.contains("+ NetworkManager"));
    }

    #[test]
    fn renders_apply_check_status() {
        let action = Action {
            id: "system.hostname".to_string(),
            domain: "system".to_string(),
            description: "set hostname to `basalt-vm`".to_string(),
            risk: crate::planning::action::Risk::Medium,
        };

        let pending = render_check(&[action]);
        assert!(pending.contains("Basalt apply check"));
        assert!(pending.contains("Pending action(s)"));
        assert!(pending.contains("system.hostname"));

        let settled = render_check(&[]);
        assert!(settled.contains("No changes needed."));
    }

    #[test]
    fn renders_matching_current_state_markers() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("configs/examples/minimal");
        let config = validate_config_dir(&root).unwrap();
        let mut current = CurrentState {
            hostname: Some("basalt-vm".to_string()),
            ..CurrentState::default()
        };
        current.pacman_packages.insert("git".to_string());
        current
            .enabled_services
            .insert("NetworkManager.service".to_string());

        let rendered = render_diff(&config, &current);
        assert!(rendered.contains("= hostname: basalt-vm"));
        assert!(rendered.contains("= git"));
        assert!(rendered.contains("= NetworkManager"));
    }

    #[test]
    fn renders_matching_system_and_file_markers() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("configs/fixtures/valid-managed-files");
        let config = validate_config_dir(&root).unwrap();
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

        let rendered = render_diff(&config, &current);
        assert!(rendered.contains("= timezone: UTC"));
        assert!(rendered.contains("= locale: en_US.UTF-8"));
        assert!(rendered.contains("= keymap: us"));
        assert!(rendered.contains("= /etc/basalt/motd"));
        assert!(rendered.contains("= old-example"));
    }

    #[test]
    fn renders_dry_run_action_plan() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("configs/examples/minimal");
        let config = validate_config_dir(&root).unwrap();
        let actions = crate::planning::action::plan_actions(&config, &CurrentState::default());
        let rendered = render_dry_run(&actions);

        assert!(rendered.contains("Basalt apply dry-run"));
        assert!(rendered.contains("No changes will be made."));
        assert!(rendered.contains("id: system.hostname"));
        assert!(rendered.contains("ensure pacman package `git` is installed"));
    }
}
