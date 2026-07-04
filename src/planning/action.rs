// Typed dry-run/apply actions.

use crate::config::BasaltConfig;
use crate::state::store::CurrentState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub id: String,
    pub domain: String,
    pub description: String,
    pub risk: Risk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    Low,
    Medium,
    High,
}

impl Risk {
    pub fn as_str(self) -> &'static str {
        match self {
            Risk::Low => "low",
            Risk::Medium => "medium",
            Risk::High => "high",
        }
    }
}

pub fn plan_actions(config: &BasaltConfig, current: &CurrentState) -> Vec<Action> {
    let mut actions = Vec::new();

    if let Some(system) = &config.system {
        if current.hostname.as_deref() != Some(system.hostname.as_str()) {
            actions.push(Action {
                id: "system.hostname".to_string(),
                domain: "system".to_string(),
                description: format!("set hostname to `{}`", system.hostname),
                risk: Risk::Medium,
            });
        }

        if let Some(timezone) = &system.timezone {
            actions.push(Action {
                id: "system.timezone".to_string(),
                domain: "system".to_string(),
                description: format!("set timezone to `{timezone}`"),
                risk: Risk::Low,
            });
        }

        if let Some(locale) = &system.locale {
            actions.push(Action {
                id: "system.locale".to_string(),
                domain: "system".to_string(),
                description: format!("set locale to `{locale}`"),
                risk: Risk::Low,
            });
        }

        if let Some(keymap) = &system.keymap {
            actions.push(Action {
                id: "system.keymap".to_string(),
                domain: "system".to_string(),
                description: format!("set keymap to `{keymap}`"),
                risk: Risk::Low,
            });
        }
    }

    if let Some(packages) = &config.packages {
        for package in &packages.pacman {
            if !current.pacman_packages.contains(package) {
                actions.push(Action {
                    id: format!("packages.pacman.{package}"),
                    domain: "packages".to_string(),
                    description: format!("ensure pacman package `{package}` is installed"),
                    risk: Risk::High,
                });
            }
        }

        for package in &packages.aur {
            actions.push(Action {
                id: format!("packages.aur.{package}"),
                domain: "packages".to_string(),
                description: format!("ensure AUR package `{package}` is installed"),
                risk: Risk::Medium,
            });
        }

        for package in &packages.nix {
            actions.push(Action {
                id: format!("packages.nix.{package}"),
                domain: "packages".to_string(),
                description: format!("ensure Nix package `{package}` is installed"),
                risk: Risk::Low,
            });
        }
    }

    if let Some(services) = &config.services {
        for service in &services.enable {
            if !current.enabled_services.contains(service) {
                actions.push(Action {
                    id: format!("services.enable.{service}"),
                    domain: "services".to_string(),
                    description: format!("enable service `{service}`"),
                    risk: Risk::Medium,
                });
            }
        }

        for service in &services.disable {
            actions.push(Action {
                id: format!("services.disable.{service}"),
                domain: "services".to_string(),
                description: format!("disable service `{service}`"),
                risk: Risk::High,
            });
        }
    }

    if let Some(files) = &config.files {
        for file in &files.managed {
            actions.push(Action {
                id: format!("files.managed.{}", file.path.trim_start_matches('/')),
                domain: "files".to_string(),
                description: format!("write managed file `{}`", file.path),
                risk: Risk::Medium,
            });
        }
    }

    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::validate_config_dir;
    use crate::state::store::CurrentState;
    use std::path::Path;

    #[test]
    fn plans_actions_for_minimal_config() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("basalt-configs/examples/minimal");
        let config = validate_config_dir(&root).unwrap();
        let actions = plan_actions(&config, &CurrentState::default());

        assert!(actions.iter().any(|action| action.id == "system.hostname"));
        assert!(actions
            .iter()
            .any(|action| action.id == "packages.pacman.base-devel"));
        assert!(actions
            .iter()
            .any(|action| action.id == "services.enable.NetworkManager"));
    }

    #[test]
    fn skips_actions_that_already_match_current_state() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("basalt-configs/examples/minimal");
        let config = validate_config_dir(&root).unwrap();
        let mut current = CurrentState {
            hostname: Some("basalt-vm".to_string()),
            ..CurrentState::default()
        };
        current.pacman_packages.insert("git".to_string());
        current
            .enabled_services
            .insert("NetworkManager".to_string());

        let actions = plan_actions(&config, &current);
        assert!(!actions.iter().any(|action| action.id == "system.hostname"));
        assert!(!actions
            .iter()
            .any(|action| action.id == "packages.pacman.git"));
        assert!(!actions
            .iter()
            .any(|action| action.id == "services.enable.NetworkManager"));
        assert!(actions
            .iter()
            .any(|action| action.id == "packages.pacman.base-devel"));
    }
}
