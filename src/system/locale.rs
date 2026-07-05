// Hostname, locale, timezone, keymap, and identity-adjacent settings.

use std::fs;
use std::path::Path;

use crate::process::command::run_capture;

pub fn read_hostname() -> Option<String> {
    if let Ok(hostname) = fs::read_to_string("/etc/hostname") {
        let hostname = hostname.trim();
        if !hostname.is_empty() {
            return Some(hostname.to_string());
        }
    }

    let output = run_capture("hostname", &[]).ok()?;
    if output.status_code == Some(0) {
        let hostname = output.stdout.trim();
        if !hostname.is_empty() {
            return Some(hostname.to_string());
        }
    }

    None
}

pub fn read_hostname_from_root(root_dir: &Path) -> Option<String> {
    read_trimmed_file(&root_dir.join("etc/hostname"))
}

pub fn read_locale(root_dir: &Path) -> Option<String> {
    read_key_value(&root_dir.join("etc/locale.conf"), "LANG")
}

pub fn read_keymap(root_dir: &Path) -> Option<String> {
    read_key_value(&root_dir.join("etc/vconsole.conf"), "KEYMAP")
}

pub fn read_timezone(root_dir: &Path) -> Option<String> {
    let target = fs::read_link(root_dir.join("etc/localtime")).ok()?;
    let target = target.to_string_lossy();
    let (_, timezone) = target.rsplit_once("/usr/share/zoneinfo/")?;
    if timezone.is_empty() {
        None
    } else {
        Some(timezone.to_string())
    }
}

fn read_trimmed_file(path: &Path) -> Option<String> {
    let value = fs::read_to_string(path).ok()?;
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn read_key_value(path: &Path, key: &str) -> Option<String> {
    let contents = fs::read_to_string(path).ok()?;
    for line in contents.lines() {
        let Some((candidate, value)) = line.split_once('=') else {
            continue;
        };
        if candidate.trim() == key {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}
