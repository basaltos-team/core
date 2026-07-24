# basalt

Rust implementation repository for the BasaltOS control plane.

## Owns

- CLI commands: `validate`, `diff`, `apply`, `update`, `shell`, `secret`, `install`, `recover`.
- Lua config loading and sandboxing.
- Typed schema and validation.
- Planning and diff model.
- Apply state model and command runner.
- TUI surfaces.
- Generated CLI metadata and schema artifacts consumed by docs/tests/config repos.

## Does Not Own

- PKGBUILDs for distro packages. Those live in `packages/`.
- ISO profile and live installer environment. Those live in `iso/`.
- Published pacman repository metadata. That lives in `repo-manifests/`.
- Long-form user docs. Those live in `docs/`.

## Validation

Run the local core gate with:

```sh
cargo test
```

From the full workspace, `./tests/scripts/check-local` also covers fake-root apply behavior.

## Config Loading

Basalt config directories support two shapes:

- `init.lua` entrypoint mode: when `init.lua` exists, Basalt evaluates only that file as the config entrypoint. The entrypoint may use the sandboxed `require("path.to.module")` helper to import other `.lua` files below the same config directory.
- legacy merge mode: when no `init.lua` exists, Basalt evaluates each top-level `*.lua` file and merges the returned top-level domains.

The sandboxed module loader is intentionally local to the config directory. It does not expose Lua `io`, `os`, `package`, `loadfile`, or `dofile`.

## Planned Layout

```text
basalt/
|-- Cargo.toml
|-- deny.toml
|-- rust-toolchain.toml
|-- src/
|   |-- main.rs
|   |-- cli.rs
|   |-- config/
|   |-- backends/
|   |-- system/
|   |-- secrets/
|   |-- shells/
|   |-- update/
|   |-- state/
|   |-- recovery/
|   |-- iso/
|   |-- planning/
|   |-- process/
|   `-- tui/
|-- tests/
|   |-- golden/
|   |-- fixtures/
|   `-- integration/
|-- xtask/
|-- lua/
|-- completions/
|-- man/
`-- docs/generated/
```

## First Milestone

Implement:

```sh
basalt validate --config ../configs/examples/minimal
```

## Useful Commands

```sh
basalt doctor
```

`doctor` is read-only and reports whether local development, host apply, and VM smoke prerequisites are available.

```sh
basalt workspace generate --config ../configs/fixtures/valid-devenv-workspace --output ./target/workspace-generate-smoke
```

`workspace generate` is generation-only. It validates the config and writes devenv artifacts under the output directory without running Nix or mutating profiles.
