// Hostname, locale, timezone, keymap, and identity-adjacent settings.

use std::fs;

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
