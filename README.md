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
