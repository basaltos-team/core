// Restore and recovery commands.

use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};

pub const BACKUP_MANIFEST: &str = "backup-manifest.tsv";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupManifestEntry {
    pub relative_path: String,
    pub kind: BackupKind,
    pub backup_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupKind {
    Missing,
    File,
    Symlink,
}

impl BackupKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::File => "file",
            Self::Symlink => "symlink",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "missing" => Ok(Self::Missing),
            "file" => Ok(Self::File),
            "symlink" => Ok(Self::Symlink),
            other => Err(format!("unknown backup kind `{other}`")),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestoreSummary {
    pub restored: Vec<PathBuf>,
    pub removed: Vec<PathBuf>,
}

pub fn backup_target(target: &Path, backup_dir: &Path, relative_path: &str) -> Result<(), String> {
    validate_relative_path(relative_path)?;
    fs::create_dir_all(backup_dir).map_err(|err| format!("{}: {err}", backup_dir.display()))?;

    if !target.exists() && !target.is_symlink() {
        append_manifest_entry(
            backup_dir,
            &BackupManifestEntry {
                relative_path: relative_path.to_string(),
                kind: BackupKind::Missing,
                backup_name: None,
            },
        )?;
        return Ok(());
    }

    let backup_name = backup_name(relative_path);
    let backup_path = backup_dir.join(&backup_name);
    if target.is_symlink() {
        let link = fs::read_link(target).map_err(|err| format!("{}: {err}", target.display()))?;
        fs::write(&backup_path, link.display().to_string())
            .map_err(|err| format!("{}: {err}", backup_path.display()))?;
        append_manifest_entry(
            backup_dir,
            &BackupManifestEntry {
                relative_path: relative_path.to_string(),
                kind: BackupKind::Symlink,
                backup_name: Some(backup_name),
            },
        )?;
    } else if target.is_file() {
        fs::copy(target, &backup_path).map_err(|err| {
            format!(
                "failed to back up {} to {}: {err}",
                target.display(),
                backup_path.display()
            )
        })?;
        append_manifest_entry(
            backup_dir,
            &BackupManifestEntry {
                relative_path: relative_path.to_string(),
                kind: BackupKind::File,
                backup_name: Some(backup_name),
            },
        )?;
    } else {
        return Err(format!(
            "{}: backup only supports files and symlinks",
            target.display()
        ));
    }

    Ok(())
}

pub fn restore_backup(root_dir: &Path, backup_dir: &Path) -> Result<RestoreSummary, String> {
    let entries = read_backup_manifest(backup_dir)?;
    let mut summary = RestoreSummary::default();

    for entry in entries {
        validate_relative_path(&entry.relative_path)?;
        let target = root_dir.join(&entry.relative_path);
        match entry.kind {
            BackupKind::Missing => {
                remove_target_if_present(&target)?;
                summary.removed.push(target);
            }
            BackupKind::File => {
                let backup_path = backup_dir.join(required_backup_name(&entry)?);
                replace_target_with_file(&target, &backup_path)?;
                summary.restored.push(target);
            }
            BackupKind::Symlink => {
                let backup_path = backup_dir.join(required_backup_name(&entry)?);
                let link_target = fs::read_to_string(&backup_path)
                    .map_err(|err| format!("{}: {err}", backup_path.display()))?;
                replace_target_with_symlink(&target, link_target.trim())?;
                summary.restored.push(target);
            }
        }
    }

    Ok(summary)
}

pub fn read_backup_manifest(backup_dir: &Path) -> Result<Vec<BackupManifestEntry>, String> {
    let path = backup_dir.join(BACKUP_MANIFEST);
    let contents = fs::read_to_string(&path).map_err(|err| format!("{}: {err}", path.display()))?;
    let mut entries = Vec::new();
    for (index, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() != 3 {
            return Err(format!(
                "{}:{}: expected relative_path, kind, backup_name",
                path.display(),
                index + 1
            ));
        }
        let backup_name = if parts[2] == "-" {
            None
        } else {
            Some(parts[2].to_string())
        };
        entries.push(BackupManifestEntry {
            relative_path: parts[0].to_string(),
            kind: BackupKind::parse(parts[1])?,
            backup_name,
        });
    }
    Ok(entries)
}

fn append_manifest_entry(backup_dir: &Path, entry: &BackupManifestEntry) -> Result<(), String> {
    if entry.relative_path.contains('\t') {
        return Err(format!(
            "backup manifest path cannot contain tabs: {}",
            entry.relative_path
        ));
    }
    let backup_name = entry.backup_name.as_deref().unwrap_or("-");
    if backup_name.contains('\t') {
        return Err(format!(
            "backup manifest backup name cannot contain tabs: {backup_name}"
        ));
    }

    let path = backup_dir.join(BACKUP_MANIFEST);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|err| format!("{}: {err}", path.display()))?;
    writeln!(
        file,
        "{}\t{}\t{}",
        entry.relative_path,
        entry.kind.as_str(),
        backup_name
    )
    .map_err(|err| format!("{}: {err}", path.display()))?;
    Ok(())
}

fn backup_name(relative_path: &str) -> String {
    relative_path.replace('/', "__")
}

fn required_backup_name(entry: &BackupManifestEntry) -> Result<&str, String> {
    entry
        .backup_name
        .as_deref()
        .ok_or_else(|| format!("backup entry `{}` has no backup file", entry.relative_path))
}

fn replace_target_with_file(target: &Path, backup_path: &Path) -> Result<(), String> {
    remove_target_if_present(target)?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
    }
    fs::copy(backup_path, target).map_err(|err| {
        format!(
            "failed to restore {} from {}: {err}",
            target.display(),
            backup_path.display()
        )
    })?;
    Ok(())
}

fn replace_target_with_symlink(target: &Path, link_target: &str) -> Result<(), String> {
    remove_target_if_present(target)?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
    }

    #[cfg(unix)]
    {
        unix_fs::symlink(link_target, target)
            .map_err(|err| format!("{}: {err}", target.display()))?;
    }
    #[cfg(not(unix))]
    {
        fs::write(target, link_target).map_err(|err| format!("{}: {err}", target.display()))?;
    }
    Ok(())
}

fn remove_target_if_present(target: &Path) -> Result<(), String> {
    if target.is_symlink() || target.is_file() {
        fs::remove_file(target).map_err(|err| format!("{}: {err}", target.display()))?;
    } else if target.exists() {
        return Err(format!(
            "{}: restore only removes files and symlinks",
            target.display()
        ));
    }
    Ok(())
}

fn validate_relative_path(relative_path: &str) -> Result<(), String> {
    let path = Path::new(relative_path);
    if relative_path.is_empty()
        || path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(format!(
            "backup path `{relative_path}` must stay inside the target root"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "basalt-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ))
    }

    #[test]
    fn restores_files_and_removes_created_files() {
        let base = temp_dir("restore-test");
        let root = base.join("root");
        let backup = base.join("backup");
        fs::create_dir_all(root.join("etc/basalt")).unwrap();
        fs::write(root.join("etc/hostname"), "old-host\n").unwrap();

        backup_target(&root.join("etc/hostname"), &backup, "etc/hostname").unwrap();
        backup_target(&root.join("etc/basalt/motd"), &backup, "etc/basalt/motd").unwrap();
        fs::write(root.join("etc/hostname"), "new-host\n").unwrap();
        fs::write(root.join("etc/basalt/motd"), "new file\n").unwrap();

        let summary = restore_backup(&root, &backup).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("etc/hostname")).unwrap(),
            "old-host\n"
        );
        assert!(!root.join("etc/basalt/motd").exists());
        assert_eq!(summary.restored.len(), 1);
        assert_eq!(summary.removed.len(), 1);

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn rejects_path_traversal() {
        let err = validate_relative_path("../etc/passwd").unwrap_err();
        assert!(err.contains("target root"));
    }
}
