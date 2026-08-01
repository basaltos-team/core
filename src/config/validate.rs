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

    if let Some(storage) = &config.storage {
        if !matches!(
            storage.layout.as_str(),
            "whole_disk" | "manual" | "installed"
        ) {
            errors.push(format!(
                "`storage.layout` unsupported layout `{}`",
                storage.layout
            ));
        }
        if storage.layout == "installed" {
            if !storage.partitions.is_empty() {
                errors.push(
                    "`storage.partitions` is not supported for installed storage history"
                        .to_string(),
                );
            }
            if let Some(disk) = &storage.disk {
                if !disk.trim().is_empty() {
                    errors.push(
                        "`storage.disk` is not supported for installed storage history".to_string(),
                    );
                }
            }
            if let Some(root_filesystem) = &storage.root_filesystem {
                if !is_supported_root_filesystem(root_filesystem) {
                    errors.push(format!(
                        "`storage.root_filesystem` unsupported filesystem `{root_filesystem}`"
                    ));
                }
            }
        } else if storage.target.trim().is_empty() {
            errors.push("`storage.target` cannot be empty".to_string());
        } else if !storage.target.starts_with('/') {
            errors.push("`storage.target` must be an absolute path".to_string());
        }

        if storage.layout != "installed" && storage.partitions.is_empty() {
            if storage.layout != "whole_disk" {
                errors.push(
                    "`storage.partitions` is required for manual storage layouts".to_string(),
                );
            }
            match &storage.disk {
                Some(disk) if disk.trim().is_empty() => {
                    errors.push("`storage.disk` cannot be empty".to_string());
                }
                Some(disk) if !disk.starts_with("/dev/") => {
                    errors.push("`storage.disk` must be an absolute /dev path".to_string());
                }
                Some(_) => {}
                None => errors
                    .push("`storage.disk` is required for whole-disk storage layouts".to_string()),
            }
            if let Some(efi_filesystem) = &storage.efi_filesystem {
                if efi_filesystem != "fat32" {
                    errors.push(format!(
                        "`storage.efi_filesystem` unsupported filesystem `{efi_filesystem}`"
                    ));
                }
            }
            if let Some(root_filesystem) = &storage.root_filesystem {
                if !is_supported_root_filesystem(root_filesystem) {
                    errors.push(format!(
                        "`storage.root_filesystem` unsupported filesystem `{root_filesystem}`"
                    ));
                }
            }
        } else if storage.layout != "installed" {
            validate_storage_partitions(storage, &mut errors);
        }
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

fn validate_storage_partitions(storage: &super::types::StorageConfig, errors: &mut Vec<String>) {
    let mut mountpoints = std::collections::BTreeSet::new();
    let mut disk_numbers = std::collections::BTreeSet::new();
    let mut has_root = false;

    for (index, partition) in storage.partitions.iter().enumerate() {
        let path = format!("storage.partitions[{}]", index + 1);

        if partition.disk.trim().is_empty() {
            errors.push(format!("`{path}.disk` cannot be empty"));
        } else if !partition.disk.starts_with("/dev/") {
            errors.push(format!("`{path}.disk` must be an absolute /dev path"));
        }

        if let Some(number) = &partition.number {
            if number.trim().is_empty() {
                errors.push(format!("`{path}.number` cannot be empty"));
            } else if !number.chars().all(|ch| ch.is_ascii_digit()) {
                errors.push(format!("`{path}.number` must be a positive integer"));
            } else if number == "0" {
                errors.push(format!("`{path}.number` must be greater than zero"));
            } else if !disk_numbers.insert((partition.disk.clone(), number.clone())) {
                errors.push(format!(
                    "`{path}.number` duplicates partition {} on {}",
                    number, partition.disk
                ));
            }
        }

        if !is_supported_partition_filesystem(&partition.filesystem) {
            errors.push(format!(
                "`{path}.filesystem` unsupported filesystem `{}`",
                partition.filesystem
            ));
        }

        match &partition.mountpoint {
            Some(mountpoint) if mountpoint.trim().is_empty() => {
                errors.push(format!("`{path}.mountpoint` cannot be empty"));
            }
            Some(mountpoint) if !mountpoint.starts_with('/') => {
                errors.push(format!("`{path}.mountpoint` must be an absolute path"));
            }
            Some(mountpoint) => {
                if mountpoint == "/" {
                    has_root = true;
                }
                if !mountpoints.insert(mountpoint.clone()) {
                    errors.push(format!("`{path}.mountpoint` duplicates `{mountpoint}`"));
                }
                if partition.filesystem == "swap" {
                    errors.push(format!(
                        "`{path}.mountpoint` must be omitted for swap partitions"
                    ));
                }
            }
            None => {
                if partition.filesystem != "swap" {
                    errors.push(format!(
                        "`{path}.mountpoint` is required unless filesystem is `swap`"
                    ));
                }
            }
        }

        for flag in &partition.flags {
            if !is_valid_storage_token(flag) {
                errors.push(format!(
                    "`{path}.flags` entry `{flag}` must contain only letters, numbers, `_`, or `-`"
                ));
            }
        }

        for option in &partition.mount_options {
            if option.trim().is_empty() || option.contains(char::is_whitespace) {
                errors.push(format!(
                    "`{path}.mount_options` entries cannot be empty or contain whitespace"
                ));
            }
        }

        if !partition.subvolumes.is_empty() && partition.filesystem != "btrfs" {
            errors.push(format!(
                "`{path}.subvolumes` is only supported for Btrfs partitions"
            ));
        }

        let mut subvolume_mountpoints = std::collections::BTreeSet::new();
        for (subvolume_index, subvolume) in partition.subvolumes.iter().enumerate() {
            let subvolume_path = format!("{path}.subvolumes[{}]", subvolume_index + 1);

            if subvolume.name.trim().is_empty() {
                errors.push(format!("`{subvolume_path}.name` cannot be empty"));
            } else if subvolume.name.starts_with('/') || subvolume.name.contains(':') {
                errors.push(format!(
                    "`{subvolume_path}.name` must be a relative Btrfs subvolume name"
                ));
            } else if !subvolume
                .name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '@'))
            {
                errors.push(format!(
                    "`{subvolume_path}.name` must contain only letters, numbers, `_`, `-`, `.`, or `@`"
                ));
            }

            if subvolume.mountpoint.trim().is_empty() {
                errors.push(format!("`{subvolume_path}.mountpoint` cannot be empty"));
            } else if !subvolume.mountpoint.starts_with('/') {
                errors.push(format!(
                    "`{subvolume_path}.mountpoint` must be an absolute path"
                ));
            } else {
                if subvolume.mountpoint == "/" {
                    has_root = true;
                }
                if !subvolume_mountpoints.insert(subvolume.mountpoint.clone()) {
                    errors.push(format!(
                        "`{subvolume_path}.mountpoint` duplicates `{}`",
                        subvolume.mountpoint
                    ));
                }
                if mountpoints.contains(&subvolume.mountpoint)
                    && partition.mountpoint.as_ref() != Some(&subvolume.mountpoint)
                {
                    errors.push(format!(
                        "`{subvolume_path}.mountpoint` duplicates `{}`",
                        subvolume.mountpoint
                    ));
                }
            }

            for option in &subvolume.mount_options {
                if option.trim().is_empty() || option.contains(char::is_whitespace) {
                    errors.push(format!(
                        "`{subvolume_path}.mount_options` entries cannot be empty or contain whitespace"
                    ));
                }
            }
        }
    }

    if !has_root {
        errors.push("`storage.partitions` must include a `/` mountpoint".to_string());
    }
}

fn is_supported_root_filesystem(filesystem: &str) -> bool {
    matches!(filesystem, "ext4" | "btrfs" | "xfs" | "f2fs")
}

fn is_supported_partition_filesystem(filesystem: &str) -> bool {
    matches!(
        filesystem,
        "fat32" | "ext4" | "btrfs" | "xfs" | "f2fs" | "swap"
    )
}

fn is_valid_storage_token(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
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
    use crate::config::types::{
        BasaltConfig, PackagesConfig, ServicesConfig, StorageConfig, StoragePartitionConfig,
        StorageSubvolumeConfig, SystemConfig,
    };

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
            storage: None,
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
            storage: None,
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

    #[test]
    fn rejects_unsupported_storage_filesystem() {
        let config = BasaltConfig {
            system: Some(SystemConfig {
                hostname: "basalt-test".to_string(),
                timezone: None,
                locale: None,
                keymap: None,
            }),
            packages: Some(PackagesConfig::default()),
            services: Some(ServicesConfig::default()),
            storage: Some(StorageConfig {
                layout: "whole_disk".to_string(),
                disk: Some("/dev/vda".to_string()),
                target: "/mnt".to_string(),
                efi_filesystem: Some("fat32".to_string()),
                root_filesystem: Some("zfs".to_string()),
                partitions: Vec::new(),
            }),
            files: None,
            workspaces: None,
        };

        let errors = validate(&config);

        assert!(errors
            .iter()
            .any(|error| error.contains("unsupported filesystem `zfs`")));
    }

    #[test]
    fn accepts_supported_storage_filesystems() {
        for filesystem in ["ext4", "btrfs", "xfs", "f2fs"] {
            let config = BasaltConfig {
                system: Some(SystemConfig {
                    hostname: "basalt-test".to_string(),
                    timezone: None,
                    locale: None,
                    keymap: None,
                }),
                packages: Some(PackagesConfig::default()),
                services: Some(ServicesConfig::default()),
                storage: Some(StorageConfig {
                    layout: "whole_disk".to_string(),
                    disk: Some("/dev/vda".to_string()),
                    target: "/mnt".to_string(),
                    efi_filesystem: Some("fat32".to_string()),
                    root_filesystem: Some(filesystem.to_string()),
                    partitions: Vec::new(),
                }),
                files: None,
                workspaces: None,
            };

            let errors = validate(&config);

            assert!(
                errors.is_empty(),
                "{filesystem} should be accepted; got {errors:?}"
            );
        }
    }

    #[test]
    fn accepts_manual_multi_disk_storage_partitions() {
        let config = BasaltConfig {
            system: Some(SystemConfig {
                hostname: "basalt-test".to_string(),
                timezone: None,
                locale: None,
                keymap: None,
            }),
            packages: Some(PackagesConfig::default()),
            services: Some(ServicesConfig::default()),
            storage: Some(StorageConfig {
                layout: "manual".to_string(),
                disk: None,
                target: "/mnt".to_string(),
                efi_filesystem: None,
                root_filesystem: None,
                partitions: vec![
                    StoragePartitionConfig {
                        disk: "/dev/nvme0n1".to_string(),
                        number: Some("1".to_string()),
                        label: Some("EFI".to_string()),
                        mountpoint: Some("/boot".to_string()),
                        filesystem: "fat32".to_string(),
                        size: Some("512MiB".to_string()),
                        flags: vec!["esp".to_string()],
                        format: true,
                        mount_options: Vec::new(),
                        subvolumes: Vec::new(),
                    },
                    StoragePartitionConfig {
                        disk: "/dev/nvme0n1".to_string(),
                        number: Some("2".to_string()),
                        label: Some("ROOT".to_string()),
                        mountpoint: Some("/".to_string()),
                        filesystem: "xfs".to_string(),
                        size: Some("80GiB".to_string()),
                        flags: Vec::new(),
                        format: true,
                        mount_options: Vec::new(),
                        subvolumes: Vec::new(),
                    },
                    StoragePartitionConfig {
                        disk: "/dev/sda".to_string(),
                        number: Some("1".to_string()),
                        label: Some("HOME".to_string()),
                        mountpoint: Some("/home".to_string()),
                        filesystem: "btrfs".to_string(),
                        size: Some("100%".to_string()),
                        flags: Vec::new(),
                        format: true,
                        mount_options: vec!["compress=zstd".to_string()],
                        subvolumes: vec![StorageSubvolumeConfig {
                            name: "@home".to_string(),
                            mountpoint: "/home".to_string(),
                            mount_options: vec!["compress=zstd".to_string()],
                        }],
                    },
                ],
            }),
            files: None,
            workspaces: None,
        };

        let errors = validate(&config);

        assert!(
            errors.is_empty(),
            "manual storage should be accepted: {errors:?}"
        );
    }

    #[test]
    fn rejects_manual_storage_without_root_mount() {
        let config = BasaltConfig {
            system: Some(SystemConfig {
                hostname: "basalt-test".to_string(),
                timezone: None,
                locale: None,
                keymap: None,
            }),
            packages: Some(PackagesConfig::default()),
            services: Some(ServicesConfig::default()),
            storage: Some(StorageConfig {
                layout: "manual".to_string(),
                disk: None,
                target: "/mnt".to_string(),
                efi_filesystem: None,
                root_filesystem: None,
                partitions: vec![StoragePartitionConfig {
                    disk: "/dev/vda".to_string(),
                    number: Some("1".to_string()),
                    label: Some("HOME".to_string()),
                    mountpoint: Some("/home".to_string()),
                    filesystem: "btrfs".to_string(),
                    size: Some("100%".to_string()),
                    flags: Vec::new(),
                    format: true,
                    mount_options: Vec::new(),
                    subvolumes: Vec::new(),
                }],
            }),
            files: None,
            workspaces: None,
        };

        let errors = validate(&config);

        assert!(errors
            .iter()
            .any(|error| error.contains("must include a `/` mountpoint")));
    }
}
