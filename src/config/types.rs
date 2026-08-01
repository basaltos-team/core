// Typed Basalt configuration structs.

use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum DomainValue {
    Boolean(bool),
    Integer(i64),
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
            DomainValue::Boolean(_)
            | DomainValue::Integer(_)
            | DomainValue::String(_)
            | DomainValue::List(_) => Err(format!(
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
    pub storage: Option<StorageConfig>,
    pub files: Option<FilesConfig>,
    pub workspaces: Option<WorkspacesConfig>,
}

impl BasaltConfig {
    pub fn has_domain(&self, domain: &str) -> bool {
        match domain {
            "system" => self.system.is_some(),
            "packages" => self.packages.is_some(),
            "services" => self.services.is_some(),
            "storage" => self.storage.is_some(),
            "files" => self.files.is_some(),
            "workspaces" => self.workspaces.is_some(),
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
            "storage" => {
                self.storage = Some(StorageConfig::from_value(value, file)?);
                Ok(())
            }
            "files" => {
                self.files = Some(FilesConfig::from_value(value, file)?);
                Ok(())
            }
            "workspaces" => {
                self.workspaces = Some(WorkspacesConfig::from_value(value, file)?);
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
            + self.storage.iter().count()
            + self.files.iter().count()
            + self.workspaces.iter().count()
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

    pub fn workspace_count(&self) -> usize {
        self.workspaces
            .as_ref()
            .map(|workspaces| workspaces.entries.len())
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub layout: String,
    pub disk: Option<String>,
    pub target: String,
    pub efi_filesystem: Option<String>,
    pub root_filesystem: Option<String>,
    pub partitions: Vec<StoragePartitionConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoragePartitionConfig {
    pub disk: String,
    pub number: Option<String>,
    pub label: Option<String>,
    pub mountpoint: Option<String>,
    pub filesystem: String,
    pub size: Option<String>,
    pub flags: Vec<String>,
    pub format: bool,
    pub mount_options: Vec<String>,
    pub subvolumes: Vec<StorageSubvolumeConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageSubvolumeConfig {
    pub name: String,
    pub mountpoint: String,
    pub mount_options: Vec<String>,
}

impl StorageConfig {
    fn from_value(value: DomainValue, file: &Path) -> Result<Self, String> {
        let mut fields = value.into_table(file, "storage")?;
        reject_unknown_fields(
            file,
            "storage",
            &fields,
            &[
                "layout",
                "disk",
                "target",
                "efi_filesystem",
                "root_filesystem",
                "partitions",
            ],
        )?;

        Ok(Self {
            layout: take_optional_string(file, "storage.layout", &mut fields)?
                .unwrap_or_else(|| "whole_disk".to_string()),
            disk: take_optional_string(file, "storage.disk", &mut fields)?,
            target: take_optional_string(file, "storage.target", &mut fields)?
                .unwrap_or_else(|| "/mnt".to_string()),
            efi_filesystem: take_optional_string(file, "storage.efi_filesystem", &mut fields)?
                .or_else(|| Some("fat32".to_string())),
            root_filesystem: take_optional_string(file, "storage.root_filesystem", &mut fields)?
                .or_else(|| Some("ext4".to_string())),
            partitions: take_optional_storage_partitions(file, "storage.partitions", &mut fields)?,
        })
    }
}

impl StoragePartitionConfig {
    fn from_value(value: DomainValue, file: &Path, path: &str) -> Result<Self, String> {
        let mut fields = value.into_table(file, path)?;
        reject_unknown_fields(
            file,
            path,
            &fields,
            &[
                "disk",
                "number",
                "label",
                "mountpoint",
                "filesystem",
                "size",
                "flags",
                "format",
                "mount_options",
                "subvolumes",
            ],
        )?;

        Ok(Self {
            disk: take_required_string(file, &format!("{path}.disk"), &mut fields)?,
            number: take_optional_string_or_integer(file, &format!("{path}.number"), &mut fields)?,
            label: take_optional_string(file, &format!("{path}.label"), &mut fields)?,
            mountpoint: take_optional_string(file, &format!("{path}.mountpoint"), &mut fields)?,
            filesystem: take_required_string(file, &format!("{path}.filesystem"), &mut fields)?,
            size: take_optional_string(file, &format!("{path}.size"), &mut fields)?,
            flags: take_optional_list(file, &format!("{path}.flags"), &mut fields)?,
            format: take_optional_bool(file, &format!("{path}.format"), &mut fields)?
                .unwrap_or(true),
            mount_options: take_optional_list(file, &format!("{path}.mount_options"), &mut fields)?,
            subvolumes: take_optional_storage_subvolumes(
                file,
                &format!("{path}.subvolumes"),
                &mut fields,
            )?,
        })
    }
}

impl StorageSubvolumeConfig {
    fn from_value(value: DomainValue, file: &Path, path: &str) -> Result<Self, String> {
        let mut fields = value.into_table(file, path)?;
        reject_unknown_fields(
            file,
            path,
            &fields,
            &["name", "mountpoint", "mount_options"],
        )?;

        Ok(Self {
            name: take_required_string(file, &format!("{path}.name"), &mut fields)?,
            mountpoint: take_required_string(file, &format!("{path}.mountpoint"), &mut fields)?,
            mount_options: take_optional_list(file, &format!("{path}.mount_options"), &mut fields)?,
        })
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

#[derive(Debug, Clone, Default)]
pub struct WorkspacesConfig {
    pub entries: BTreeMap<String, WorkspaceConfig>,
}

impl WorkspacesConfig {
    fn from_value(value: DomainValue, file: &Path) -> Result<Self, String> {
        let entries = value.into_table(file, "workspaces")?;
        let mut workspaces = BTreeMap::new();

        for (name, value) in entries {
            workspaces.insert(
                name.clone(),
                WorkspaceConfig::from_value(value, file, &name)?,
            );
        }

        Ok(Self {
            entries: workspaces,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct WorkspaceConfig {
    pub path: String,
    pub backend: String,
    pub languages: BTreeMap<String, bool>,
    pub packages: Vec<String>,
    pub services: BTreeMap<String, bool>,
    pub tasks: BTreeMap<String, String>,
}

impl WorkspaceConfig {
    fn from_value(value: DomainValue, file: &Path, name: &str) -> Result<Self, String> {
        let domain = format!("workspaces.{name}");
        let mut fields = value.into_table(file, &domain)?;
        reject_unknown_fields(
            file,
            &domain,
            &fields,
            &[
                "path",
                "backend",
                "languages",
                "packages",
                "services",
                "tasks",
            ],
        )?;

        Ok(Self {
            path: take_required_string(file, &format!("{domain}.path"), &mut fields)?,
            backend: take_optional_string(file, &format!("{domain}.backend"), &mut fields)?
                .unwrap_or_else(|| "devenv".to_string()),
            languages: take_optional_bool_map(file, &format!("{domain}.languages"), &mut fields)?,
            packages: take_optional_list(file, &format!("{domain}.packages"), &mut fields)?,
            services: take_optional_bool_map(file, &format!("{domain}.services"), &mut fields)?,
            tasks: take_optional_string_map(file, &format!("{domain}.tasks"), &mut fields)?,
        })
    }
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

fn take_optional_string_or_integer(
    file: &Path,
    path: &str,
    fields: &mut BTreeMap<String, DomainValue>,
) -> Result<Option<String>, String> {
    match fields.remove(path.rsplit_once('.').map(|(_, key)| key).unwrap_or(path)) {
        Some(DomainValue::String(value)) => Ok(Some(value)),
        Some(DomainValue::Integer(value)) => Ok(Some(value.to_string())),
        Some(_) => Err(format!(
            "{}: `{path}` must be a string or integer",
            file.display()
        )),
        None => Ok(None),
    }
}

fn take_optional_bool(
    file: &Path,
    path: &str,
    fields: &mut BTreeMap<String, DomainValue>,
) -> Result<Option<bool>, String> {
    match fields.remove(path.rsplit_once('.').map(|(_, key)| key).unwrap_or(path)) {
        Some(DomainValue::Boolean(value)) => Ok(Some(value)),
        Some(_) => Err(format!("{}: `{path}` must be a boolean", file.display())),
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
                DomainValue::Boolean(_)
                | DomainValue::Integer(_)
                | DomainValue::List(_)
                | DomainValue::Table(_) => Err(format!(
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

fn take_optional_bool_map(
    file: &Path,
    path: &str,
    fields: &mut BTreeMap<String, DomainValue>,
) -> Result<BTreeMap<String, bool>, String> {
    match fields.remove(path.rsplit_once('.').map(|(_, key)| key).unwrap_or(path)) {
        Some(DomainValue::Table(values)) => values
            .into_iter()
            .map(|(key, value)| match value {
                DomainValue::Boolean(value) => Ok((key, value)),
                DomainValue::Integer(_)
                | DomainValue::String(_)
                | DomainValue::List(_)
                | DomainValue::Table(_) => Err(format!(
                    "{}: `{path}.{key}` must be a boolean",
                    file.display()
                )),
            })
            .collect(),
        Some(_) => Err(format!("{}: `{path}` must be a table", file.display())),
        None => Ok(BTreeMap::new()),
    }
}

fn take_optional_string_map(
    file: &Path,
    path: &str,
    fields: &mut BTreeMap<String, DomainValue>,
) -> Result<BTreeMap<String, String>, String> {
    match fields.remove(path.rsplit_once('.').map(|(_, key)| key).unwrap_or(path)) {
        Some(DomainValue::Table(values)) => values
            .into_iter()
            .map(|(key, value)| match value {
                DomainValue::String(value) => Ok((key, value)),
                DomainValue::Boolean(_)
                | DomainValue::Integer(_)
                | DomainValue::List(_)
                | DomainValue::Table(_) => Err(format!(
                    "{}: `{path}.{key}` must be a string",
                    file.display()
                )),
            })
            .collect(),
        Some(_) => Err(format!("{}: `{path}` must be a table", file.display())),
        None => Ok(BTreeMap::new()),
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

fn take_optional_storage_partitions(
    file: &Path,
    path: &str,
    fields: &mut BTreeMap<String, DomainValue>,
) -> Result<Vec<StoragePartitionConfig>, String> {
    match fields.remove(path.rsplit_once('.').map(|(_, key)| key).unwrap_or(path)) {
        Some(DomainValue::List(values)) => values
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                StoragePartitionConfig::from_value(value, file, &format!("{path}[{}]", index + 1))
            })
            .collect(),
        Some(_) => Err(format!(
            "{}: `{path}` must be a list of partition tables",
            file.display()
        )),
        None => Ok(Vec::new()),
    }
}

fn take_optional_storage_subvolumes(
    file: &Path,
    path: &str,
    fields: &mut BTreeMap<String, DomainValue>,
) -> Result<Vec<StorageSubvolumeConfig>, String> {
    match fields.remove(path.rsplit_once('.').map(|(_, key)| key).unwrap_or(path)) {
        Some(DomainValue::List(values)) => values
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                StorageSubvolumeConfig::from_value(value, file, &format!("{path}[{}]", index + 1))
            })
            .collect(),
        Some(_) => Err(format!(
            "{}: `{path}` must be a list of Btrfs subvolume tables",
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
