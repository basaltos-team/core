// Schema version metadata and generated reference output.

use std::fs;
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: &str = "0";

#[derive(Debug, Clone, Copy)]
struct DomainSchema {
    name: &'static str,
    ownership: &'static str,
    fields: &'static [FieldSchema],
}

#[derive(Debug, Clone, Copy)]
struct FieldSchema {
    path: &'static str,
    required: bool,
    field_type: &'static str,
}

const DOMAINS: &[DomainSchema] = &[
    DomainSchema {
        name: "system",
        ownership: "owns host identity intent",
        fields: &[
            FieldSchema {
                path: "system.hostname",
                required: true,
                field_type: "string",
            },
            FieldSchema {
                path: "system.timezone",
                required: false,
                field_type: "string",
            },
            FieldSchema {
                path: "system.locale",
                required: false,
                field_type: "string",
            },
            FieldSchema {
                path: "system.keymap",
                required: false,
                field_type: "string",
            },
        ],
    },
    DomainSchema {
        name: "packages",
        ownership: "owns package desired state intent",
        fields: &[
            FieldSchema {
                path: "packages.pacman",
                required: false,
                field_type: "list<string>",
            },
            FieldSchema {
                path: "packages.aur",
                required: false,
                field_type: "list<string>",
            },
            FieldSchema {
                path: "packages.nix",
                required: false,
                field_type: "list<string>",
            },
        ],
    },
    DomainSchema {
        name: "services",
        ownership: "owns service desired state intent",
        fields: &[
            FieldSchema {
                path: "services.enable",
                required: false,
                field_type: "list<string>",
            },
            FieldSchema {
                path: "services.disable",
                required: false,
                field_type: "list<string>",
            },
        ],
    },
    DomainSchema {
        name: "files",
        ownership: "owns managed file desired state intent",
        fields: &[
            FieldSchema {
                path: "files.managed",
                required: false,
                field_type: "list<table>",
            },
            FieldSchema {
                path: "files.managed[].path",
                required: true,
                field_type: "string",
            },
            FieldSchema {
                path: "files.managed[].content",
                required: true,
                field_type: "string",
            },
            FieldSchema {
                path: "files.managed[].mode",
                required: false,
                field_type: "string",
            },
        ],
    },
];

pub fn generate_schema_artifacts(repo_root: &Path) -> Result<Vec<PathBuf>, String> {
    let json_path = repo_root.join("schemas/basalt-config-v0.json");
    let markdown_path = repo_root.join("docs/generated/config-schema.md");

    write_file(&json_path, &render_json())?;
    write_file(&markdown_path, &render_markdown())?;

    Ok(vec![json_path, markdown_path])
}

pub fn render_json() -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&format!("  \"schema_version\": \"{}\",\n", SCHEMA_VERSION));
    out.push_str("  \"domains\": [\n");

    for (domain_index, domain) in DOMAINS.iter().enumerate() {
        out.push_str("    {\n");
        out.push_str(&format!("      \"name\": \"{}\",\n", domain.name));
        out.push_str(&format!("      \"ownership\": \"{}\",\n", domain.ownership));
        out.push_str("      \"fields\": [\n");

        for (field_index, field) in domain.fields.iter().enumerate() {
            out.push_str("        {\n");
            out.push_str(&format!("          \"path\": \"{}\",\n", field.path));
            out.push_str(&format!("          \"type\": \"{}\",\n", field.field_type));
            out.push_str(&format!("          \"required\": {}\n", field.required));
            out.push_str("        }");
            if field_index + 1 != domain.fields.len() {
                out.push(',');
            }
            out.push('\n');
        }

        out.push_str("      ]\n");
        out.push_str("    }");
        if domain_index + 1 != DOMAINS.len() {
            out.push(',');
        }
        out.push('\n');
    }

    out.push_str("  ]\n");
    out.push_str("}\n");
    out
}

pub fn render_markdown() -> String {
    let mut out = String::new();
    out.push_str(&format!("# Basalt Config Schema v{}\n\n", SCHEMA_VERSION));
    out.push_str("Generated from the Basalt implementation.\n\n");
    out.push_str("## Supported Domains\n\n");

    for domain in DOMAINS {
        out.push_str(&format!("- `{}`: {}\n", domain.name, domain.ownership));
    }

    out.push_str("\n## Fields\n\n");
    out.push_str("| Field | Type | Required |\n");
    out.push_str("|---|---|---|\n");

    for domain in DOMAINS {
        for field in domain.fields {
            out.push_str(&format!(
                "| `{}` | `{}` | {} |\n",
                field.path,
                field.field_type,
                if field.required { "yes" } else { "no" }
            ));
        }
    }

    out.push_str("\n## Ownership Notes\n\n");
    for domain in DOMAINS {
        out.push_str(&format!("- `{}` {}.\n", domain.name, domain.ownership));
    }

    out
}

fn write_file(path: &Path, content: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{}: path has no parent directory", path.display()))?;
    fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
    fs::write(path, content).map_err(|err| format!("{}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_required_schema_fields() {
        let json = render_json();
        assert!(json.contains("\"schema_version\": \"0\""));
        assert!(json.contains("\"path\": \"system.hostname\""));
        assert!(json.contains("\"type\": \"list<string>\""));

        let markdown = render_markdown();
        assert!(markdown.contains("# Basalt Config Schema v0"));
        assert!(markdown.contains("| `services.disable` | `list<string>` | no |"));
        assert!(markdown.contains("| `files.managed[].content` | `string` | yes |"));
    }
}
