// AUR helper integration and build environment sanitization.

use crate::backends::pacman::PackageTransaction;

pub fn resolve_aur_install_transaction(packages: &[String]) -> Result<PackageTransaction, String> {
    let requested = packages
        .iter()
        .map(|package| package.trim())
        .filter(|package| !package.is_empty())
        .count();
    if requested == 0 {
        return Ok(PackageTransaction::skipped("no AUR packages requested"));
    }

    Ok(PackageTransaction::skipped(
        "AUR transaction resolution is not implemented yet",
    ))
}
