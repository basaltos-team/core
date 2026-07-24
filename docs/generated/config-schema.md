# Basalt Config Schema v0

Generated from the Basalt implementation.

## Supported Domains

- `system`: owns host identity intent
- `packages`: owns package desired state intent
- `services`: owns service desired state intent
- `files`: owns managed file desired state intent
- `workspaces`: owns generated developer workspace environment intent

## Fields

| Field | Type | Required |
|---|---|---|
| `system.hostname` | `string` | yes |
| `system.timezone` | `string` | no |
| `system.locale` | `string` | no |
| `system.keymap` | `string` | no |
| `packages.pacman` | `list<string>` | no |
| `packages.aur` | `list<string>` | no |
| `packages.nix` | `list<string>` | no |
| `services.enable` | `list<string>` | no |
| `services.disable` | `list<string>` | no |
| `files.managed` | `list<table>` | no |
| `files.managed[].path` | `string` | yes |
| `files.managed[].content` | `string` | yes |
| `files.managed[].mode` | `string` | no |
| `workspaces.<name>.path` | `string` | yes |
| `workspaces.<name>.backend` | `string` | no |
| `workspaces.<name>.languages` | `table<boolean>` | no |
| `workspaces.<name>.packages` | `list<string>` | no |
| `workspaces.<name>.services` | `table<boolean>` | no |
| `workspaces.<name>.tasks` | `table<string>` | no |

## Ownership Notes

- `system` owns host identity intent.
- `packages` owns package desired state intent.
- `services` owns service desired state intent.
- `files` owns managed file desired state intent.
- `workspaces` owns generated developer workspace environment intent.
