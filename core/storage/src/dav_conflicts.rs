//! Atomic preservation of rejected DAV writes as ordinary local workspace records.
use maicenta_application::{ApplicationError, DavConflict, DavItemKind};
use maicenta_domain::{AccountId, DavCollection, WorkspaceItemId};
use rusqlite::{OptionalExtension, params};

use super::{SqliteMailStore, invalid_data, storage_error};

impl DavItemKindSql for DavItemKind {
    fn table(self) -> &'static str {
        match self {
            Self::Event => "calendar_events",
            Self::Task => "tasks",
            Self::Contact => "contacts",
        }
    }
    fn columns(self) -> &'static str {
        match self {
            Self::Event => {
                "starts_at_ms, ends_at_ms, all_day, location, description, reminder_minutes_before, reminder_dismissed_through_ms, recurrence_frequency, recurrence_interval, recurrence_weekdays, recurrence_until_ms, recurrence_count, exception_dates"
            }
            Self::Task => {
                "description, starts_at_ms, due_at_ms, status, priority, reminder_at_ms, reminder_dismissed, completed_at_ms"
            }
            Self::Contact => "company, job_title, birthday, website, notes",
        }
    }
}

trait DavItemKindSql {
    fn table(self) -> &'static str;
    fn columns(self) -> &'static str;
}

impl SqliteMailStore {
    pub(super) fn preserve_conflict(
        &mut self,
        collection: &DavCollection,
        item_id: &WorkspaceItemId,
        kind: DavItemKind,
        href: &str,
        uid: &str,
    ) -> Result<(), ApplicationError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(storage_error)?;
        let table = kind.table();
        let title_column = if kind == DavItemKind::Contact {
            "display_name"
        } else {
            "title"
        };
        let dirty: bool = transaction
            .query_row(
                &format!("SELECT dirty FROM {table} WHERE id = ?1 AND collection_id = ?2"),
                params![item_id.as_str(), collection.id.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?
            .ok_or(ApplicationError::NotFound)?;
        // Both the snapshot and this flag commit together. Repeating the
        // operation after a crash or reopening cannot create another copy.
        if !dirty {
            return Ok(());
        }
        let recovery_id: String = transaction
            .query_row(
                "SELECT 'dav-conflict.' || lower(hex(randomblob(16)))",
                [],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        let columns = kind.columns();
        transaction
            .execute(
                &format!(
                    "INSERT INTO {table} (id, {title_column}, {columns})
                      SELECT ?1, {title_column} || ' (DAV-Konfliktkopie)', {columns}
                      FROM {table} WHERE id = ?2"
                ),
                params![recovery_id, item_id.as_str()],
            )
            .map_err(storage_error)?;
        if kind == DavItemKind::Contact {
            for (child, columns) in [
                ("contact_emails", "position, label, address"),
                ("contact_phones", "position, label, number"),
                (
                    "contact_addresses",
                    "position, label, street, postal_code, city, country",
                ),
            ] {
                transaction
                    .execute(
                        &format!(
                            "INSERT INTO {child} (contact_id, {columns})
                              SELECT ?1, {columns} FROM {child} WHERE contact_id = ?2"
                        ),
                        params![recovery_id, item_id.as_str()],
                    )
                    .map_err(storage_error)?;
            }
        }
        transaction
            .execute(
                &format!(
                    "INSERT INTO dav_conflicts (
                recovery_id, item_kind, original_id, account_id, collection_id,
                collection_name, original_title, uid, href, etag
             ) SELECT ?1, ?2, id, ?3, collection_id, ?4, {title_column}, ?5, ?6, etag
               FROM {table} WHERE id = ?7"
                ),
                params![
                    recovery_id,
                    table,
                    collection.account_id.as_str(),
                    collection.display_name,
                    uid,
                    href,
                    item_id.as_str()
                ],
            )
            .map_err(storage_error)?;
        // An unknown ETag forces a pull even when a server returns an old
        // listing. New-resource collisions also acquire the rejected href.
        transaction.execute(
            &format!("UPDATE {table} SET dirty = 0, etag = NULL, remote_href = ?2, uid = ?3 WHERE id = ?1"),
            params![item_id.as_str(), href, uid],
        ).map_err(storage_error)?;
        transaction.commit().map_err(storage_error)
    }

    pub(super) fn conflicts(
        &self,
        account_id: Option<&AccountId>,
    ) -> Result<Vec<DavConflict>, ApplicationError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT recovery_id, collection_name, original_title FROM dav_conflicts
             WHERE ?1 IS NULL OR account_id = ?1 ORDER BY rowid",
            )
            .map_err(storage_error)?;
        statement
            .query_map([account_id.map(AccountId::as_str)], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(storage_error)?
            .map(|row| {
                let (id, collection_name, original_title) = row.map_err(storage_error)?;
                Ok(DavConflict {
                    recovery_id: WorkspaceItemId::parse(id).map_err(invalid_data)?,
                    collection_name,
                    original_title,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maicenta_application::{DavStore, WorkspaceStore};
    use maicenta_domain::{CollectionId, CollectionKind};

    fn seed(store: &mut SqliteMailStore, kind: DavItemKind) -> DavCollection {
        let collection = DavCollection {
            id: CollectionId::parse("synthetic.collection").unwrap(),
            account_id: AccountId::parse("synthetic").unwrap(),
            kind: CollectionKind::Calendar,
            href: "/collection/".into(),
            display_name: "Synthetic".into(),
            color: None,
            ctag: None,
            enabled: true,
            position: 0,
        };
        store
            .connection()
            .unwrap()
            .execute_batch(
                "INSERT INTO dav_accounts (id, display_name, server_url, username)
             VALUES ('synthetic', 'Synthetic', 'https://example.invalid', 'synthetic');",
            )
            .unwrap();
        store.save_dav_collection(&collection).unwrap();
        let (table, columns, values) = match kind {
            DavItemKind::Event => (
                "calendar_events",
                "title, starts_at_ms, ends_at_ms",
                "'Local edit', 10, 20",
            ),
            DavItemKind::Task => (
                "tasks",
                "title, description",
                "'Local edit', 'Synthetic note'",
            ),
            DavItemKind::Contact => (
                "contacts",
                "display_name, notes",
                "'Local edit', 'Synthetic note'",
            ),
        };
        store.connection().unwrap().execute_batch(&format!(
            "INSERT INTO {table} (id, {columns}, collection_id, uid, remote_href, etag, dirty)
             VALUES ('item', {values}, 'synthetic.collection', 'item', '/collection/item', 'old', 1);"
        )).unwrap();
        if kind == DavItemKind::Contact {
            store.connection().unwrap().execute_batch(
                "INSERT INTO contact_emails VALUES ('item', 0, 'work', 'synthetic@example.invalid');"
            ).unwrap();
        }
        collection
    }

    #[test]
    fn rolls_back_copy_notice_and_source_on_storage_failure() {
        for kind in [DavItemKind::Event, DavItemKind::Task, DavItemKind::Contact] {
            for failure in ["journal", "source", "copy"] {
                let mut store = SqliteMailStore::open_in_memory().unwrap();
                let collection = seed(&mut store, kind);
                let table = kind.table();
                let (operation, target) = match failure {
                    "journal" => ("INSERT", "dav_conflicts"),
                    "source" => ("UPDATE", table),
                    "copy" => ("INSERT", table),
                    _ => unreachable!(),
                };
                store
                    .connection()
                    .unwrap()
                    .execute_batch(&format!(
                        "CREATE TRIGGER injected_failure BEFORE {operation} ON {target}
                     BEGIN SELECT RAISE(ABORT, 'injected storage failure'); END;"
                    ))
                    .unwrap();
                let result = store.preserve_dav_conflict(
                    &collection,
                    &WorkspaceItemId::parse("item").unwrap(),
                    kind,
                    "/collection/item",
                    "item",
                );
                assert!(
                    result
                        .unwrap_err()
                        .to_string()
                        .contains("injected storage failure")
                );
                assert_eq!(store.list_dav_conflicts(None).unwrap(), Vec::new());
                let state: (i64, bool, String) = store
                    .connection()
                    .unwrap()
                    .query_row(
                        &format!("SELECT count(*), dirty, etag FROM {table}"),
                        [],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )
                    .unwrap();
                assert_eq!(state, (1, true, "old".into()));
                store
                    .connection()
                    .unwrap()
                    .execute_batch("DROP TRIGGER injected_failure;")
                    .unwrap();
                for _ in 0..2 {
                    store
                        .preserve_dav_conflict(
                            &collection,
                            &WorkspaceItemId::parse("item").unwrap(),
                            kind,
                            "/collection/item",
                            "item",
                        )
                        .unwrap();
                }
                assert_eq!(store.list_dav_conflicts(None).unwrap().len(), 1);
                let count: i64 = store
                    .connection()
                    .unwrap()
                    .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                        row.get(0)
                    })
                    .unwrap();
                assert_eq!(count, 2);
            }
        }
    }

    #[test]
    fn rolls_back_partial_contact_copy() {
        let mut store = SqliteMailStore::open_in_memory().unwrap();
        let collection = seed(&mut store, DavItemKind::Contact);
        store
            .connection()
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER injected_failure BEFORE INSERT ON contact_emails
             BEGIN SELECT RAISE(ABORT, 'injected child failure'); END;",
            )
            .unwrap();
        let before = store.list_contacts().unwrap();
        assert!(
            store
                .preserve_dav_conflict(
                    &collection,
                    &WorkspaceItemId::parse("item").unwrap(),
                    DavItemKind::Contact,
                    "/collection/item",
                    "item"
                )
                .is_err()
        );
        assert_eq!(store.list_contacts().unwrap(), before);
        assert_eq!(store.list_dav_conflicts(None).unwrap(), Vec::new());
    }

    #[test]
    fn migrates_v18_without_changing_records_and_keeps_recovery_after_reopen() {
        let path = super::super::tests::TemporaryDatabase::new();
        let mut store = SqliteMailStore::open(&path.path).unwrap();
        let collection = seed(&mut store, DavItemKind::Contact);
        let original = store.list_contacts().unwrap();
        // Remove exactly migration 19 to produce the preceding schema with
        // populated DAV records, then exercise the normal open/migrate path.
        store
            .connection()
            .unwrap()
            .execute_batch(
                "DROP TRIGGER calendar_conflict_removed;
             DROP TRIGGER task_conflict_removed;
             DROP TRIGGER contact_conflict_removed;
             DROP TABLE dav_conflicts;
             PRAGMA user_version = 18;",
            )
            .unwrap();
        drop(store);
        let mut store = SqliteMailStore::open(&path.path).unwrap();
        assert_eq!(store.list_contacts().unwrap(), original);
        assert_eq!(store.list_dav_conflicts(None).unwrap(), Vec::new());
        store
            .preserve_dav_conflict(
                &collection,
                &original[0].id,
                DavItemKind::Contact,
                "/collection/item",
                "item",
            )
            .unwrap();
        let conflicts = store.list_dav_conflicts(None).unwrap();
        drop(store);
        let store = SqliteMailStore::open(&path.path).unwrap();
        assert_eq!(store.list_dav_conflicts(None).unwrap(), conflicts);
        assert_eq!(store.list_contacts().unwrap().len(), 2);
        let connection = store.connection().unwrap();
        assert_eq!(
            connection
                .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
                .unwrap(),
            19
        );
        assert_eq!(
            connection
                .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
        assert!(
            !connection
                .prepare("PRAGMA foreign_key_check")
                .unwrap()
                .exists([])
                .unwrap()
        );
    }
}
