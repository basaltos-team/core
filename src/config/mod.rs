// Lua evaluation, config loading, and merge orchestration.

pub mod migrate;
pub mod sandbox;
pub mod schema;
pub mod types;
pub mod validate;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use mlua::Value;

pub use types::{BasaltConfig, DomainValue};

pub fn validate_config_dir(path: &Path) -> Result<BasaltConfig, Vec<String>> {
    let mut errors = Vec::new();
    let mut config = BasaltConfig::default();

    if !path.exists() {
        return Err(vec![format!(
            "{}: config directory does not exist",
            path.display()
        )]);
    }

    if !path.is_dir() {
        return Err(vec![format!(
            "{}: config path is not a directory",
            path.display()
        )]);
    }

    let files = match config_entry_files(path) {
        Ok(files) => files,
        Err(err) => return Err(vec![err]),
    };

    if files.is_empty() {
        return Err(vec![format!(
            "{}: no .lua config files found",
            path.display()
        )]);
    }

    for file in files {
        match parse_config_file(&file) {
            Ok(domains) => {
                for (domain, value) in domains {
                    if config.has_domain(&domain) {
                        errors.push(format!(
                            "{}: duplicate top-level domain `{domain}`",
                            file.display()
                        ));
                        continue;
                    }

                    if let Err(err) = config.insert_domain(domain, value, &file) {
                        errors.push(err);
                    }
                }
            }
            Err(err) => errors.push(err),
        }
    }

    if errors.is_empty() {
        errors.extend(validate::validate(&config));
    }

    if errors.is_empty() {
        Ok(config)
    } else {
        Err(errors)
    }
}

fn config_entry_files(path: &Path) -> Result<Vec<PathBuf>, String> {
    let init = path.join("init.lua");
    if init.is_file() {
        return Ok(vec![init]);
    }

    let mut files = Vec::new();

    for entry in fs::read_dir(path).map_err(|err| format!("{}: {err}", path.display()))? {
        let entry = entry.map_err(|err| format!("{}: {err}", path.display()))?;
        let file_path = entry.path();
        if file_path.extension().is_some_and(|ext| ext == "lua") {
            files.push(file_path);
        }
    }

    files.sort();
    Ok(files)
}

fn parse_config_file(path: &Path) -> Result<BTreeMap<String, DomainValue>, String> {
    let input = fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))?;
    parse_config_source(path, &input)
}

fn parse_config_source(path: &Path, input: &str) -> Result<BTreeMap<String, DomainValue>, String> {
    let lua = if path.file_name().is_some_and(|name| name == "init.lua") {
        let module_root = path.parent().unwrap_or_else(|| Path::new("."));
        sandbox::new_sandboxed_lua_with_modules(path, module_root)?
    } else {
        sandbox::new_sandboxed_lua(path)?
    };
    let value = lua
        .load(input)
        .set_name(path.display().to_string())
        .eval::<Value>()
        .map_err(|err| {
            let message = err.to_string();
            let first_line = message.lines().next().unwrap_or("unknown Lua error");
            format!("{}: Lua evaluation failed: {first_line}", path.display())
        })?;

    let mut domains = BTreeMap::new();
    match lua_value_to_domain_value(path, "<root>", value)? {
        DomainValue::Table(entries) => {
            for (domain, value) in entries {
                domains.insert(domain, value);
            }
        }
        DomainValue::Boolean(_)
        | DomainValue::Integer(_)
        | DomainValue::String(_)
        | DomainValue::List(_) => {
            return Err(format!(
                "{}: config file must return a table",
                path.display()
            ));
        }
    }

    Ok(domains)
}

fn lua_value_to_domain_value(
    path: &Path,
    lua_path: &str,
    value: Value,
) -> Result<DomainValue, String> {
    match value {
        Value::String(value) => value
            .to_str()
            .map(|value| DomainValue::String(value.to_string()))
            .map_err(|err| {
                format!(
                    "{}: `{lua_path}` contains invalid UTF-8: {err}",
                    path.display()
                )
            }),
        Value::Boolean(value) => Ok(DomainValue::Boolean(value)),
        Value::Integer(value) => Ok(DomainValue::Integer(value)),
        Value::Table(table) => lua_table_to_domain_value(path, lua_path, table),
        Value::Nil => Err(format!("{}: `{lua_path}` is nil", path.display())),
        _ => Err(format!(
            "{}: `{lua_path}` must be a boolean, integer, string, list, or table",
            path.display()
        )),
    }
}

fn lua_table_to_domain_value(
    path: &Path,
    lua_path: &str,
    table: mlua::Table,
) -> Result<DomainValue, String> {
    let mut keyed = Vec::new();
    let mut list = Vec::new();

    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair.map_err(|err| {
            format!(
                "{}: failed reading table `{lua_path}`: {err}",
                path.display()
            )
        })?;

        match key {
            Value::String(key) => {
                let key = key.to_str().map_err(|err| {
                    format!(
                        "{}: table `{lua_path}` has invalid UTF-8 key: {err}",
                        path.display()
                    )
                })?;
                let key = key.to_string();
                let next_path = if lua_path == "<root>" {
                    key.clone()
                } else {
                    format!("{lua_path}.{key}")
                };
                keyed.push((key, lua_value_to_domain_value(path, &next_path, value)?));
            }
            Value::Integer(index) => {
                let next_path = format!("{lua_path}[{index}]");
                list.push((index, lua_value_to_domain_value(path, &next_path, value)?));
            }
            _ => {
                return Err(format!(
                    "{}: table `{lua_path}` contains unsupported key type",
                    path.display()
                ));
            }
        }
    }

    if !keyed.is_empty() && !list.is_empty() {
        return Err(format!(
            "{}: table `{lua_path}` cannot mix keyed fields and list values",
            path.display()
        ));
    }

    if keyed.is_empty() {
        list.sort_by_key(|(index, _)| *index);
        Ok(DomainValue::List(
            list.into_iter().map(|(_, value)| value).collect(),
        ))
    } else {
        keyed.sort_by(|(left, _), (right, _)| left.cmp(right));
        Ok(DomainValue::Table(keyed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_system_domain() {
        let input = r#"
            return {
              system = {
                hostname = "basalt-vm",
              },
            }
        "#;
        let path = Path::new("system.lua");
        let lua = sandbox::new_sandboxed_lua(path).unwrap();
        let value = lua.load(input).eval::<Value>().unwrap();
        let parsed = lua_value_to_domain_value(path, "<root>", value).unwrap();

        match parsed {
            DomainValue::Table(entries) => {
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].0, "system");
            }
            _ => panic!("expected table"),
        }
    }

    #[test]
    fn rejects_non_returning_file() {
        let input = r#"local hostname = "basalt-vm""#;
        assert!(parse_config_source(Path::new("system.lua"), input).is_err());
    }

    #[test]
    fn loads_storage_partition_numbers() {
        let root = std::env::temp_dir().join(format!(
            "basalt-storage-partition-number-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("system.lua"),
            r#"return { system = { hostname = "basalt-test" } }"#,
        )
        .unwrap();
        fs::write(
            root.join("packages.lua"),
            r#"return { packages = { pacman = {}, aur = {}, nix = {} } }"#,
        )
        .unwrap();
        fs::write(
            root.join("services.lua"),
            r#"return { services = { enable = {}, disable = {} } }"#,
        )
        .unwrap();
        fs::write(
            root.join("storage.lua"),
            r#"
            return {
              storage = {
                layout = "manual",
                target = "/mnt",
                partitions = {
                  {
                    disk = "/dev/vda",
                    number = 1,
                    mountpoint = "/",
                    filesystem = "xfs",
                  },
                },
              },
            }
            "#,
        )
        .unwrap();

        let config = validate_config_dir(&root).unwrap();

        assert_eq!(
            config.storage.unwrap().partitions[0].number.as_deref(),
            Some("1")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn validates_shared_valid_fixture() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("configs/fixtures/valid-basic");

        let config = validate_config_dir(&root).unwrap();
        assert_eq!(config.domain_count(), 3);
        assert_eq!(config.package_count(), 2);
        assert_eq!(config.service_count(), 1);
    }

    #[test]
    fn validates_composed_init_fixture() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("configs/fixtures/valid-composed");

        let config = validate_config_dir(&root).unwrap();
        assert_eq!(
            config
                .system
                .as_ref()
                .map(|system| system.hostname.as_str()),
            Some("basalt-composed")
        );
        assert_eq!(config.package_count(), 5);
        assert_eq!(config.service_count(), 2);
    }

    #[test]
    fn validates_devenv_workspace_fixture() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("configs/fixtures/valid-devenv-workspace");

        let config = validate_config_dir(&root).unwrap();
        assert_eq!(config.workspace_count(), 1);
        let workspaces = config.workspaces.as_ref().unwrap();
        let workspace = workspaces.entries.get("basalt_core").unwrap();
        assert_eq!(workspace.backend, "devenv");
        assert_eq!(workspace.languages.get("rust"), Some(&true));
        assert_eq!(workspace.services.get("postgres"), Some(&true));
        assert_eq!(
            workspace.tasks.get("test").map(String::as_str),
            Some("cargo test")
        );
    }

    #[test]
    fn rejects_shared_unknown_field_fixture() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("configs/fixtures/invalid-unknown-field");

        let errors = validate_config_dir(&root).unwrap_err();
        assert!(errors
            .iter()
            .any(|err| err.contains("unknown field `system.planet`")));
    }

    #[test]
    fn rejects_shared_duplicate_domain_fixture() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("configs/fixtures/invalid-duplicate-domain");

        let errors = validate_config_dir(&root).unwrap_err();
        assert!(errors
            .iter()
            .any(|err| err.contains("duplicate top-level domain `system`")));
    }

    #[test]
    fn rejects_shared_lua_sandbox_fixture() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("configs/fixtures/invalid-lua-sandbox");

        let errors = validate_config_dir(&root).unwrap_err();
        assert!(errors.iter().any(|err| {
            err.contains("Lua evaluation failed")
                && (err.contains("os") || err.contains("nil value"))
        }));
    }

    #[test]
    fn rejects_module_escape_from_init() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("configs/fixtures/invalid-module-escape");

        let errors = validate_config_dir(&root).unwrap_err();
        assert!(errors
            .iter()
            .any(|err| err.contains("unsupported module name")));
    }

    #[test]
    fn rejects_shared_package_version_constraint_fixture() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("configs/fixtures/invalid-package-version-constraint");

        let errors = validate_config_dir(&root).unwrap_err();
        assert!(errors
            .iter()
            .any(|err| err.contains("unsupported version constraint syntax")));
    }
}
