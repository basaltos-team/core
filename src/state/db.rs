// SQLite index for run records and operation artifacts.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use crate::backends::pacman::{PackageBackend, PackageSnapshot, PackageTransaction};
use crate::state::store::RunRecord;

const STATE_DB: &str = "state.db";
const MIGRATION_VERSION: i64 = 6;

#[derive(Debug, Clone, Default)]
pub struct StateDbArtifacts {
    pub run_json_path: PathBuf,
    pub latest_json_path: PathBuf,
    pub package_intent: Vec<PackageIntent>,
    pub service_intent: Vec<ServiceIntent>,
    pub package_operations_path: Option<PathBuf>,
    pub service_operations_path: Option<PathBuf>,
    pub backup_dir: Option<PathBuf>,
    pub pacman_snapshot_before: Option<PackageSnapshot>,
    pub pacman_snapshot_after: Option<PackageSnapshot>,
    pub enabled_services_before: Option<BTreeSet<String>>,
    pub enabled_services_after: Option<BTreeSet<String>>,
    pub pacman_transaction: Option<PackageTransaction>,
    pub aur_transaction: Option<PackageTransaction>,
    pub nix_transaction: Option<PackageTransaction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageIntent {
    pub backend: PackageBackend,
    pub package: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceIntent {
    pub action: String,
    pub service: String,
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
    pub declared_services: Vec<String>,
    pub package_transaction_statuses: Vec<String>,
    pub resolved_package_transactions: Vec<String>,
    pub requested_package_operations: Vec<String>,
    pub requested_service_operations: Vec<String>,
    pub package_snapshot_changes: Vec<String>,
    pub service_snapshot_changes: Vec<String>,
    pub package_result_audit: Vec<String>,
    pub service_result_audit: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageHistoryRow {
    pub run_id: String,
    pub mode: String,
    pub created_at: String,
    pub config_path: PathBuf,
    pub intent: Vec<String>,
    pub operations: Vec<String>,
    pub snapshot_changes: Vec<String>,
    pub audit: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceHistoryRow {
    pub run_id: String,
    pub mode: String,
    pub created_at: String,
    pub config_path: PathBuf,
    pub intent: Vec<String>,
    pub operations: Vec<String>,
    pub snapshot_changes: Vec<String>,
    pub audit: Vec<String>,
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
        "delete from package_intent where run_id = ?1",
        params![record.id],
    )
    .map_err(|err| format!("failed to replace indexed package intent: {err}"))?;
    for (index, intent) in artifacts.package_intent.iter().enumerate() {
        tx.execute(
            "insert into package_intent (
                run_id,
                intent_index,
                backend,
                package
            ) values (?1, ?2, ?3, ?4)",
            params![
                record.id,
                index as i64,
                intent.backend.as_str(),
                intent.package,
            ],
        )
        .map_err(|err| format!("failed to index package intent `{}`: {err}", intent.package))?;
    }

    tx.execute(
        "delete from service_intent where run_id = ?1",
        params![record.id],
    )
    .map_err(|err| format!("failed to replace indexed service intent: {err}"))?;
    for (index, intent) in artifacts.service_intent.iter().enumerate() {
        tx.execute(
            "insert into service_intent (
                run_id,
                intent_index,
                action,
                service
            ) values (?1, ?2, ?3, ?4)",
            params![record.id, index as i64, intent.action, intent.service],
        )
        .map_err(|err| format!("failed to index service intent `{}`: {err}", intent.service))?;
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
        "delete from package_transaction_rows where run_id = ?1",
        params![record.id],
    )
    .map_err(|err| format!("failed to replace indexed package transaction rows: {err}"))?;
    tx.execute(
        "delete from package_transactions where run_id = ?1",
        params![record.id],
    )
    .map_err(|err| format!("failed to replace indexed package transactions: {err}"))?;
    if let Some(transaction) = &artifacts.pacman_transaction {
        index_package_transaction(&tx, &record.id, PackageBackend::Pacman, transaction)?;
    }
    if let Some(transaction) = &artifacts.aur_transaction {
        index_package_transaction(&tx, &record.id, PackageBackend::Aur, transaction)?;
    }
    if let Some(transaction) = &artifacts.nix_transaction {
        index_package_transaction(&tx, &record.id, PackageBackend::Nix, transaction)?;
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

    tx.execute(
        "delete from service_snapshot_services where run_id = ?1",
        params![record.id],
    )
    .map_err(|err| format!("failed to replace indexed service snapshots: {err}"))?;
    tx.execute(
        "delete from service_snapshots where run_id = ?1",
        params![record.id],
    )
    .map_err(|err| format!("failed to replace indexed service snapshot metadata: {err}"))?;
    tx.execute(
        "delete from service_snapshot_diff where run_id = ?1",
        params![record.id],
    )
    .map_err(|err| format!("failed to replace indexed service snapshot diff: {err}"))?;
    if let Some(services) = &artifacts.enabled_services_before {
        index_service_snapshot(&tx, &record.id, "before", services)?;
    }
    if let Some(services) = &artifacts.enabled_services_after {
        index_service_snapshot(&tx, &record.id, "after", services)?;
    }
    if let (Some(before), Some(after)) = (
        &artifacts.enabled_services_before,
        &artifacts.enabled_services_after,
    ) {
        index_service_snapshot_diff(&tx, &record.id, before, after)?;
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

pub fn package_history_rows(
    state_dir: &Path,
    package: &str,
    limit: usize,
) -> Result<Vec<PackageHistoryRow>, String> {
    let package = package.trim();
    if package.is_empty() {
        return Err("package history requires a package name".to_string());
    }

    let db_path = init_state_db(state_dir)?;
    let conn = open_connection(&db_path)?;
    let mut statement = conn
        .prepare(
            "select distinct runs.id, runs.mode, runs.created_at, runs.config_path
            from runs
            left join package_intent on package_intent.run_id = runs.id
            left join package_operations on package_operations.run_id = runs.id
            left join package_snapshot_diff on package_snapshot_diff.run_id = runs.id
            left join package_transaction_rows on package_transaction_rows.run_id = runs.id
            where package_intent.package = ?1
                or package_operations.package = ?1
                or package_snapshot_diff.package = ?1
                or package_transaction_rows.package = ?1
            order by runs.created_at desc, runs.id desc
            limit ?2",
        )
        .map_err(|err| format!("failed to prepare package history query: {err}"))?;
    let rows = statement
        .query_map(params![package, limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|err| format!("failed to query package history: {err}"))?;

    let mut history = Vec::new();
    for row in rows {
        let (run_id, mode, created_at, config_path) =
            row.map_err(|err| format!("failed to read package history row: {err}"))?;
        history.push(PackageHistoryRow {
            intent: package_history_intent(&conn, &run_id, package)?,
            operations: package_history_operations(&conn, &run_id, package)?,
            snapshot_changes: package_history_snapshot_changes(&conn, &run_id, package)?,
            audit: package_history_audit(&conn, &run_id, package)?,
            run_id,
            mode,
            created_at,
            config_path: PathBuf::from(config_path),
        });
    }
    Ok(history)
}

pub fn render_package_history(package: &str, rows: &[PackageHistoryRow]) -> String {
    if rows.is_empty() {
        return format!("No package history for `{}`.\n", package.trim());
    }

    let mut out = format!("Basalt package history: {}\n\n", package.trim());
    for row in rows {
        out.push_str(&format!(
            "{} | {} | {} | {}\n",
            row.run_id,
            row.mode,
            row.created_at,
            row.config_path.display()
        ));
        push_indented_list(&mut out, "intent", &row.intent);
        push_indented_list(&mut out, "operations", &row.operations);
        push_indented_list(&mut out, "snapshot", &row.snapshot_changes);
        push_indented_list(&mut out, "audit", &row.audit);
        out.push('\n');
    }
    out
}

pub fn service_history_rows(
    state_dir: &Path,
    service: &str,
    limit: usize,
) -> Result<Vec<ServiceHistoryRow>, String> {
    let service = service.trim();
    if service.is_empty() {
        return Err("service history requires a service name".to_string());
    }
    let service_unit = service_unit_alias(service);

    let db_path = init_state_db(state_dir)?;
    let conn = open_connection(&db_path)?;
    let mut statement = conn
        .prepare(
            "select distinct runs.id, runs.mode, runs.created_at, runs.config_path
            from runs
            left join service_intent on service_intent.run_id = runs.id
            left join service_operations on service_operations.run_id = runs.id
            left join service_snapshot_diff on service_snapshot_diff.run_id = runs.id
            left join service_snapshot_services on service_snapshot_services.run_id = runs.id
            where service_intent.service in (?1, ?2)
                or service_operations.service in (?1, ?2)
                or service_snapshot_diff.service in (?1, ?2)
                or service_snapshot_services.service in (?1, ?2)
            order by runs.created_at desc, runs.id desc
            limit ?3",
        )
        .map_err(|err| format!("failed to prepare service history query: {err}"))?;
    let rows = statement
        .query_map(params![service, service_unit, limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|err| format!("failed to query service history: {err}"))?;

    let mut history = Vec::new();
    for row in rows {
        let (run_id, mode, created_at, config_path) =
            row.map_err(|err| format!("failed to read service history row: {err}"))?;
        history.push(ServiceHistoryRow {
            intent: service_history_intent(&conn, &run_id, service, &service_unit)?,
            operations: service_history_operations(&conn, &run_id, service, &service_unit)?,
            snapshot_changes: service_history_snapshot_changes(&conn, &run_id, service)?,
            audit: service_history_audit(&conn, &run_id, service)?,
            run_id,
            mode,
            created_at,
            config_path: PathBuf::from(config_path),
        });
    }
    Ok(history)
}

pub fn render_service_history(service: &str, rows: &[ServiceHistoryRow]) -> String {
    if rows.is_empty() {
        return format!("No service history for `{}`.\n", service.trim());
    }

    let mut out = format!("Basalt service history: {}\n\n", service.trim());
    for row in rows {
        out.push_str(&format!(
            "{} | {} | {} | {}\n",
            row.run_id,
            row.mode,
            row.created_at,
            row.config_path.display()
        ));
        push_indented_list(&mut out, "intent", &row.intent);
        push_indented_list(&mut out, "operations", &row.operations);
        push_indented_list(&mut out, "snapshot", &row.snapshot_changes);
        push_indented_list(&mut out, "audit", &row.audit);
        out.push('\n');
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
        declared_services: declared_services(&conn, &id)?,
        package_transaction_statuses: package_transaction_statuses(&conn, &id)?,
        resolved_package_transactions: resolved_package_transactions(&conn, &id)?,
        requested_package_operations: requested_package_operations(&conn, &id)?,
        requested_service_operations: requested_service_operations(&conn, &id)?,
        package_snapshot_changes: package_snapshot_changes(&conn, &id)?,
        service_snapshot_changes: service_snapshot_changes(&conn, &id)?,
        package_result_audit: package_result_audit(&conn, &id)?,
        service_result_audit: service_result_audit(&conn, &id)?,
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
        "Declared service intent",
        &inspection.declared_services,
    );
    push_list(
        &mut out,
        "Package transaction resolution",
        &inspection.package_transaction_statuses,
    );
    push_list(
        &mut out,
        "Resolved package transactions",
        &inspection.resolved_package_transactions,
    );
    push_list(
        &mut out,
        "Requested package operations",
        &inspection.requested_package_operations,
    );
    push_list(
        &mut out,
        "Requested service operations",
        &inspection.requested_service_operations,
    );
    push_list(
        &mut out,
        "Actual package snapshot changes",
        &inspection.package_snapshot_changes,
    );
    push_list(
        &mut out,
        "Actual service snapshot changes",
        &inspection.service_snapshot_changes,
    );
    push_list(
        &mut out,
        "Package result audit",
        &inspection.package_result_audit,
    );
    push_list(
        &mut out,
        "Service result audit",
        &inspection.service_result_audit,
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

        create table if not exists package_intent (
            run_id text not null,
            intent_index integer not null,
            backend text not null,
            package text not null,
            primary key (run_id, intent_index),
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

        create table if not exists service_intent (
            run_id text not null,
            intent_index integer not null,
            action text not null,
            service text not null,
            primary key (run_id, intent_index),
            foreign key (run_id) references runs(id) on delete cascade
        );

        create table if not exists service_snapshots (
            run_id text not null,
            phase text not null,
            service_count integer not null,
            primary key (run_id, phase),
            foreign key (run_id) references runs(id) on delete cascade
        );

        create table if not exists service_snapshot_services (
            run_id text not null,
            phase text not null,
            service text not null,
            primary key (run_id, phase, service),
            foreign key (run_id) references runs(id) on delete cascade
        );

        create table if not exists service_snapshot_diff (
            run_id text not null,
            service text not null,
            change text not null,
            primary key (run_id, service, change),
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

        create table if not exists package_transactions (
            run_id text not null,
            backend text not null,
            row_count integer not null,
            status text not null default 'resolved',
            message text,
            primary key (run_id, backend),
            foreign key (run_id) references runs(id) on delete cascade
        );

        create table if not exists package_transaction_rows (
            run_id text not null,
            backend text not null,
            row_index integer not null,
            package text not null,
            version text,
            change text not null,
            reason text not null,
            primary key (run_id, backend, row_index),
            foreign key (run_id) references runs(id) on delete cascade
        );

        create index if not exists idx_runs_created_at on runs(created_at);
        create index if not exists idx_actions_domain on actions(domain);
        create index if not exists idx_package_intent_package on package_intent(package);
        create index if not exists idx_package_operations_package on package_operations(package);
        create index if not exists idx_service_intent_service on service_intent(service);
        create index if not exists idx_service_operations_service on service_operations(service);
        create index if not exists idx_service_snapshot_services_service on service_snapshot_services(service);
        create index if not exists idx_service_snapshot_diff_change on service_snapshot_diff(change);
        create index if not exists idx_package_snapshot_packages_package on package_snapshot_packages(package);
        create index if not exists idx_package_snapshot_diff_change on package_snapshot_diff(change);
        create index if not exists idx_package_transaction_rows_package on package_transaction_rows(package);
        ",
    )
    .map_err(|err| format!("failed to migrate state database: {err}"))?;
    add_column_if_missing(
        conn,
        "package_transactions",
        "status",
        "status text not null default 'resolved'",
    )?;
    add_column_if_missing(conn, "package_transactions", "message", "message text")?;

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

fn index_package_transaction(
    conn: &Connection,
    run_id: &str,
    backend: PackageBackend,
    transaction: &PackageTransaction,
) -> Result<(), String> {
    let backend = backend.as_str();
    conn.execute(
        "insert into package_transactions (
            run_id,
            backend,
            row_count,
            status,
            message
        ) values (?1, ?2, ?3, ?4, ?5)",
        params![
            run_id,
            backend,
            transaction.rows.len() as i64,
            transaction.status.as_str(),
            transaction.message,
        ],
    )
    .map_err(|err| format!("failed to index {backend} package transaction: {err}"))?;

    for (index, row) in transaction.rows.iter().enumerate() {
        conn.execute(
            "insert into package_transaction_rows (
                run_id,
                backend,
                row_index,
                package,
                version,
                change,
                reason
            ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                run_id,
                backend,
                index as i64,
                row.package,
                row.version,
                row.change.as_str(),
                row.reason.as_str(),
            ],
        )
        .map_err(|err| {
            format!(
                "failed to index package transaction row `{}`: {err}",
                row.package
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

fn index_service_snapshot(
    conn: &Connection,
    run_id: &str,
    phase: &str,
    services: &BTreeSet<String>,
) -> Result<(), String> {
    conn.execute(
        "insert into service_snapshots (
            run_id,
            phase,
            service_count
        ) values (?1, ?2, ?3)",
        params![run_id, phase, services.len() as i64],
    )
    .map_err(|err| format!("failed to index {phase} service snapshot: {err}"))?;

    for service in services {
        conn.execute(
            "insert into service_snapshot_services (
                run_id,
                phase,
                service
            ) values (?1, ?2, ?3)",
            params![run_id, phase, service],
        )
        .map_err(|err| format!("failed to index service snapshot row `{service}`: {err}"))?;
    }
    Ok(())
}

fn index_service_snapshot_diff(
    conn: &Connection,
    run_id: &str,
    before: &BTreeSet<String>,
    after: &BTreeSet<String>,
) -> Result<(), String> {
    for service in after.difference(before) {
        insert_service_snapshot_change(conn, run_id, service, "enabled")?;
    }
    for service in before.difference(after) {
        insert_service_snapshot_change(conn, run_id, service, "disabled")?;
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

fn insert_service_snapshot_change(
    conn: &Connection,
    run_id: &str,
    service: &str,
    change: &str,
) -> Result<(), String> {
    conn.execute(
        "insert into service_snapshot_diff (
            run_id,
            service,
            change
        ) values (?1, ?2, ?3)",
        params![run_id, service, change],
    )
    .map_err(|err| format!("failed to index service snapshot change `{service}`: {err}"))?;
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
        "select 'packages.' || backend || '.' || package
        from package_intent
        where run_id = ?1
        order by intent_index",
        run_id,
    )
}

fn declared_services(conn: &Connection, run_id: &str) -> Result<Vec<String>, String> {
    query_strings(
        conn,
        "select 'services.' || action || '.' || service
        from service_intent
        where run_id = ?1
        order by intent_index",
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

fn requested_service_operations(conn: &Connection, run_id: &str) -> Result<Vec<String>, String> {
    let mut statement = conn
        .prepare(
            "select action || ' ' || service
            from service_operations
            where run_id = ?1
            order by operation_index",
        )
        .map_err(|err| format!("failed to prepare service operation inspection: {err}"))?;
    let rows = statement
        .query_map(params![run_id], |row| row.get(0))
        .map_err(|err| format!("failed to inspect service operations: {err}"))?;
    collect_string_rows(rows)
}

fn package_transaction_statuses(conn: &Connection, run_id: &str) -> Result<Vec<String>, String> {
    let mut statement = conn
        .prepare(
            "select backend || ': ' || status ||
                case
                    when message is null or message = '' then ''
                    else ' - ' || message
                end
            from package_transactions
            where run_id = ?1
            order by backend",
        )
        .map_err(|err| format!("failed to prepare package transaction status inspection: {err}"))?;
    let rows = statement
        .query_map(params![run_id], |row| row.get(0))
        .map_err(|err| format!("failed to inspect package transaction statuses: {err}"))?;
    collect_string_rows(rows)
}

fn resolved_package_transactions(conn: &Connection, run_id: &str) -> Result<Vec<String>, String> {
    let mut statement = conn
        .prepare(
            "select backend || ' ' || change || ' ' || package ||
                coalesce(' ' || version, '') || ' [' || reason || ']'
            from package_transaction_rows
            where run_id = ?1
            order by backend, row_index",
        )
        .map_err(|err| format!("failed to prepare package transaction inspection: {err}"))?;
    let rows = statement
        .query_map(params![run_id], |row| row.get(0))
        .map_err(|err| format!("failed to inspect package transactions: {err}"))?;
    collect_string_rows(rows)
}

fn package_snapshot_changes(conn: &Connection, run_id: &str) -> Result<Vec<String>, String> {
    let mut statement = conn
        .prepare(
            "select package_snapshot_diff.backend || ' ' ||
                package_snapshot_diff.change || ' ' ||
                package_snapshot_diff.package ||
                coalesce(' ' || package_snapshot_packages.version, '') ||
                ' [' || coalesce(package_snapshot_packages.reason, 'unknown') || ']'
            from package_snapshot_diff
            left join package_snapshot_packages on
                package_snapshot_packages.run_id = package_snapshot_diff.run_id
                and package_snapshot_packages.backend = package_snapshot_diff.backend
                and package_snapshot_packages.package = package_snapshot_diff.package
                and package_snapshot_packages.phase = case
                    when package_snapshot_diff.change = 'added' then 'after'
                    when package_snapshot_diff.change = 'removed' then 'before'
                    else package_snapshot_diff.change
                end
            where package_snapshot_diff.run_id = ?1
            order by package_snapshot_diff.backend, package_snapshot_diff.change, package_snapshot_diff.package",
        )
        .map_err(|err| format!("failed to prepare package snapshot inspection: {err}"))?;
    let rows = statement
        .query_map(params![run_id], |row| row.get(0))
        .map_err(|err| format!("failed to inspect package snapshot changes: {err}"))?;
    collect_string_rows(rows)
}

fn service_snapshot_changes(conn: &Connection, run_id: &str) -> Result<Vec<String>, String> {
    let mut statement = conn
        .prepare(
            "select change || ' ' || service
            from service_snapshot_diff
            where run_id = ?1
            order by change, service",
        )
        .map_err(|err| format!("failed to prepare service snapshot inspection: {err}"))?;
    let rows = statement
        .query_map(params![run_id], |row| row.get(0))
        .map_err(|err| format!("failed to inspect service snapshot changes: {err}"))?;
    collect_string_rows(rows)
}

fn package_result_audit(conn: &Connection, run_id: &str) -> Result<Vec<String>, String> {
    let mut values = Vec::new();

    let mut observed_statement = conn
        .prepare(
            "select package_operations.backend || ' install ' ||
                package_operations.package || ' observed'
            from package_operations
            inner join package_snapshot_diff on
                package_snapshot_diff.run_id = package_operations.run_id
                and package_snapshot_diff.backend = package_operations.backend
                and package_snapshot_diff.package = package_operations.package
                and package_snapshot_diff.change = 'added'
            where package_operations.run_id = ?1
                and package_operations.action = 'ensure-installed'
            order by package_operations.backend, package_operations.package",
        )
        .map_err(|err| format!("failed to prepare package result audit: {err}"))?;
    let observed_rows = observed_statement
        .query_map(params![run_id], |row| row.get(0))
        .map_err(|err| format!("failed to inspect package result audit: {err}"))?;
    values.extend(collect_string_rows(observed_rows)?);

    let mut intent_satisfied_statement = conn
        .prepare(
            "select package_intent.backend || ' install ' ||
                package_intent.package || ' already satisfied'
            from package_intent
            inner join package_snapshot_packages before_snapshot on
                before_snapshot.run_id = package_intent.run_id
                and before_snapshot.backend = package_intent.backend
                and before_snapshot.package = package_intent.package
                and before_snapshot.phase = 'before'
            inner join package_snapshot_packages after_snapshot on
                after_snapshot.run_id = package_intent.run_id
                and after_snapshot.backend = package_intent.backend
                and after_snapshot.package = package_intent.package
                and after_snapshot.phase = 'after'
            where package_intent.run_id = ?1
                and not exists (
                    select 1
                    from package_operations
                    where package_operations.run_id = package_intent.run_id
                        and package_operations.backend = package_intent.backend
                        and package_operations.package = package_intent.package
                )
            order by package_intent.backend, package_intent.package",
        )
        .map_err(|err| format!("failed to prepare intent package result audit: {err}"))?;
    let intent_satisfied_rows = intent_satisfied_statement
        .query_map(params![run_id], |row| row.get(0))
        .map_err(|err| format!("failed to inspect intent package result audit: {err}"))?;
    values.extend(collect_string_rows(intent_satisfied_rows)?);

    let mut satisfied_statement = conn
        .prepare(
            "select package_operations.backend || ' install ' ||
                package_operations.package || ' already satisfied'
            from package_operations
            inner join package_snapshot_packages before_snapshot on
                before_snapshot.run_id = package_operations.run_id
                and before_snapshot.backend = package_operations.backend
                and before_snapshot.package = package_operations.package
                and before_snapshot.phase = 'before'
            inner join package_snapshot_packages after_snapshot on
                after_snapshot.run_id = package_operations.run_id
                and after_snapshot.backend = package_operations.backend
                and after_snapshot.package = package_operations.package
                and after_snapshot.phase = 'after'
            where package_operations.run_id = ?1
                and package_operations.action = 'ensure-installed'
                and not exists (
                    select 1
                    from package_snapshot_diff
                    where package_snapshot_diff.run_id = package_operations.run_id
                        and package_snapshot_diff.backend = package_operations.backend
                        and package_snapshot_diff.package = package_operations.package
                        and package_snapshot_diff.change = 'added'
                )
            order by package_operations.backend, package_operations.package",
        )
        .map_err(|err| format!("failed to prepare satisfied package result audit: {err}"))?;
    let satisfied_rows = satisfied_statement
        .query_map(params![run_id], |row| row.get(0))
        .map_err(|err| format!("failed to inspect satisfied package result audit: {err}"))?;
    values.extend(collect_string_rows(satisfied_rows)?);

    let mut missing_statement = conn
        .prepare(
            "select package_operations.backend || ' install ' ||
                package_operations.package || ' missing from actual snapshot changes'
            from package_operations
            where package_operations.run_id = ?1
                and package_operations.action = 'ensure-installed'
                and exists (
                    select 1
                    from package_snapshots
                    where package_snapshots.run_id = package_operations.run_id
                        and package_snapshots.backend = package_operations.backend
                )
                and not exists (
                    select 1
                    from package_snapshot_diff
                    where package_snapshot_diff.run_id = package_operations.run_id
                        and package_snapshot_diff.backend = package_operations.backend
                        and package_snapshot_diff.package = package_operations.package
                        and package_snapshot_diff.change = 'added'
                )
                and not exists (
                    select 1
                    from package_snapshot_packages
                    where package_snapshot_packages.run_id = package_operations.run_id
                        and package_snapshot_packages.backend = package_operations.backend
                        and package_snapshot_packages.package = package_operations.package
                        and package_snapshot_packages.phase = 'after'
                )
            order by package_operations.backend, package_operations.package",
        )
        .map_err(|err| format!("failed to prepare missing package result audit: {err}"))?;
    let missing_rows = missing_statement
        .query_map(params![run_id], |row| row.get(0))
        .map_err(|err| format!("failed to inspect missing package result audit: {err}"))?;
    values.extend(collect_string_rows(missing_rows)?);

    let mut unexpected_statement = conn
        .prepare(
            "select package_snapshot_diff.backend || ' ' ||
                package_snapshot_diff.change || ' ' ||
                package_snapshot_diff.package || ' without resolved transaction row'
            from package_snapshot_diff
            where package_snapshot_diff.run_id = ?1
                and not exists (
                    select 1
                    from package_operations
                    where package_operations.run_id = package_snapshot_diff.run_id
                        and package_operations.backend = package_snapshot_diff.backend
                        and package_operations.package = package_snapshot_diff.package
                        and package_snapshot_diff.change = 'added'
                        and package_operations.action = 'ensure-installed'
                )
            order by package_snapshot_diff.backend, package_snapshot_diff.change, package_snapshot_diff.package",
        )
        .map_err(|err| format!("failed to prepare unexpected package result audit: {err}"))?;
    let unexpected_rows = unexpected_statement
        .query_map(params![run_id], |row| row.get(0))
        .map_err(|err| format!("failed to inspect unexpected package result audit: {err}"))?;
    values.extend(collect_string_rows(unexpected_rows)?);

    Ok(values)
}

fn service_result_audit(conn: &Connection, run_id: &str) -> Result<Vec<String>, String> {
    let mut values = Vec::new();

    let mut observed_statement = conn
        .prepare(
            "select service_operations.action || ' ' ||
                service_operations.service || ' observed'
            from service_operations
            inner join service_snapshot_diff on
                service_snapshot_diff.run_id = service_operations.run_id
                and service_snapshot_diff.service = service_operations.service
                and (
                    (service_operations.action = 'enable' and service_snapshot_diff.change = 'enabled')
                    or (service_operations.action = 'disable' and service_snapshot_diff.change = 'disabled')
                )
            where service_operations.run_id = ?1
            order by service_operations.action, service_operations.service",
        )
        .map_err(|err| format!("failed to prepare service result audit: {err}"))?;
    let observed_rows = observed_statement
        .query_map(params![run_id], |row| row.get(0))
        .map_err(|err| format!("failed to inspect service result audit: {err}"))?;
    values.extend(collect_string_rows(observed_rows)?);

    let mut intent_satisfied_statement = conn
        .prepare(
            "select service_intent.action || ' ' ||
                service_intent.service || ' already satisfied'
            from service_intent
            where service_intent.run_id = ?1
                and (
                    (
                        service_intent.action = 'enable'
                        and exists (
                            select 1
                            from service_snapshot_services before_snapshot
                            where before_snapshot.run_id = service_intent.run_id
                                and before_snapshot.phase = 'before'
                                and before_snapshot.service = service_intent.service
                        )
                        and exists (
                            select 1
                            from service_snapshot_services after_snapshot
                            where after_snapshot.run_id = service_intent.run_id
                                and after_snapshot.phase = 'after'
                                and after_snapshot.service = service_intent.service
                        )
                    )
                    or (
                        service_intent.action = 'disable'
                        and not exists (
                            select 1
                            from service_snapshot_services before_snapshot
                            where before_snapshot.run_id = service_intent.run_id
                                and before_snapshot.phase = 'before'
                                and before_snapshot.service = service_intent.service
                        )
                        and not exists (
                            select 1
                            from service_snapshot_services after_snapshot
                            where after_snapshot.run_id = service_intent.run_id
                                and after_snapshot.phase = 'after'
                                and after_snapshot.service = service_intent.service
                        )
                    )
                )
                and not exists (
                    select 1
                    from service_operations
                    where service_operations.run_id = service_intent.run_id
                        and service_operations.action = service_intent.action
                        and service_operations.service = service_intent.service
                )
            order by service_intent.action, service_intent.service",
        )
        .map_err(|err| format!("failed to prepare intent service result audit: {err}"))?;
    let intent_satisfied_rows = intent_satisfied_statement
        .query_map(params![run_id], |row| row.get(0))
        .map_err(|err| format!("failed to inspect intent service result audit: {err}"))?;
    values.extend(collect_string_rows(intent_satisfied_rows)?);

    let mut satisfied_statement = conn
        .prepare(
            "select service_operations.action || ' ' ||
                service_operations.service || ' already satisfied'
            from service_operations
            where service_operations.run_id = ?1
                and (
                    (
                        service_operations.action = 'enable'
                        and exists (
                            select 1
                            from service_snapshot_services before_snapshot
                            where before_snapshot.run_id = service_operations.run_id
                                and before_snapshot.phase = 'before'
                                and before_snapshot.service = service_operations.service
                        )
                        and exists (
                            select 1
                            from service_snapshot_services after_snapshot
                            where after_snapshot.run_id = service_operations.run_id
                                and after_snapshot.phase = 'after'
                                and after_snapshot.service = service_operations.service
                        )
                    )
                    or (
                        service_operations.action = 'disable'
                        and not exists (
                            select 1
                            from service_snapshot_services before_snapshot
                            where before_snapshot.run_id = service_operations.run_id
                                and before_snapshot.phase = 'before'
                                and before_snapshot.service = service_operations.service
                        )
                        and not exists (
                            select 1
                            from service_snapshot_services after_snapshot
                            where after_snapshot.run_id = service_operations.run_id
                                and after_snapshot.phase = 'after'
                                and after_snapshot.service = service_operations.service
                        )
                    )
                )
                and not exists (
                    select 1
                    from service_snapshot_diff
                    where service_snapshot_diff.run_id = service_operations.run_id
                        and service_snapshot_diff.service = service_operations.service
                        and (
                            (service_operations.action = 'enable' and service_snapshot_diff.change = 'enabled')
                            or (service_operations.action = 'disable' and service_snapshot_diff.change = 'disabled')
                        )
                )
            order by service_operations.action, service_operations.service",
        )
        .map_err(|err| format!("failed to prepare satisfied service result audit: {err}"))?;
    let satisfied_rows = satisfied_statement
        .query_map(params![run_id], |row| row.get(0))
        .map_err(|err| format!("failed to inspect satisfied service result audit: {err}"))?;
    values.extend(collect_string_rows(satisfied_rows)?);

    let mut missing_statement = conn
        .prepare(
            "select service_operations.action || ' ' ||
                service_operations.service || ' missing from actual service snapshot changes'
            from service_operations
            where service_operations.run_id = ?1
                and exists (
                    select 1
                    from service_snapshots
                    where service_snapshots.run_id = service_operations.run_id
                )
                and not exists (
                    select 1
                    from service_snapshot_diff
                    where service_snapshot_diff.run_id = service_operations.run_id
                        and service_snapshot_diff.service = service_operations.service
                        and (
                            (service_operations.action = 'enable' and service_snapshot_diff.change = 'enabled')
                            or (service_operations.action = 'disable' and service_snapshot_diff.change = 'disabled')
                        )
                )
                and not (
                    (
                        service_operations.action = 'enable'
                        and exists (
                            select 1
                            from service_snapshot_services
                            where service_snapshot_services.run_id = service_operations.run_id
                                and service_snapshot_services.phase = 'after'
                                and service_snapshot_services.service = service_operations.service
                        )
                    )
                    or (
                        service_operations.action = 'disable'
                        and not exists (
                            select 1
                            from service_snapshot_services
                            where service_snapshot_services.run_id = service_operations.run_id
                                and service_snapshot_services.phase = 'after'
                                and service_snapshot_services.service = service_operations.service
                        )
                    )
                )
            order by service_operations.action, service_operations.service",
        )
        .map_err(|err| format!("failed to prepare missing service result audit: {err}"))?;
    let missing_rows = missing_statement
        .query_map(params![run_id], |row| row.get(0))
        .map_err(|err| format!("failed to inspect missing service result audit: {err}"))?;
    values.extend(collect_string_rows(missing_rows)?);

    let mut unexpected_statement = conn
        .prepare(
            "select change || ' ' || service || ' without requested service operation'
            from service_snapshot_diff
            where run_id = ?1
                and not exists (
                    select 1
                    from service_operations
                    where service_operations.run_id = service_snapshot_diff.run_id
                        and service_operations.service = service_snapshot_diff.service
                        and (
                            (service_operations.action = 'enable' and service_snapshot_diff.change = 'enabled')
                            or (service_operations.action = 'disable' and service_snapshot_diff.change = 'disabled')
                        )
                )
            order by change, service",
        )
        .map_err(|err| format!("failed to prepare unexpected service result audit: {err}"))?;
    let unexpected_rows = unexpected_statement
        .query_map(params![run_id], |row| row.get(0))
        .map_err(|err| format!("failed to inspect unexpected service result audit: {err}"))?;
    values.extend(collect_string_rows(unexpected_rows)?);

    Ok(values)
}

fn package_history_intent(
    conn: &Connection,
    run_id: &str,
    package: &str,
) -> Result<Vec<String>, String> {
    query_package_strings(
        conn,
        "select 'packages.' || backend || '.' || package
        from package_intent
        where run_id = ?1 and package = ?2
        order by intent_index",
        run_id,
        package,
    )
}

fn package_history_operations(
    conn: &Connection,
    run_id: &str,
    package: &str,
) -> Result<Vec<String>, String> {
    query_package_strings(
        conn,
        "select backend || ' ' || action || ' ' || package
        from package_operations
        where run_id = ?1 and package = ?2
        order by operation_index",
        run_id,
        package,
    )
}

fn package_history_snapshot_changes(
    conn: &Connection,
    run_id: &str,
    package: &str,
) -> Result<Vec<String>, String> {
    let all_changes = package_snapshot_changes(conn, run_id)?;
    Ok(all_changes
        .into_iter()
        .filter(|change| row_mentions_package(change, package))
        .collect())
}

fn package_history_audit(
    conn: &Connection,
    run_id: &str,
    package: &str,
) -> Result<Vec<String>, String> {
    let all_audit = package_result_audit(conn, run_id)?;
    Ok(all_audit
        .into_iter()
        .filter(|audit| row_mentions_package(audit, package))
        .collect())
}

fn service_history_intent(
    conn: &Connection,
    run_id: &str,
    service: &str,
    service_unit: &str,
) -> Result<Vec<String>, String> {
    query_service_strings(
        conn,
        "select 'services.' || action || '.' || service
        from service_intent
        where run_id = ?1 and service in (?2, ?3)
        order by intent_index",
        run_id,
        service,
        service_unit,
    )
}

fn service_history_operations(
    conn: &Connection,
    run_id: &str,
    service: &str,
    service_unit: &str,
) -> Result<Vec<String>, String> {
    query_service_strings(
        conn,
        "select action || ' ' || service
        from service_operations
        where run_id = ?1 and service in (?2, ?3)
        order by operation_index",
        run_id,
        service,
        service_unit,
    )
}

fn service_history_snapshot_changes(
    conn: &Connection,
    run_id: &str,
    service: &str,
) -> Result<Vec<String>, String> {
    let all_changes = service_snapshot_changes(conn, run_id)?;
    Ok(all_changes
        .into_iter()
        .filter(|change| row_mentions_service(change, service))
        .collect())
}

fn service_history_audit(
    conn: &Connection,
    run_id: &str,
    service: &str,
) -> Result<Vec<String>, String> {
    let all_audit = service_result_audit(conn, run_id)?;
    Ok(all_audit
        .into_iter()
        .filter(|audit| row_mentions_service(audit, service))
        .collect())
}

fn row_mentions_package(row: &str, package: &str) -> bool {
    row.split_whitespace().any(|part| part == package)
}

fn row_mentions_service(row: &str, service: &str) -> bool {
    let service = service.trim();
    let service_unit = service_unit_alias(service);
    row.split_whitespace()
        .any(|part| part == service || part == service_unit)
}

fn service_unit_alias(service: &str) -> String {
    if service.ends_with(".service") {
        service.to_string()
    } else {
        format!("{service}.service")
    }
}

fn query_package_strings(
    conn: &Connection,
    sql: &str,
    run_id: &str,
    package: &str,
) -> Result<Vec<String>, String> {
    let mut statement = conn
        .prepare(sql)
        .map_err(|err| format!("failed to prepare package history detail query: {err}"))?;
    let rows = statement
        .query_map(params![run_id, package], |row| row.get(0))
        .map_err(|err| format!("failed to inspect package history detail: {err}"))?;
    collect_string_rows(rows)
}

fn query_service_strings(
    conn: &Connection,
    sql: &str,
    run_id: &str,
    service: &str,
    service_unit: &str,
) -> Result<Vec<String>, String> {
    let mut statement = conn
        .prepare(sql)
        .map_err(|err| format!("failed to prepare service history detail query: {err}"))?;
    let rows = statement
        .query_map(params![run_id, service, service_unit], |row| row.get(0))
        .map_err(|err| format!("failed to inspect service history detail: {err}"))?;
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

fn push_indented_list(out: &mut String, label: &str, items: &[String]) {
    out.push_str("  ");
    out.push_str(label);
    out.push_str(":\n");
    if items.is_empty() {
        out.push_str("    - none\n");
    } else {
        for item in items {
            out.push_str("    - ");
            out.push_str(item);
            out.push('\n');
        }
    }
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

fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), String> {
    let mut statement = conn
        .prepare(&format!("pragma table_info({table})"))
        .map_err(|err| format!("failed to inspect table `{table}`: {err}"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|err| format!("failed to inspect table `{table}`: {err}"))?;
    for existing in columns {
        let existing =
            existing.map_err(|err| format!("failed to inspect table `{table}`: {err}"))?;
        if existing == column {
            return Ok(());
        }
    }
    conn.execute(&format!("alter table {table} add column {definition}"), [])
        .map_err(|err| format!("failed to add `{table}.{column}`: {err}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::pacman::{
        PackageReason, PackageResolutionStatus, PackageTransactionChange, PackageTransactionRow,
    };
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
        fs::write(
            &package_log,
            "pacman ensure-installed basalt-test\naur ensure-installed yay-bin\nnix ensure-installed hello\n",
        )
        .unwrap();
        fs::write(&service_log, "enable basalt-example.service\n").unwrap();

        let action = Action {
            id: "packages.pacman.basalt-test".to_string(),
            domain: "packages".to_string(),
            description: "ensure pacman package `basalt-test` is installed".to_string(),
            risk: Risk::High,
        };
        let aur_action = Action {
            id: "packages.aur.yay-bin".to_string(),
            domain: "packages".to_string(),
            description: "ensure AUR package `yay-bin` is installed".to_string(),
            risk: Risk::High,
        };
        let nix_action = Action {
            id: "packages.nix.hello".to_string(),
            domain: "packages".to_string(),
            description: "ensure Nix package `hello` is installed".to_string(),
            risk: Risk::High,
        };
        let record = RunRecord::apply(
            PathBuf::from("config"),
            vec![action, aur_action, nix_action],
            &CurrentState::default(),
        );
        let artifacts = StateDbArtifacts {
            run_json_path: base.join("runs").join(&record.id).join("run.json"),
            latest_json_path: base.join("latest-run.json"),
            package_intent: vec![
                PackageIntent {
                    backend: PackageBackend::Pacman,
                    package: "basalt-test".to_string(),
                },
                PackageIntent {
                    backend: PackageBackend::Aur,
                    package: "yay-bin".to_string(),
                },
                PackageIntent {
                    backend: PackageBackend::Nix,
                    package: "hello".to_string(),
                },
            ],
            service_intent: vec![ServiceIntent {
                action: "enable".to_string(),
                service: "basalt-example.service".to_string(),
            }],
            package_operations_path: Some(package_log),
            service_operations_path: Some(service_log),
            backup_dir: Some(base.join("backups").join("apply-test")),
            pacman_snapshot_before: Some(crate::backends::pacman::PackageSnapshot::from_names(
                std::collections::BTreeSet::from(["old-package".to_string()]),
            )),
            pacman_snapshot_after: Some(crate::backends::pacman::PackageSnapshot::from_names(
                std::collections::BTreeSet::from(["basalt-test".to_string()]),
            )),
            enabled_services_before: Some(BTreeSet::new()),
            enabled_services_after: Some(BTreeSet::from(["basalt-example.service".to_string()])),
            pacman_transaction: Some(crate::backends::pacman::PackageTransaction {
                status: PackageResolutionStatus::Resolved,
                message: None,
                rows: vec![PackageTransactionRow {
                    package: "basalt-test".to_string(),
                    version: Some("1.0-1".to_string()),
                    change: PackageTransactionChange::Install,
                    reason: PackageReason::Explicit,
                }],
            }),
            aur_transaction: Some(crate::backends::pacman::PackageTransaction::skipped(
                "AUR transaction resolution is not implemented yet",
            )),
            nix_transaction: Some(crate::backends::pacman::PackageTransaction::skipped(
                "Nix transaction resolution is not implemented yet",
            )),
        };

        let db_path = index_run(&base, &record, &artifacts).unwrap();
        assert!(db_path.exists());

        let rows = history_rows(&base, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].mode, "apply");
        assert_eq!(rows[0].action_count, 3);
        assert_eq!(rows[0].package_operation_count, 3);
        assert_eq!(rows[0].service_operation_count, 1);

        let inspection = inspect_run(&base, Some(&record.id)).unwrap();
        assert_eq!(
            inspection.declared_packages,
            vec![
                "packages.pacman.basalt-test",
                "packages.aur.yay-bin",
                "packages.nix.hello"
            ]
        );
        assert_eq!(
            inspection.declared_services,
            vec!["services.enable.basalt-example.service"]
        );
        assert_eq!(
            inspection.package_transaction_statuses,
            vec![
                "aur: skipped - AUR transaction resolution is not implemented yet",
                "nix: skipped - Nix transaction resolution is not implemented yet",
                "pacman: resolved"
            ]
        );
        assert_eq!(
            inspection.resolved_package_transactions,
            vec!["pacman install basalt-test 1.0-1 [explicit]"]
        );
        assert_eq!(
            inspection.requested_package_operations,
            vec![
                "pacman ensure-installed basalt-test",
                "aur ensure-installed yay-bin",
                "nix ensure-installed hello"
            ]
        );
        assert_eq!(
            inspection.requested_service_operations,
            vec!["enable basalt-example.service"]
        );
        assert_eq!(
            inspection.package_snapshot_changes,
            vec![
                "pacman added basalt-test [unknown]".to_string(),
                "pacman removed old-package [unknown]".to_string()
            ]
        );
        assert_eq!(
            inspection.service_snapshot_changes,
            vec!["enabled basalt-example.service"]
        );
        assert_eq!(
            inspection.package_result_audit,
            vec![
                "pacman install basalt-test observed".to_string(),
                "pacman removed old-package without resolved transaction row".to_string()
            ]
        );
        assert_eq!(
            inspection.service_result_audit,
            vec!["enable basalt-example.service observed"]
        );

        let rendered = render_history(&rows);
        assert!(rendered.contains("Basalt run history"));
        assert!(rendered.contains("Packages"));
        assert!(rendered.contains("apply"));

        let inspection_rendered = render_run_inspection(&inspection);
        assert!(inspection_rendered.contains("Declared package intent"));
        assert!(inspection_rendered.contains("Declared service intent"));
        assert!(inspection_rendered.contains("Package transaction resolution"));
        assert!(inspection_rendered.contains("Resolved package transactions"));
        assert!(inspection_rendered.contains("Actual package snapshot changes"));
        assert!(inspection_rendered.contains("Actual service snapshot changes"));
        assert!(inspection_rendered.contains("Package result audit"));
        assert!(inspection_rendered.contains("Service result audit"));

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn package_result_audit_marks_already_satisfied_installs() {
        let base = temp_dir("state-db-satisfied-test");
        fs::create_dir_all(&base).unwrap();
        let package_log = base.join("package-operations.log");
        fs::write(&package_log, "pacman ensure-installed git\n").unwrap();

        let action = Action {
            id: "packages.pacman.git".to_string(),
            domain: "packages".to_string(),
            description: "ensure pacman package `git` is installed".to_string(),
            risk: Risk::High,
        };
        let record = RunRecord::apply(
            PathBuf::from("config"),
            vec![action],
            &CurrentState::default(),
        );
        let snapshot = crate::backends::pacman::PackageSnapshot::from_names(
            std::collections::BTreeSet::from(["git".to_string()]),
        );
        let artifacts = StateDbArtifacts {
            run_json_path: base.join("runs").join(&record.id).join("run.json"),
            latest_json_path: base.join("latest-run.json"),
            package_intent: vec![PackageIntent {
                backend: PackageBackend::Pacman,
                package: "git".to_string(),
            }],
            package_operations_path: Some(package_log),
            pacman_snapshot_before: Some(snapshot.clone()),
            pacman_snapshot_after: Some(snapshot),
            pacman_transaction: Some(crate::backends::pacman::PackageTransaction {
                status: PackageResolutionStatus::Resolved,
                message: None,
                rows: Vec::new(),
            }),
            ..StateDbArtifacts::default()
        };

        index_run(&base, &record, &artifacts).unwrap();

        let inspection = inspect_run(&base, Some(&record.id)).unwrap();
        assert_eq!(
            inspection.package_result_audit,
            vec!["pacman install git already satisfied"]
        );

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn package_result_audit_marks_declared_intent_satisfied_without_operation() {
        let base = temp_dir("state-db-intent-satisfied-test");
        fs::create_dir_all(&base).unwrap();

        let record = RunRecord::apply(
            PathBuf::from("config"),
            Vec::new(),
            &CurrentState::default(),
        );
        let snapshot = crate::backends::pacman::PackageSnapshot::from_names(
            std::collections::BTreeSet::from(["tree".to_string()]),
        );
        let artifacts = StateDbArtifacts {
            run_json_path: base.join("runs").join(&record.id).join("run.json"),
            latest_json_path: base.join("latest-run.json"),
            package_intent: vec![PackageIntent {
                backend: PackageBackend::Pacman,
                package: "tree".to_string(),
            }],
            pacman_snapshot_before: Some(snapshot.clone()),
            pacman_snapshot_after: Some(snapshot),
            pacman_transaction: Some(crate::backends::pacman::PackageTransaction {
                status: PackageResolutionStatus::Resolved,
                message: None,
                rows: Vec::new(),
            }),
            ..StateDbArtifacts::default()
        };

        index_run(&base, &record, &artifacts).unwrap();

        let inspection = inspect_run(&base, Some(&record.id)).unwrap();
        assert_eq!(inspection.declared_packages, vec!["packages.pacman.tree"]);
        assert_eq!(
            inspection.package_result_audit,
            vec!["pacman install tree already satisfied"]
        );

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn service_result_audit_marks_declared_intent_satisfied_without_operation() {
        let base = temp_dir("state-db-service-intent-satisfied-test");
        fs::create_dir_all(&base).unwrap();

        let record = RunRecord::apply(
            PathBuf::from("config"),
            Vec::new(),
            &CurrentState {
                enabled_services: BTreeSet::from(["NetworkManager".to_string()]),
                ..CurrentState::default()
            },
        );
        let snapshot = BTreeSet::from(["NetworkManager".to_string()]);
        let artifacts = StateDbArtifacts {
            run_json_path: base.join("runs").join(&record.id).join("run.json"),
            latest_json_path: base.join("latest-run.json"),
            service_intent: vec![ServiceIntent {
                action: "enable".to_string(),
                service: "NetworkManager".to_string(),
            }],
            enabled_services_before: Some(snapshot.clone()),
            enabled_services_after: Some(snapshot),
            ..StateDbArtifacts::default()
        };

        index_run(&base, &record, &artifacts).unwrap();

        let inspection = inspect_run(&base, Some(&record.id)).unwrap();
        assert_eq!(
            inspection.declared_services,
            vec!["services.enable.NetworkManager"]
        );
        assert_eq!(
            inspection.service_result_audit,
            vec!["enable NetworkManager already satisfied"]
        );

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn package_history_filters_runs_for_one_package() {
        let base = temp_dir("state-db-package-history-test");
        fs::create_dir_all(&base).unwrap();
        let package_log = base.join("package-operations.log");
        fs::write(&package_log, "pacman ensure-installed tree\n").unwrap();

        let action = Action {
            id: "packages.pacman.tree".to_string(),
            domain: "packages".to_string(),
            description: "ensure pacman package `tree` is installed".to_string(),
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
            package_intent: vec![PackageIntent {
                backend: PackageBackend::Pacman,
                package: "tree".to_string(),
            }],
            package_operations_path: Some(package_log),
            pacman_snapshot_before: Some(crate::backends::pacman::PackageSnapshot::from_names(
                std::collections::BTreeSet::new(),
            )),
            pacman_snapshot_after: Some(crate::backends::pacman::PackageSnapshot::from_names(
                std::collections::BTreeSet::from(["tree".to_string()]),
            )),
            pacman_transaction: Some(crate::backends::pacman::PackageTransaction {
                status: PackageResolutionStatus::Resolved,
                message: None,
                rows: Vec::new(),
            }),
            ..StateDbArtifacts::default()
        };

        index_run(&base, &record, &artifacts).unwrap();

        let rows = package_history_rows(&base, "tree", 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].intent, vec!["packages.pacman.tree"]);
        assert_eq!(rows[0].operations, vec!["pacman ensure-installed tree"]);
        assert_eq!(
            rows[0].snapshot_changes,
            vec!["pacman added tree [unknown]"]
        );
        assert_eq!(rows[0].audit, vec!["pacman install tree observed"]);

        let rendered = render_package_history("tree", &rows);
        assert!(rendered.contains("Basalt package history: tree"));
        assert!(rendered.contains("pacman install tree observed"));

        let empty = package_history_rows(&base, "not-present", 10).unwrap();
        assert!(empty.is_empty());
        assert_eq!(
            render_package_history("not-present", &empty),
            "No package history for `not-present`.\n"
        );

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn service_history_filters_runs_for_one_service() {
        let base = temp_dir("state-db-service-history-test");
        fs::create_dir_all(&base).unwrap();
        let service_log = base.join("service-operations.log");
        fs::write(&service_log, "enable basalt-example.service\n").unwrap();

        let action = Action {
            id: "services.enable.basalt-example.service".to_string(),
            domain: "services".to_string(),
            description: "enable service `basalt-example.service`".to_string(),
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
            service_intent: vec![ServiceIntent {
                action: "enable".to_string(),
                service: "basalt-example.service".to_string(),
            }],
            service_operations_path: Some(service_log),
            enabled_services_before: Some(BTreeSet::new()),
            enabled_services_after: Some(BTreeSet::from(["basalt-example.service".to_string()])),
            ..StateDbArtifacts::default()
        };

        index_run(&base, &record, &artifacts).unwrap();

        let rows = service_history_rows(&base, "basalt-example", 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].intent,
            vec!["services.enable.basalt-example.service"]
        );
        assert_eq!(rows[0].operations, vec!["enable basalt-example.service"]);
        assert_eq!(
            rows[0].snapshot_changes,
            vec!["enabled basalt-example.service"]
        );
        assert_eq!(
            rows[0].audit,
            vec!["enable basalt-example.service observed"]
        );

        let rendered = render_service_history("basalt-example", &rows);
        assert!(rendered.contains("Basalt service history: basalt-example"));
        assert!(rendered.contains("enable basalt-example.service observed"));

        let empty = service_history_rows(&base, "not-present", 10).unwrap();
        assert!(empty.is_empty());
        assert_eq!(
            render_service_history("not-present", &empty),
            "No service history for `not-present`.\n"
        );

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn renders_empty_history() {
        assert_eq!(render_history(&[]), "No runs recorded.\n");
    }
}
