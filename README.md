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

- PKGBUILDs for distro packages. Those live in `basalt-packages/`.
- ISO profile and live installer environment. Those live in `basalt-iso/`.
- Published pacman repository metadata. That lives in `basalt-repo/`.
- Long-form user docs. Those live in `basalt-docs/`.

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
basalt validate --config ../basalt-configs/examples/minimal
```
