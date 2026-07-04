// SQLite index for run records and operation artifacts.

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use crate::backends::pacman::PackageSnapshot;
use crate::state::store::RunRecord;

const STATE_DB: &str = "state.db";
const MIGRATION_VERSION: i64 = 2;

#[derive(Debug, Clone, Default)]
pub struct StateDbArtifacts {
    pub run_json_path: PathBuf,
    pub latest_json_path: PathBuf,
    pub package_operations_path: Option<PathBuf>,
    pub service_operations_path: Option<PathBuf>,
    pub backup_dir: Option<PathBuf>,
    pub pacman_snapshot_before: Option<PackageSnapshot>,
    pub pacman_snapshot_after: Option<PackageSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRow {
    pub id: String,
    pub mode: String,
    pub action_count: usize,
    pub config_path: PathBuf,
    pub created_at: String,
    pub package_operation_count: usize,
    pub service_operation_count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunInspection {
    pub id: String,
    pub declared_packages: Vec<String>,
    pub requested_package_operations: Vec<String>,
    pub package_snapshot_changes: Vec<String>,
}

pub fn init_state_db(state_dir: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(state_dir).map_err(|err| format!("{}: {err}", state_dir.display()))?;
    let db_path = state_dir.join(STATE_DB);
    let conn = open_connection(&db_path)?;
    migrate(&conn)?;
    Ok(db_path)
}

pub fn index_run(
    state_dir: &Path,
    record: &RunRecord,
    artifacts: &StateDbArtifacts,
) -> Result<PathBuf, String> {
    let db_path = init_state_db(state_dir)?;
    let mut conn = open_connection(&db_path)?;
    let tx = conn
        .transaction()
        .map_err(|err| format!("{}: {err}", db_path.display()))?;

    tx.execute(
        "insert or replace into runs (
            id,
            mode,
            config_path,
            schema_version,
            action_count,
            current_hostname,
            pacman_package_count,
            enabled_service_count,
            run_json_path,
            latest_json_path,
            package_operations_path,
            service_operations_path,
            backup_dir
        ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            record.id,
            record.mode,
            record.config_path.display().to_string(),
            record.schema_version,
            record.action_count as i64,
            record.current_hostname,
            record.pacman_package_count as i64,
            record.enabled_service_count as i64,
            artifacts.run_json_path.display().to_string(),
            artifacts.latest_json_path.display().to_string(),
            artifacts
                .package_operations_path
                .as_ref()
                .map(|path| path.display().to_string()),
            artifacts
                .service_operations_path
                .as_ref()
                .map(|path| path.display().to_string()),
            artifacts
                .backup_dir
                .as_ref()
                .map(|path| path.display().to_string()),
        ],
    )
    .map_err(|err| format!("failed to index run {}: {err}", record.id))?;

    tx.execute("delete from actions where run_id = ?1", params![record.id])
        .map_err(|err| format!("failed to replace indexed actions: {err}"))?;
    for (index, action) in record.actions.iter().enumerate() {
        tx.execute(
            "insert into actions (
                run_id,
                action_index,
                id,
                domain,
                risk,
                description
            ) values (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.id,
                index as i64,
                action.id,
                action.domain,
                action.risk.as_str(),
                action.description,
            ],
        )
        .map_err(|err| format!("failed to index action {}: {err}", action.id))?;
    }

    tx.execute(
        "delete from package_operations where run_id = ?1",
        params![record.id],
    )
    .map_err(|err| format!("failed to replace indexed package operations: {err}"))?;
    if let Some(path) = &artifacts.package_operations_path {
        index_package_operations(&tx, &record.id, path)?;
    }

    tx.execute(
        "delete from service_operations where run_id = ?1",
        params![record.id],
    )
    .map_err(|err| format!("failed to replace indexed service operations: {err}"))?;
    if let Some(path) = &artifacts.service_operations_path {
        index_service_operations(&tx, &record.id, path)?;
    }

    tx.execute(
        "delete from package_snapshot_packages where run_id = ?1",
        params![record.id],
    )
    .map_err(|err| format!("failed to replace indexed package snapshots: {err}"))?;
    tx.execute(
        "delete from package_snapshots where run_id = ?1",
        params![record.id],
    )
    .map_err(|err| format!("failed to replace indexed package snapshot metadata: {err}"))?;
    tx.execute(
        "delete from package_snapshot_diff where run_id = ?1",
        params![record.id],
    )
    .map_err(|err| format!("failed to replace indexed package snapshot diff: {err}"))?;
    if let Some(snapshot) = &artifacts.pacman_snapshot_before {
        index_package_snapshot(&tx, &record.id, "before", "pacman", snapshot)?;
    }
    if let Some(snapshot) = &artifacts.pacman_snapshot_after {
        index_package_snapshot(&tx, &record.id, "after", "pacman", snapshot)?;
    }
    if let (Some(before), Some(after)) = (
        &artifacts.pacman_snapshot_before,
        &artifacts.pacman_snapshot_after,
    ) {
        index_package_snapshot_diff(&tx, &record.id, "pacman", before, after)?;
    }

    tx.commit()
        .map_err(|err| format!("{}: {err}", db_path.display()))?;
    Ok(db_path)
}

pub fn history_rows(state_dir: &Path, limit: usize) -> Result<Vec<HistoryRow>, String> {
    let db_path = init_state_db(state_dir)?;
    let conn = open_connection(&db_path)?;
    let mut statement = conn
        .prepare(
            "select
                runs.id,
                runs.mode,
                runs.action_count,
                runs.config_path,
                runs.created_at,
                count(distinct package_operations.operation_index) as package_operation_count,
                count(distinct service_operations.operation_index) as service_operation_count
            from runs
            left join package_operations on package_operations.run_id = runs.id
            left join service_operations on service_operations.run_id = runs.id
            group by runs.id
            order by runs.created_at desc, runs.id desc
            limit ?1",
        )
        .map_err(|err| format!("{}: {err}", db_path.display()))?;

    let rows = statement
        .query_map(params![limit as i64], |row| {
            Ok(HistoryRow {
                id: row.get(0)?,
                mode: row.get(1)?,
                action_count: row.get::<_, i64>(2)? as usize,
                config_path: PathBuf::from(row.get::<_, String>(3)?),
                created_at: row.get(4)?,
                package_operation_count: row.get::<_, i64>(5)? as usize,
                service_operation_count: row.get::<_, i64>(6)? as usize,
            })
        })
        .map_err(|err| format!("failed to query run history: {err}"))?;

    let mut history = Vec::new();
    for row in rows {
        history.push(row.map_err(|err| format!("failed to read run history row: {err}"))?);
    }
    Ok(history)
}

pub fn render_history(rows: &[HistoryRow]) -> String {
    if rows.is_empty() {
        return "No runs recorded.\n".to_string();
    }

    let mut out = String::from("Basalt run history\n\n");
    out.push_str("ID | Mode | Actions | Packages | Services | Created | Config\n");
    out.push_str("---|------|---------|----------|----------|---------|-------\n");
    for row in rows {
        out.push_str(&format!(
            "{} | {} | {} | {} | {} | {} | {}\n",
            row.id,
            row.mode,
            row.action_count,
            row.package_operation_count,
            row.service_operation_count,
            row.created_at,
            row.config_path.display()
        ));
    }
    out
}

pub fn inspect_run(state_dir: &Path, run_id: Option<&str>) -> Result<RunInspection, String> {
    let db_path = init_state_db(state_dir)?;
    let conn = open_connection(&db_path)?;
    let id = match run_id {
        Some("latest") | None => latest_run_id(&conn)?,
        Some(id) => id.to_string(),
    };

    Ok(RunInspection {
        declared_packages: declared_packages(&conn, &id)?,
        requested_package_operations: requested_package_operations(&conn, &id)?,
        package_snapshot_changes: package_snapshot_changes(&conn, &id)?,
        id,
    })
}

pub fn render_run_inspection(inspection: &RunInspection) -> String {
    let mut out = String::new();
    out.push_str("Basalt run inspection\n\n");
    out.push_str(&format!("Run: {}\n\n", inspection.id));
    push_list(
        &mut out,
        "Declared package intent",
        &inspection.declared_packages,
    );
    push_list(
        &mut out,
        "Requested package operations",
        &inspection.requested_package_operations,
    );
    push_list(
        &mut out,
        "Actual package snapshot changes",
        &inspection.package_snapshot_changes,
    );
    out
}

fn migrate(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "
        create table if not exists schema_migrations (
            version integer primary key,
            applied_at text not null default (datetime('now'))
        );

        create table if not exists runs (
            id text primary key,
            mode text not null,
            config_path text not null,
            schema_version text not null,
            action_count integer not null,
            current_hostname text,
            pacman_package_count integer not null,
            enabled_service_count integer not null,
            run_json_path text not null,
            latest_json_path text not null,
            package_operations_path text,
            service_operations_path text,
            backup_dir text,
            created_at text not null default (datetime('now'))
        );

        create table if not exists actions (
            run_id text not null,
            action_index integer not null,
            id text not null,
            domain text not null,
            risk text not null,
            description text not null,
            primary key (run_id, action_index),
            foreign key (run_id) references runs(id) on delete cascade
        );

        create table if not exists package_operations (
            run_id text not null,
            operation_index integer not null,
            backend text not null,
            action text not null,
            package text not null,
            primary key (run_id, operation_index),
            foreign key (run_id) references runs(id) on delete cascade
        );

        create table if not exists service_operations (
            run_id text not null,
            operation_index integer not null,
            action text not null,
            service text not null,
            primary key (run_id, operation_index),
            foreign key (run_id) references runs(id) on delete cascade
        );

        create table if not exists package_snapshots (
            run_id text not null,
            phase text not null,
            backend text not null,
            package_count integer not null,
            primary key (run_id, phase, backend),
            foreign key (run_id) references runs(id) on delete cascade
        );

        create table if not exists package_snapshot_packages (
            run_id text not null,
            phase text not null,
            backend text not null,
            package text not null,
            version text,
            reason text not null,
            primary key (run_id, phase, backend, package),
            foreign key (run_id) references runs(id) on delete cascade
        );

        create table if not exists package_snapshot_diff (
            run_id text not null,
            backend text not null,
            package text not null,
            change text not null,
            primary key (run_id, backend, package, change),
            foreign key (run_id) references runs(id) on delete cascade
        );

        create index if not exists idx_runs_created_at on runs(created_at);
        create index if not exists idx_actions_domain on actions(domain);
        create index if not exists idx_package_operations_package on package_operations(package);
        create index if not exists idx_service_operations_service on service_operations(service);
        create index if not exists idx_package_snapshot_packages_package on package_snapshot_packages(package);
        create index if not exists idx_package_snapshot_diff_change on package_snapshot_diff(change);
        ",
    )
    .map_err(|err| format!("failed to migrate state database: {err}"))?;

    conn.execute(
        "insert or ignore into schema_migrations (version) values (?1)",
        params![MIGRATION_VERSION],
    )
    .map_err(|err| format!("failed to record state database migration: {err}"))?;
    Ok(())
}

fn open_connection(db_path: &Path) -> Result<Connection, String> {
    Connection::open(db_path).map_err(|err| format!("{}: {err}", db_path.display()))
}

fn index_package_operations(conn: &Connection, run_id: &str, path: &Path) -> Result<(), String> {
    let contents = fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))?;
    for (index, line) in contents.lines().enumerate() {
        let mut parts = line.splitn(3, ' ');
        let Some(backend) = parts.next().filter(|value| !value.is_empty()) else {
            continue;
        };
        let action = parts
            .next()
            .ok_or_else(|| format!("{}: invalid package operation `{line}`", path.display()))?;
        let package = parts
            .next()
            .ok_or_else(|| format!("{}: invalid package operation `{line}`", path.display()))?;
        conn.execute(
            "insert into package_operations (
                run_id,
                operation_index,
                backend,
                action,
                package
            ) values (?1, ?2, ?3, ?4, ?5)",
            params![run_id, index as i64, backend, action, package],
        )
        .map_err(|err| format!("failed to index package operation `{line}`: {err}"))?;
    }
    Ok(())
}

fn index_service_operations(conn: &Connection, run_id: &str, path: &Path) -> Result<(), String> {
    let contents = fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))?;
    for (index, line) in contents.lines().enumerate() {
        let mut parts = line.splitn(2, ' ');
        let Some(action) = parts.next().filter(|value| !value.is_empty()) else {
            continue;
        };
        let service = parts
            .next()
            .ok_or_else(|| format!("{}: invalid service operation `{line}`", path.display()))?;
        conn.execute(
            "insert into service_operations (
                run_id,
                operation_index,
                action,
                service
            ) values (?1, ?2, ?3, ?4)",
            params![run_id, index as i64, action, service],
        )
        .map_err(|err| format!("failed to index service operation `{line}`: {err}"))?;
    }
    Ok(())
}

fn index_package_snapshot(
    conn: &Connection,
    run_id: &str,
    phase: &str,
    backend: &str,
    snapshot: &PackageSnapshot,
) -> Result<(), String> {
    conn.execute(
        "insert into package_snapshots (
            run_id,
            phase,
            backend,
            package_count
        ) values (?1, ?2, ?3, ?4)",
        params![run_id, phase, backend, snapshot.packages.len() as i64],
    )
    .map_err(|err| format!("failed to index {backend} package snapshot: {err}"))?;

    for package in snapshot.packages.values() {
        conn.execute(
            "insert into package_snapshot_packages (
                run_id,
                phase,
                backend,
                package,
                version,
                reason
            ) values (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                run_id,
                phase,
                backend,
                package.name,
                package.version,
                package.reason.as_str(),
            ],
        )
        .map_err(|err| {
            format!(
                "failed to index package snapshot row `{}`: {err}",
                package.name
            )
        })?;
    }
    Ok(())
}

fn index_package_snapshot_diff(
    conn: &Connection,
    run_id: &str,
    backend: &str,
    before: &PackageSnapshot,
    after: &PackageSnapshot,
) -> Result<(), String> {
    let diff = before.diff(after);
    for package in diff.added {
        insert_package_snapshot_change(conn, run_id, backend, &package, "added")?;
    }
    for package in diff.removed {
        insert_package_snapshot_change(conn, run_id, backend, &package, "removed")?;
    }
    Ok(())
}

fn insert_package_snapshot_change(
    conn: &Connection,
    run_id: &str,
    backend: &str,
    package: &str,
    change: &str,
) -> Result<(), String> {
    conn.execute(
        "insert into package_snapshot_diff (
            run_id,
            backend,
            package,
            change
        ) values (?1, ?2, ?3, ?4)",
        params![run_id, backend, package, change],
    )
    .map_err(|err| format!("failed to index package snapshot change `{package}`: {err}"))?;
    Ok(())
}

fn latest_run_id(conn: &Connection) -> Result<String, String> {
    conn.query_row(
        "select id from runs order by created_at desc, id desc limit 1",
        [],
        |row| row.get(0),
    )
    .map_err(|err| format!("failed to find latest run: {err}"))
}

fn declared_packages(conn: &Connection, run_id: &str) -> Result<Vec<String>, String> {
    query_strings(
        conn,
        "select id from actions where run_id = ?1 and domain = 'packages' order by action_index",
        run_id,
    )
}

fn requested_package_operations(conn: &Connection, run_id: &str) -> Result<Vec<String>, String> {
    let mut statement = conn
        .prepare(
            "select backend || ' ' || action || ' ' || package
            from package_operations
            where run_id = ?1
            order by operation_index",
        )
        .map_err(|err| format!("failed to prepare package operation inspection: {err}"))?;
    let rows = statement
        .query_map(params![run_id], |row| row.get(0))
        .map_err(|err| format!("failed to inspect package operations: {err}"))?;
    collect_string_rows(rows)
}

fn package_snapshot_changes(conn: &Connection, run_id: &str) -> Result<Vec<String>, String> {
    let mut statement = conn
        .prepare(
            "select backend || ' ' || change || ' ' || package
            from package_snapshot_diff
            where run_id = ?1
            order by backend, change, package",
        )
        .map_err(|err| format!("failed to prepare package snapshot inspection: {err}"))?;
    let rows = statement
        .query_map(params![run_id], |row| row.get(0))
        .map_err(|err| format!("failed to inspect package snapshot changes: {err}"))?;
    collect_string_rows(rows)
}

fn query_strings(conn: &Connection, sql: &str, run_id: &str) -> Result<Vec<String>, String> {
    let mut statement = conn
        .prepare(sql)
        .map_err(|err| format!("failed to prepare run inspection query: {err}"))?;
    let rows = statement
        .query_map(params![run_id], |row| row.get(0))
        .map_err(|err| format!("failed to inspect run: {err}"))?;
    collect_string_rows(rows)
}

fn collect_string_rows(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<String>>,
) -> Result<Vec<String>, String> {
    let mut values = Vec::new();
    for row in rows {
        values.push(row.map_err(|err| format!("failed to read inspection row: {err}"))?);
    }
    Ok(values)
}

fn push_list(out: &mut String, heading: &str, values: &[String]) {
    out.push_str(heading);
    out.push_str(":\n");
    if values.is_empty() {
        out.push_str("- none\n\n");
    } else {
        for value in values {
            out.push_str("- ");
            out.push_str(value);
            out.push('\n');
        }
        out.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::action::{Action, Risk};
    use crate::state::store::{CurrentState, RunRecord};

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
    fn indexes_run_history_and_operation_artifacts() {
        let base = temp_dir("state-db-test");
        fs::create_dir_all(&base).unwrap();
        let package_log = base.join("package-operations.log");
        let service_log = base.join("service-operations.log");
        fs::write(&package_log, "pacman ensure-installed basalt-test\n").unwrap();
        fs::write(&service_log, "enable basalt-example.service\n").unwrap();

        let action = Action {
            id: "packages.pacman.basalt-test".to_string(),
            domain: "packages".to_string(),
            description: "ensure pacman package `basalt-test` is installed".to_string(),
            risk: Risk::High,
        };
        let record = RunRecord::apply(
            PathBuf::from("config"),
            vec![action],
            &CurrentState::default(),
        );
        let artifacts = StateDbArtifacts {
            run_json_path: base.join("runs").join(&record.id).join("run.json"),
            latest_json_path: base.join("latest-run.json"),
            package_operations_path: Some(package_log),
            service_operations_path: Some(service_log),
            backup_dir: Some(base.join("backups").join("apply-test")),
            pacman_snapshot_before: Some(crate::backends::pacman::PackageSnapshot::from_names(
                std::collections::BTreeSet::from(["old-package".to_string()]),
            )),
            pacman_snapshot_after: Some(crate::backends::pacman::PackageSnapshot::from_names(
                std::collections::BTreeSet::from(["basalt-test".to_string()]),
            )),
        };

        let db_path = index_run(&base, &record, &artifacts).unwrap();
        assert!(db_path.exists());

        let rows = history_rows(&base, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].mode, "apply");
        assert_eq!(rows[0].action_count, 1);
        assert_eq!(rows[0].package_operation_count, 1);
        assert_eq!(rows[0].service_operation_count, 1);

        let inspection = inspect_run(&base, Some(&record.id)).unwrap();
        assert_eq!(
            inspection.declared_packages,
            vec!["packages.pacman.basalt-test"]
        );
        assert_eq!(
            inspection.requested_package_operations,
            vec!["pacman ensure-installed basalt-test"]
        );
        assert_eq!(
            inspection.package_snapshot_changes,
            vec![
                "pacman added basalt-test".to_string(),
                "pacman removed old-package".to_string()
            ]
        );

        let rendered = render_history(&rows);
        assert!(rendered.contains("Basalt run history"));
        assert!(rendered.contains("Packages"));
        assert!(rendered.contains("apply"));

        let inspection_rendered = render_run_inspection(&inspection);
        assert!(inspection_rendered.contains("Declared package intent"));
        assert!(inspection_rendered.contains("Actual package snapshot changes"));

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn renders_empty_history() {
        assert_eq!(render_history(&[]), "No runs recorded.\n");
    }
}
