use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn temp_test_dir(name: &str) -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "basalt-rebuild-cli-{name}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn write_destructive_config(config_dir: &Path) {
    fs::write(
        config_dir.join("system.lua"),
        "return { system = { hostname = \"basalt-vm\" } }\n",
    )
    .unwrap();
    fs::write(
        config_dir.join("packages.lua"),
        "return { packages = { pacman = {}, aur = {}, nix = {} } }\n",
    )
    .unwrap();
    fs::write(
        config_dir.join("services.lua"),
        "return { services = { enable = {}, disable = {} } }\n",
    )
    .unwrap();
    fs::write(
        config_dir.join("storage.lua"),
        r#"return {
  storage = {
    layout = "manual",
    disk = "/dev/vda",
    target = "/mnt",
    root_filesystem = "btrfs",
    partitions = {
      {
        disk = "/dev/vda",
        number = 3,
        mountpoint = "/",
        filesystem = "btrfs",
        format = true,
      },
    },
  },
}
"#,
    )
    .unwrap();
}

#[test]
fn rebuild_rejects_destructive_storage_before_state_record() {
    let config_dir = temp_test_dir("destructive-config");
    let state_dir = temp_test_dir("state");
    write_destructive_config(&config_dir);

    let output = Command::new(env!("CARGO_BIN_EXE_basalt"))
        .arg("rebuild")
        .arg("--dry-run")
        .arg("--config")
        .arg(&config_dir)
        .arg("--state-dir")
        .arg(&state_dir)
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "destructive rebuild unexpectedly succeeded"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Basalt rebuild rejected"),
        "stderr did not contain rebuild rejection:\n{stderr}"
    );
    assert!(
        stderr.contains("format = true"),
        "stderr did not mention destructive formatting:\n{stderr}"
    );
    assert!(
        !state_dir.join("latest-run.json").exists(),
        "rejected rebuild wrote latest-run.json"
    );
    assert!(
        !state_dir.join("state.db").exists(),
        "rejected rebuild wrote state.db"
    );
}
