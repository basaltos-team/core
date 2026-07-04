// Typed Basalt configuration structs.

use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum DomainValue {
    String(String),
    List(Vec<DomainValue>),
    Table(Vec<(String, DomainValue)>),
}

impl DomainValue {
    pub fn into_table(
        self,
        file: &Path,
        domain: &str,
    ) -> Result<BTreeMap<String, DomainValue>, String> {
        match self {
            DomainValue::Table(entries) => Ok(entries.into_iter().collect()),
            DomainValue::String(_) | DomainValue::List(_) => Err(format!(
                "{}: domain `{domain}` must be a table",
                file.display()
            )),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct BasaltConfig {
    pub system: Option<SystemConfig>,
    pub packages: Option<PackagesConfig>,
    pub services: Option<ServicesConfig>,
    pub files: Option<FilesConfig>,
}

impl BasaltConfig {
    pub fn has_domain(&self, domain: &str) -> bool {
        match domain {
            "system" => self.system.is_some(),
            "packages" => self.packages.is_some(),
            "services" => self.services.is_some(),
            "files" => self.files.is_some(),
            _ => false,
        }
    }

    pub fn insert_domain(
        &mut self,
        domain: String,
        value: DomainValue,
        file: &Path,
    ) -> Result<(), String> {
        match domain.as_str() {
            "system" => {
                self.system = Some(SystemConfig::from_value(value, file)?);
                Ok(())
            }
            "packages" => {
                self.packages = Some(PackagesConfig::from_value(value, file)?);
                Ok(())
            }
            "services" => {
                self.services = Some(ServicesConfig::from_value(value, file)?);
                Ok(())
            }
            "files" => {
                self.files = Some(FilesConfig::from_value(value, file)?);
                Ok(())
            }
            other => Err(format!(
                "{}: unknown top-level domain `{other}`",
                file.display()
            )),
        }
    }

    pub fn domain_count(&self) -> usize {
        if let Some(system) = &self.system {
            let _ = (&system.timezone, &system.locale, &system.keymap);
        }
        self.system.iter().count()
            + self.packages.iter().count()
            + self.services.iter().count()
            + self.files.iter().count()
    }

    pub fn package_count(&self) -> usize {
        self.packages
            .as_ref()
            .map(|packages| packages.pacman.len() + packages.aur.len() + packages.nix.len())
            .unwrap_or(0)
    }

    pub fn service_count(&self) -> usize {
        self.services
            .as_ref()
            .map(|services| {
                let _ = services.disable.len();
                services.enable.len()
            })
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, Default)]
pub struct FilesConfig {
    pub managed: Vec<ManagedFileConfig>,
}

impl FilesConfig {
    fn from_value(value: DomainValue, file: &Path) -> Result<Self, String> {
        let mut fields = value.into_table(file, "files")?;
        reject_unknown_fields(file, "files", &fields, &["managed"])?;

        Ok(Self {
            managed: take_optional_managed_files(file, "files.managed", &mut fields)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedFileConfig {
    pub path: String,
    pub content: String,
    pub mode: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SystemConfig {
    pub hostname: String,
    pub timezone: Option<String>,
    pub locale: Option<String>,
    pub keymap: Option<String>,
}

impl SystemConfig {
    fn from_value(value: DomainValue, file: &Path) -> Result<Self, String> {
        let mut fields = value.into_table(file, "system")?;
        reject_unknown_fields(
            file,
            "system",
            &fields,
            &["hostname", "timezone", "locale", "keymap"],
        )?;

        let hostname = take_required_string(file, "system.hostname", &mut fields)?;
        let timezone = take_optional_string(file, "system.timezone", &mut fields)?;
        let locale = take_optional_string(file, "system.locale", &mut fields)?;
        let keymap = take_optional_string(file, "system.keymap", &mut fields)?;

        Ok(Self {
            hostname,
            timezone,
            locale,
            keymap,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct PackagesConfig {
    pub pacman: Vec<String>,
    pub aur: Vec<String>,
    pub nix: Vec<String>,
}

impl PackagesConfig {
    fn from_value(value: DomainValue, file: &Path) -> Result<Self, String> {
        let mut fields = value.into_table(file, "packages")?;
        reject_unknown_fields(file, "packages", &fields, &["pacman", "aur", "nix"])?;

        Ok(Self {
            pacman: take_optional_list(file, "packages.pacman", &mut fields)?,
            aur: take_optional_list(file, "packages.aur", &mut fields)?,
            nix: take_optional_list(file, "packages.nix", &mut fields)?,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct ServicesConfig {
    pub enable: Vec<String>,
    pub disable: Vec<String>,
}

impl ServicesConfig {
    fn from_value(value: DomainValue, file: &Path) -> Result<Self, String> {
        let mut fields = value.into_table(file, "services")?;
        reject_unknown_fields(file, "services", &fields, &["enable", "disable"])?;

        Ok(Self {
            enable: take_optional_list(file, "services.enable", &mut fields)?,
            disable: take_optional_list(file, "services.disable", &mut fields)?,
        })
    }
}

fn reject_unknown_fields(
    file: &Path,
    domain: &str,
    fields: &BTreeMap<String, DomainValue>,
    allowed: &[&str],
) -> Result<(), String> {
    for field in fields.keys() {
        if !allowed.contains(&field.as_str()) {
            return Err(format!(
                "{}: unknown field `{domain}.{field}`",
                file.display()
            ));
        }
    }
    Ok(())
}

fn take_required_string(
    file: &Path,
    path: &str,
    fields: &mut BTreeMap<String, DomainValue>,
) -> Result<String, String> {
    match fields.remove(path.rsplit_once('.').map(|(_, key)| key).unwrap_or(path)) {
        Some(DomainValue::String(value)) => Ok(value),
        Some(_) => Err(format!("{}: `{path}` must be a string", file.display())),
        None => Err(format!(
            "{}: missing required field `{path}`",
            file.display()
        )),
    }
}

fn take_optional_string(
    file: &Path,
    path: &str,
    fields: &mut BTreeMap<String, DomainValue>,
) -> Result<Option<String>, String> {
    match fields.remove(path.rsplit_once('.').map(|(_, key)| key).unwrap_or(path)) {
        Some(DomainValue::String(value)) => Ok(Some(value)),
        Some(_) => Err(format!("{}: `{path}` must be a string", file.display())),
        None => Ok(None),
    }
}

fn take_optional_list(
    file: &Path,
    path: &str,
    fields: &mut BTreeMap<String, DomainValue>,
) -> Result<Vec<String>, String> {
    match fields.remove(path.rsplit_once('.').map(|(_, key)| key).unwrap_or(path)) {
        Some(DomainValue::List(values)) => values
            .into_iter()
            .map(|value| match value {
                DomainValue::String(value) => Ok(value),
                DomainValue::List(_) | DomainValue::Table(_) => Err(format!(
                    "{}: `{path}` must be a list of strings",
                    file.display()
                )),
            })
            .collect(),
        Some(_) => Err(format!(
            "{}: `{path}` must be a list of strings",
            file.display()
        )),
        None => Ok(Vec::new()),
    }
}

fn take_optional_managed_files(
    file: &Path,
    path: &str,
    fields: &mut BTreeMap<String, DomainValue>,
) -> Result<Vec<ManagedFileConfig>, String> {
    match fields.remove(path.rsplit_once('.').map(|(_, key)| key).unwrap_or(path)) {
        Some(DomainValue::List(values)) => values
            .into_iter()
            .enumerate()
            .map(|(index, value)| managed_file_from_value(file, path, index, value))
            .collect(),
        Some(_) => Err(format!(
            "{}: `{path}` must be a list of file tables",
            file.display()
        )),
        None => Ok(Vec::new()),
    }
}

fn managed_file_from_value(
    file: &Path,
    path: &str,
    index: usize,
    value: DomainValue,
) -> Result<ManagedFileConfig, String> {
    let item_path = format!("{path}[{}]", index + 1);
    let mut fields = value.into_table(file, &item_path)?;
    reject_unknown_fields(file, &item_path, &fields, &["path", "content", "mode"])?;

    Ok(ManagedFileConfig {
        path: take_required_string(file, &format!("{item_path}.path"), &mut fields)?,
        content: take_required_string(file, &format!("{item_path}.content"), &mut fields)?,
        mode: take_optional_string(file, &format!("{item_path}.mode"), &mut fields)?,
    })
}
