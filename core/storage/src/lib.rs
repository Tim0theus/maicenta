//! SQLite-backed local persistence for MAICENTA.

use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::Read,
    ops::{Deref, DerefMut},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, OnceLock, Weak},
    time::Duration,
};

use maicenta_application::{
    ApplicationError, LocalDraftMetadata, LocalDraftStore, MailAccountStore, MailStore,
    MailSyncStore, PendingDraftAction, PendingDraftOperation, PendingMailMutation,
    RemoteMailboxSyncState, RemoteMessageIdentity, RemoteMessageMetadata, SecretStore,
    WorkspaceStore,
};
use maicenta_domain::{
    AccountId, AttachmentId, CalendarEvent, Contact, MailAccount, MailAddress, MailProvider,
    Mailbox, MailboxId, MailboxRole, MessageAttachment, MessageBody, MessageFlag, MessageId,
    MessageRecipients, MessageSummary, TaskItem, TransportSecurity, WorkspaceItemId,
};
use rusqlite::{Connection, OptionalExtension, params};

const CURRENT_SCHEMA_VERSION: u32 = 15;
const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";
const FAVORITE_MAILBOXES_KEY: &str = "favorite_mailbox_ids";
const DARK_MODE_KEY: &str = "dark_mode_enabled";

static DATABASE_OPERATION_LOCKS: OnceLock<Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>> =
    OnceLock::new();

/// Thread-safe `SQLite` implementation of [`MailStore`].
pub struct SqliteMailStore {
    connection: Mutex<Connection>,
    operation_lock: Arc<Mutex<()>>,
}

struct StoreConnectionGuard<'a> {
    _operation_guard: MutexGuard<'a, ()>,
    connection_guard: MutexGuard<'a, Connection>,
}

impl Deref for StoreConnectionGuard<'_> {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        &self.connection_guard
    }
}

impl DerefMut for StoreConnectionGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.connection_guard
    }
}

impl SqliteMailStore {
    /// Opens or creates a database and applies all pending migrations.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the database cannot be opened, configured,
    /// or migrated.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ApplicationError> {
        let path = path.as_ref();
        let operation_lock = database_operation_lock(path);
        let _operation_guard = lock_operation(&operation_lock)?;
        let connection = Connection::open(path).map_err(storage_error)?;
        Self::from_connection_with_lock(connection, operation_lock.clone())
    }

    /// Opens an encrypted profile database, migrating a legacy plaintext
    /// database atomically before the first encrypted use.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the key is wrong, migration fails, or the
    /// encrypted database cannot be configured.
    pub fn open_encrypted(
        path: impl AsRef<Path>,
        key: &[u8; 32],
    ) -> Result<Self, ApplicationError> {
        let path = path.as_ref();
        let operation_lock = database_operation_lock(path);
        let _operation_guard = lock_operation(&operation_lock)?;
        migrate_plaintext_database(path, key)?;
        let connection = open_keyed_connection(path, key)?;
        restrict_database_permissions(path)?;
        Self::from_connection_with_lock(connection, operation_lock.clone())
    }

    /// Creates an initialized in-memory database.
    ///
    /// # Errors
    ///
    /// Returns a storage error when configuration or migration fails.
    pub fn open_in_memory() -> Result<Self, ApplicationError> {
        let connection = Connection::open_in_memory().map_err(storage_error)?;
        Self::from_connection(connection)
    }

    fn from_connection(connection: Connection) -> Result<Self, ApplicationError> {
        Self::from_connection_with_lock(connection, Arc::new(Mutex::new(())))
    }

    fn from_connection_with_lock(
        mut connection: Connection,
        operation_lock: Arc<Mutex<()>>,
    ) -> Result<Self, ApplicationError> {
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(storage_error)?;
        connection
            .execute_batch(
                "
                PRAGMA foreign_keys = ON;
                PRAGMA synchronous = NORMAL;
                ",
            )
            .map_err(storage_error)?;
        let journal_mode: String = connection
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .map_err(storage_error)?;
        if !matches!(journal_mode.as_str(), "wal" | "memory") {
            connection
                .execute_batch("PRAGMA journal_mode = WAL;")
                .map_err(storage_error)?;
        }
        migrate(&mut connection)?;

        Ok(Self {
            connection: Mutex::new(connection),
            operation_lock,
        })
    }

    /// Returns the schema version currently stored by `SQLite`.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the database cannot be read.
    pub fn schema_version(&self) -> Result<u32, ApplicationError> {
        self.connection()?
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(storage_error)
    }

    /// Folds the encrypted WAL into the main database before a portable copy.
    ///
    /// # Errors
    ///
    /// Returns a storage error when `SQLite` cannot checkpoint the profile.
    pub fn checkpoint(&self) -> Result<(), ApplicationError> {
        self.connection()?
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(storage_error)
    }

    /// Authenticates every encrypted database page and verifies the logical
    /// `SQLite` structure.
    ///
    /// # Errors
    ///
    /// Returns a storage error when ciphertext authentication or the database
    /// structure check reports corruption.
    pub fn integrity_check(&self) -> Result<(), ApplicationError> {
        let connection = self.connection()?;
        let mut cipher_statement = connection
            .prepare("PRAGMA cipher_integrity_check")
            .map_err(storage_error)?;
        let cipher_errors = cipher_statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        if !cipher_errors.is_empty() {
            return Err(ApplicationError::Storage(format!(
                "encrypted profile integrity check failed: {}",
                cipher_errors.join("; ")
            )));
        }
        let logical_result: String = connection
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .map_err(storage_error)?;
        if logical_result != "ok" {
            return Err(ApplicationError::Storage(format!(
                "profile database integrity check failed: {logical_result}"
            )));
        }
        Ok(())
    }

    fn connection(&self) -> Result<StoreConnectionGuard<'_>, ApplicationError> {
        let operation_guard = lock_operation(&self.operation_lock)?;
        let connection_guard = self.connection.lock().map_err(|_| {
            ApplicationError::Storage("database connection lock was poisoned".into())
        })?;
        Ok(StoreConnectionGuard {
            _operation_guard: operation_guard,
            connection_guard,
        })
    }
}

fn lock_operation(lock: &Mutex<()>) -> Result<MutexGuard<'_, ()>, ApplicationError> {
    lock.lock()
        .map_err(|_| ApplicationError::Storage("database operation lock was poisoned".into()))
}

fn database_operation_lock(path: &Path) -> Arc<Mutex<()>> {
    let key = fs::canonicalize(path).unwrap_or_else(|_| {
        path.parent()
            .and_then(|parent| fs::canonicalize(parent).ok())
            .and_then(|parent| path.file_name().map(|name| parent.join(name)))
            .unwrap_or_else(|| path.to_path_buf())
    });
    let registry = DATABASE_OPERATION_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut locks = registry
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
        return lock;
    }
    locks.retain(|_, lock| lock.strong_count() > 0);
    let lock = Arc::new(Mutex::new(()));
    locks.insert(key, Arc::downgrade(&lock));
    lock
}

impl MailStore for SqliteMailStore {
    fn list_mailboxes(&self, account_id: &AccountId) -> Result<Vec<Mailbox>, ApplicationError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "
                SELECT id, account_id, display_name, remote_name, role, unread_count,
                       total_count
                FROM mailboxes
                WHERE account_id = ?1
                ORDER BY
                    CASE role
                        WHEN 'inbox' THEN 0
                        WHEN 'drafts' THEN 1
                        WHEN 'sent' THEN 2
                        WHEN 'archive' THEN 3
                        WHEN 'trash' THEN 4
                        WHEN 'junk' THEN 5
                        ELSE 6
                    END,
                    display_name COLLATE NOCASE
                ",
            )
            .map_err(storage_error)?;

        let rows = statement
            .query_map([account_id.as_str()], |row| {
                Ok(RawMailbox {
                    id: row.get(0)?,
                    account_id: row.get(1)?,
                    display_name: row.get(2)?,
                    remote_name: row.get(3)?,
                    role: row.get(4)?,
                    unread_count: row.get(5)?,
                    total_count: row.get(6)?,
                })
            })
            .map_err(storage_error)?;

        rows.map(|row| row.map_err(storage_error).and_then(RawMailbox::into_domain))
            .collect()
    }

    fn list_messages(
        &self,
        mailbox_id: &MailboxId,
        limit: usize,
    ) -> Result<Vec<MessageSummary>, ApplicationError> {
        self.list_message_page(mailbox_id, 0, limit)
    }

    fn list_message_page(
        &self,
        mailbox_id: &MailboxId,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<MessageSummary>, ApplicationError> {
        let offset =
            i64::try_from(offset).map_err(|error| ApplicationError::Storage(error.to_string()))?;
        let limit =
            i64::try_from(limit).map_err(|error| ApplicationError::Storage(error.to_string()))?;
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "
                SELECT
                    id,
                    account_id,
                    mailbox_id,
                    from_address,
                    from_display_name,
                    subject,
                    preview,
                    received_at_ms,
                    flags,
                    has_attachments
                FROM messages
                WHERE mailbox_id = ?1
                ORDER BY received_at_ms DESC, id
                LIMIT ?2
                OFFSET ?3
                ",
            )
            .map_err(storage_error)?;

        let rows = statement
            .query_map(params![mailbox_id.as_str(), limit, offset], |row| {
                Ok(RawMessageSummary {
                    id: row.get(0)?,
                    account_id: row.get(1)?,
                    mailbox_id: row.get(2)?,
                    from_address: row.get(3)?,
                    from_display_name: row.get(4)?,
                    subject: row.get(5)?,
                    preview: row.get(6)?,
                    received_at_ms: row.get(7)?,
                    flags: row.get(8)?,
                    has_attachments: row.get(9)?,
                })
            })
            .map_err(storage_error)?;

        rows.map(|row| {
            row.map_err(storage_error)
                .and_then(RawMessageSummary::into_domain)
        })
        .collect()
    }

    fn message_summary(&self, message_id: &MessageId) -> Result<MessageSummary, ApplicationError> {
        let connection = self.connection()?;
        let raw = connection
            .query_row(
                "SELECT id, account_id, mailbox_id, from_address,
                        from_display_name, subject, preview, received_at_ms,
                        flags, has_attachments
                 FROM messages
                 WHERE id = ?1",
                [message_id.as_str()],
                |row| {
                    Ok(RawMessageSummary {
                        id: row.get(0)?,
                        account_id: row.get(1)?,
                        mailbox_id: row.get(2)?,
                        from_address: row.get(3)?,
                        from_display_name: row.get(4)?,
                        subject: row.get(5)?,
                        preview: row.get(6)?,
                        received_at_ms: row.get(7)?,
                        flags: row.get(8)?,
                        has_attachments: row.get(9)?,
                    })
                },
            )
            .optional()
            .map_err(storage_error)?
            .ok_or(ApplicationError::NotFound)?;
        raw.into_domain()
    }

    fn search_messages(
        &self,
        query: &str,
        include_content: bool,
        limit: usize,
    ) -> Result<Vec<MessageSummary>, ApplicationError> {
        let Some(expression) = search_expression(query, include_content)? else {
            return Ok(Vec::new());
        };
        let raw_query = query.trim();
        let limit = limit.min(200);
        let limit =
            i64::try_from(limit).map_err(|error| ApplicationError::Storage(error.to_string()))?;
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "
                SELECT
                    messages.id,
                    messages.account_id,
                    messages.mailbox_id,
                    messages.from_address,
                    messages.from_display_name,
                    messages.subject,
                    messages.preview,
                    messages.received_at_ms,
                    messages.flags,
                    messages.has_attachments
                FROM message_search
                JOIN messages ON messages.id = message_search.message_id
                WHERE message_search MATCH ?1
                ORDER BY
                    CASE
                        WHEN messages.subject = ?3 COLLATE NOCASE THEN 0
                        WHEN messages.subject LIKE (?3 || '%') COLLATE NOCASE THEN 1
                        ELSE 2
                    END,
                    bm25(message_search, 0.0, 12.0, 8.0, 10.0, 8.0, 4.0, 2.0, 1.0),
                    messages.received_at_ms DESC
                LIMIT ?2
                ",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map(params![expression, limit, raw_query], |row| {
                Ok(RawMessageSummary {
                    id: row.get(0)?,
                    account_id: row.get(1)?,
                    mailbox_id: row.get(2)?,
                    from_address: row.get(3)?,
                    from_display_name: row.get(4)?,
                    subject: row.get(5)?,
                    preview: row.get(6)?,
                    received_at_ms: row.get(7)?,
                    flags: row.get(8)?,
                    has_attachments: row.get(9)?,
                })
            })
            .map_err(storage_error)?;
        rows.map(|row| {
            row.map_err(storage_error)
                .and_then(RawMessageSummary::into_domain)
        })
        .collect()
    }

    fn message_body(&self, message_id: &MessageId) -> Result<MessageBody, ApplicationError> {
        let connection = self.connection()?;
        let raw = connection
            .query_row(
                "
                SELECT message_id, plain_text, sanitized_html
                FROM message_bodies
                WHERE message_id = ?1
                ",
                [message_id.as_str()],
                |row| {
                    Ok(RawMessageBody {
                        message_id: row.get(0)?,
                        plain_text: row.get(1)?,
                        sanitized_html: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(storage_error)?
            .ok_or(ApplicationError::NotFound)?;

        raw.into_domain()
    }

    fn message_recipients(
        &self,
        message_id: &MessageId,
    ) -> Result<MessageRecipients, ApplicationError> {
        Ok(self
            .connection()?
            .query_row(
                "SELECT to_recipients, cc_recipients, bcc_recipients
                 FROM message_recipients
                 WHERE message_id = ?1",
                [message_id.as_str()],
                |row| {
                    Ok(MessageRecipients {
                        message_id: message_id.clone(),
                        to: row.get(0)?,
                        cc: row.get(1)?,
                        bcc: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(storage_error)?
            .unwrap_or_else(|| MessageRecipients {
                message_id: message_id.clone(),
                to: String::new(),
                cc: String::new(),
                bcc: String::new(),
            }))
    }

    fn list_attachments(
        &self,
        message_id: &MessageId,
    ) -> Result<Vec<MessageAttachment>, ApplicationError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, message_id, file_name, content_type, size_bytes, object_key,
                        remote_section, transfer_encoding
                 FROM message_attachments
                 WHERE message_id = ?1
                 ORDER BY id",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([message_id.as_str()], raw_attachment)
            .map_err(storage_error)?;
        rows.map(|row| row.map_err(storage_error).and_then(attachment_from_raw))
            .collect()
    }

    fn attachment(
        &self,
        attachment_id: &AttachmentId,
    ) -> Result<MessageAttachment, ApplicationError> {
        let connection = self.connection()?;
        let raw = connection
            .query_row(
                "SELECT id, message_id, file_name, content_type, size_bytes, object_key,
                        remote_section, transfer_encoding
                 FROM message_attachments WHERE id = ?1",
                [attachment_id.as_str()],
                raw_attachment,
            )
            .optional()
            .map_err(storage_error)?
            .ok_or(ApplicationError::NotFound)?;
        attachment_from_raw(raw)
    }

    fn save_mailboxes(&mut self, mailboxes: &[Mailbox]) -> Result<(), ApplicationError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(storage_error)?;

        for mailbox in mailboxes {
            transaction
                .execute(
                    "
                    INSERT INTO mailboxes (
                        id, account_id, display_name, remote_name, role, unread_count,
                        total_count
                    )
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                    ON CONFLICT(id) DO UPDATE SET
                        account_id = excluded.account_id,
                        display_name = excluded.display_name,
                        remote_name = excluded.remote_name,
                        role = excluded.role,
                        unread_count = excluded.unread_count,
                        total_count = excluded.total_count
                    ",
                    params![
                        mailbox.id.as_str(),
                        mailbox.account_id.as_str(),
                        mailbox.display_name,
                        mailbox.remote_name,
                        mailbox_role_to_str(mailbox.role),
                        mailbox.unread_count,
                        mailbox.total_count,
                    ],
                )
                .map_err(storage_error)?;
        }

        transaction.commit().map_err(storage_error)
    }

    fn save_summaries(&mut self, messages: &[MessageSummary]) -> Result<(), ApplicationError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(storage_error)?;

        for message in messages {
            save_summary(&transaction, message)?;
        }

        refresh_mailbox_counts(&transaction)?;

        transaction.commit().map_err(storage_error)
    }

    fn save_body(&mut self, body: &MessageBody) -> Result<(), ApplicationError> {
        let connection = self.connection()?;
        save_message_body(&connection, body)
    }

    fn save_message(
        &mut self,
        summary: &MessageSummary,
        body: &MessageBody,
    ) -> Result<(), ApplicationError> {
        if summary.id != body.message_id {
            return Err(ApplicationError::Storage(
                "message summary and body identifiers differ".into(),
            ));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(storage_error)?;
        save_summary(&transaction, summary)?;
        save_message_body(&transaction, body)?;
        refresh_mailbox_counts(&transaction)?;
        transaction.commit().map_err(storage_error)
    }

    fn save_message_with_attachments(
        &mut self,
        summary: &MessageSummary,
        body: &MessageBody,
        attachments: &[MessageAttachment],
    ) -> Result<(), ApplicationError> {
        if summary.id != body.message_id
            || attachments
                .iter()
                .any(|attachment| attachment.message_id != summary.id)
        {
            return Err(ApplicationError::Storage(
                "message and attachment identifiers differ".into(),
            ));
        }
        if summary.has_attachments == attachments.is_empty() {
            return Err(ApplicationError::Storage(
                "message attachment flag does not match metadata".into(),
            ));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(storage_error)?;
        save_summary(&transaction, summary)?;
        save_message_body(&transaction, body)?;
        transaction
            .execute(
                "DELETE FROM message_attachments WHERE message_id = ?1",
                [summary.id.as_str()],
            )
            .map_err(storage_error)?;
        for attachment in attachments {
            save_attachment(&transaction, attachment)?;
        }
        refresh_mailbox_counts(&transaction)?;
        transaction.commit().map_err(storage_error)
    }

    fn update_message_state(
        &mut self,
        message_id: &MessageId,
        mailbox_id: &MailboxId,
        unread: bool,
        flagged: bool,
    ) -> Result<u32, ApplicationError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(storage_error)?;
        let message: Option<(u32, String)> = transaction
            .query_row(
                "SELECT flags, account_id FROM messages WHERE id = ?1",
                [message_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(storage_error)?;
        let (encoded, message_account_id) = message.ok_or(ApplicationError::NotFound)?;
        let target_mailbox: Option<(String, String)> = transaction
            .query_row(
                "SELECT account_id, COALESCE(remote_name, display_name)
                 FROM mailboxes WHERE id = ?1",
                [mailbox_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(storage_error)?;
        let (target_account_id, target_remote_name) =
            target_mailbox.ok_or(ApplicationError::NotFound)?;
        if target_account_id != message_account_id {
            return Err(ApplicationError::Storage(
                "cannot move a message into another account".into(),
            ));
        }

        let mut flags = decode_flags(encoded);
        flags.retain(|flag| !matches!(flag, MessageFlag::Seen | MessageFlag::Flagged));
        if !unread {
            flags.push(MessageFlag::Seen);
        }
        if flagged {
            flags.push(MessageFlag::Flagged);
        }

        transaction
            .execute(
                "UPDATE messages SET mailbox_id = ?1, flags = ?2 WHERE id = ?3",
                params![
                    mailbox_id.as_str(),
                    encode_flags(&flags),
                    message_id.as_str()
                ],
            )
            .map_err(storage_error)?;

        let remote = query_remote_identity(&transaction, message_id)?;
        if let Some((source_mailbox, identity)) = remote {
            let target_mailbox =
                (target_remote_name != source_mailbox).then_some(target_remote_name);
            let (uid_validity, remote_uid, provider_id) = identity_columns(&identity);
            transaction
                .execute(
                    "INSERT INTO pending_mail_mutations (
                        message_id, account_id, source_mailbox, target_mailbox,
                        uid_validity, remote_uid, remote_id, seen, flagged, updated_at_ms
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                        CAST(strftime('%s', 'now') AS INTEGER) * 1000)
                     ON CONFLICT(message_id) DO UPDATE SET
                        account_id = excluded.account_id,
                        target_mailbox = excluded.target_mailbox,
                        seen = excluded.seen,
                        flagged = excluded.flagged,
                        updated_at_ms = excluded.updated_at_ms",
                    params![
                        message_id.as_str(),
                        message_account_id,
                        source_mailbox,
                        target_mailbox,
                        uid_validity,
                        remote_uid,
                        provider_id,
                        !unread,
                        flagged,
                    ],
                )
                .map_err(storage_error)?;
        }
        refresh_mailbox_counts(&transaction)?;
        let pending_count = transaction
            .query_row("SELECT COUNT(*) FROM pending_mail_mutations", [], |row| {
                row.get::<_, u32>(0)
            })
            .map_err(storage_error)?;
        transaction.commit().map_err(storage_error)?;
        Ok(pending_count)
    }

    fn rename_mailbox(
        &mut self,
        mailbox_id: &MailboxId,
        display_name: &str,
    ) -> Result<(), ApplicationError> {
        let changed = self
            .connection()?
            .execute(
                "UPDATE mailboxes SET display_name = ?1 WHERE id = ?2",
                params![display_name, mailbox_id.as_str()],
            )
            .map_err(storage_error)?;
        if changed == 0 {
            Err(ApplicationError::NotFound)
        } else {
            Ok(())
        }
    }

    fn delete_mailbox(
        &mut self,
        mailbox_id: &MailboxId,
        fallback_mailbox_id: &MailboxId,
    ) -> Result<(), ApplicationError> {
        if mailbox_id == fallback_mailbox_id {
            return Err(ApplicationError::Storage(
                "cannot delete a mailbox into itself".into(),
            ));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(storage_error)?;
        let fallback_exists: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM mailboxes WHERE id = ?1)",
                [fallback_mailbox_id.as_str()],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if !fallback_exists {
            return Err(ApplicationError::NotFound);
        }
        transaction
            .execute(
                "UPDATE messages SET mailbox_id = ?1 WHERE mailbox_id = ?2",
                params![fallback_mailbox_id.as_str(), mailbox_id.as_str()],
            )
            .map_err(storage_error)?;
        let deleted = transaction
            .execute("DELETE FROM mailboxes WHERE id = ?1", [mailbox_id.as_str()])
            .map_err(storage_error)?;
        if deleted == 0 {
            return Err(ApplicationError::NotFound);
        }
        refresh_mailbox_counts(&transaction)?;
        transaction.commit().map_err(storage_error)
    }
}

impl LocalDraftStore for SqliteMailStore {
    fn local_draft_metadata(
        &self,
        message_id: &MessageId,
    ) -> Result<Option<LocalDraftMetadata>, ApplicationError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT to_recipients, cc_recipients, bcc_recipients, editor_delta_json
                 FROM local_drafts
                 WHERE message_id = ?1",
                [message_id.as_str()],
                |row| {
                    Ok(LocalDraftMetadata {
                        message_id: message_id.clone(),
                        to: row.get(0)?,
                        cc: row.get(1)?,
                        bcc: row.get(2)?,
                        editor_delta_json: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(storage_error)
    }

    fn save_local_message(
        &mut self,
        summary: &MessageSummary,
        body: &MessageBody,
        recipients: &MessageRecipients,
        attachments: &[MessageAttachment],
        draft: Option<&LocalDraftMetadata>,
    ) -> Result<(), ApplicationError> {
        validate_local_message(summary, body, recipients, attachments, draft)?;

        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(storage_error)?;
        let draft_context = draft_operation_context(&transaction, summary)?;
        save_summary(&transaction, summary)?;
        save_message_body(&transaction, body)?;
        save_message_recipients(&transaction, recipients)?;
        transaction
            .execute(
                "DELETE FROM message_attachments WHERE message_id = ?1",
                [summary.id.as_str()],
            )
            .map_err(storage_error)?;
        for attachment in attachments {
            save_attachment(&transaction, attachment)?;
        }
        save_local_draft_metadata(&transaction, &summary.id, draft)?;
        queue_local_draft_operation(&transaction, summary, draft, &draft_context)?;
        refresh_mailbox_counts(&transaction)?;
        transaction.commit().map_err(storage_error)
    }

    fn save_synchronized_draft_metadata(
        &mut self,
        draft: &LocalDraftMetadata,
    ) -> Result<(), ApplicationError> {
        let connection = self.connection()?;
        let is_remote_draft: bool = connection
            .query_row(
                "SELECT EXISTS(
                    SELECT 1
                    FROM messages
                    JOIN remote_messages ON remote_messages.message_id = messages.id
                    WHERE messages.id = ?1 AND (messages.flags & ?2) != 0
                 )",
                params![draft.message_id.as_str(), flag_bit(MessageFlag::Draft)],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if !is_remote_draft {
            return Err(ApplicationError::Storage(
                "editable server draft metadata requires a cached remote draft".into(),
            ));
        }
        connection
            .execute(
                "INSERT INTO local_drafts (
                    message_id, to_recipients, cc_recipients, bcc_recipients,
                    editor_delta_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(message_id) DO UPDATE SET
                    to_recipients = excluded.to_recipients,
                    cc_recipients = excluded.cc_recipients,
                    bcc_recipients = excluded.bcc_recipients,
                    editor_delta_json = excluded.editor_delta_json",
                params![
                    draft.message_id.as_str(),
                    draft.to,
                    draft.cc,
                    draft.bcc,
                    draft.editor_delta_json,
                ],
            )
            .map(|_| ())
            .map_err(storage_error)
    }
}

impl MailSyncStore for SqliteMailStore {
    fn remote_mailbox_sync_states(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<RemoteMailboxSyncState>, ApplicationError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT account_id, remote_mailbox, uid_validity, uid_next,
                        highest_modseq, delta_cursor, catalog_complete,
                        last_full_reconcile_at_ms
                 FROM remote_mailbox_sync_states
                 WHERE account_id = ?1
                 ORDER BY remote_mailbox",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([account_id.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u32>(2)?,
                    row.get::<_, Option<u32>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, bool>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            })
            .map_err(storage_error)?;
        rows.map(|row| {
            let raw = row.map_err(storage_error)?;
            Ok(RemoteMailboxSyncState {
                account_id: AccountId::parse(raw.0).map_err(invalid_data)?,
                remote_mailbox: raw.1,
                uid_validity: raw.2,
                uid_next: raw.3,
                highest_modseq: raw
                    .4
                    .map(|value| value.parse::<u64>().map_err(invalid_data))
                    .transpose()?,
                delta_cursor: raw.5,
                catalog_complete: raw.6,
                last_full_reconcile_at_ms: raw.7,
            })
        })
        .collect()
    }

    fn save_remote_mailbox_sync_state(
        &mut self,
        state: &RemoteMailboxSyncState,
    ) -> Result<(), ApplicationError> {
        if state.remote_mailbox.is_empty()
            || state.uid_next == Some(0)
            || state.last_full_reconcile_at_ms < 0
            || state
                .delta_cursor
                .as_ref()
                .is_some_and(|cursor| cursor.is_empty() || cursor.len() > 8_192)
        {
            return Err(ApplicationError::Storage(
                "remote mailbox synchronization state is invalid".into(),
            ));
        }
        self.connection()?
            .execute(
                "INSERT INTO remote_mailbox_sync_states (
                    account_id, remote_mailbox, uid_validity, uid_next,
                    highest_modseq, delta_cursor, catalog_complete,
                    last_full_reconcile_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(account_id, remote_mailbox) DO UPDATE SET
                    uid_validity = excluded.uid_validity,
                    uid_next = excluded.uid_next,
                    highest_modseq = excluded.highest_modseq,
                    delta_cursor = excluded.delta_cursor,
                    catalog_complete = excluded.catalog_complete,
                    last_full_reconcile_at_ms = excluded.last_full_reconcile_at_ms",
                params![
                    state.account_id.as_str(),
                    state.remote_mailbox,
                    state.uid_validity,
                    state.uid_next,
                    state.highest_modseq.map(|value| value.to_string()),
                    state.delta_cursor,
                    state.catalog_complete,
                    state.last_full_reconcile_at_ms,
                ],
            )
            .map(|_| ())
            .map_err(storage_error)
    }

    fn remote_messages_for_account(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<RemoteMessageMetadata>, ApplicationError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT message_id, account_id, remote_mailbox, uid_validity, remote_uid,
                        remote_id, catalog_complete, body_requested, body_complete
                 FROM remote_messages
                 WHERE account_id = ?1
                 ORDER BY remote_mailbox, uid_validity, remote_uid, remote_id",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([account_id.as_str()], remote_metadata_row)
            .map_err(storage_error)?;
        rows.map(|row| {
            row.map_err(storage_error)
                .and_then(RawRemoteMetadata::into_domain)
        })
        .collect()
    }

    fn remote_message_metadata(
        &self,
        message_id: &MessageId,
    ) -> Result<RemoteMessageMetadata, ApplicationError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT message_id, account_id, remote_mailbox, uid_validity, remote_uid,
                        remote_id, catalog_complete, body_requested, body_complete
                 FROM remote_messages WHERE message_id = ?1",
                [message_id.as_str()],
                remote_metadata_row,
            )
            .optional()
            .map_err(storage_error)?
            .ok_or(ApplicationError::NotFound)?
            .into_domain()
    }

    fn save_remote_message(
        &mut self,
        summary: &MessageSummary,
        body: &MessageBody,
        recipients: &MessageRecipients,
        metadata: &RemoteMessageMetadata,
        attachments: &[MessageAttachment],
    ) -> Result<(), ApplicationError> {
        if summary.id != body.message_id
            || summary.id != recipients.message_id
            || summary.id != metadata.message_id
            || summary.account_id != metadata.account_id
            || attachments
                .iter()
                .any(|attachment| attachment.message_id != summary.id)
        {
            return Err(ApplicationError::Storage(
                "remote message identifiers differ".into(),
            ));
        }
        if metadata.remote_mailbox.is_empty() || !metadata.identity.is_complete() {
            return Err(ApplicationError::Storage(
                "remote message identity is incomplete".into(),
            ));
        }
        if !summary.has_attachments && !attachments.is_empty() {
            return Err(ApplicationError::Storage(
                "remote attachment metadata contradicts the message summary".into(),
            ));
        }

        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(storage_error)?;
        if metadata.body_requested {
            save_summary(&transaction, summary)?;
            save_message_body(&transaction, body)?;
        } else {
            let existing_preview = transaction
                .query_row(
                    "SELECT preview FROM messages WHERE id = ?1",
                    [summary.id.as_str()],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(storage_error)?;
            let mut catalog_summary = summary.clone();
            if let Some(existing_preview) = existing_preview {
                catalog_summary.preview = existing_preview;
            }
            save_summary(&transaction, &catalog_summary)?;
            transaction
                .execute(
                    "INSERT INTO message_bodies (message_id, plain_text, sanitized_html)
                     VALUES (?1, '', NULL)
                     ON CONFLICT(message_id) DO NOTHING",
                    [summary.id.as_str()],
                )
                .map_err(storage_error)?;
        }
        save_message_recipients(&transaction, recipients)?;
        let (uid_validity, remote_uid, provider_id) = identity_columns(&metadata.identity);
        transaction
            .execute(
                "INSERT INTO remote_messages (
                    message_id, account_id, remote_mailbox, uid_validity, remote_uid,
                    remote_id, catalog_complete, body_requested, body_complete
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(message_id) DO UPDATE SET
                    account_id = excluded.account_id,
                    remote_mailbox = excluded.remote_mailbox,
                    uid_validity = excluded.uid_validity,
                    remote_uid = excluded.remote_uid,
                    remote_id = excluded.remote_id,
                    catalog_complete = excluded.catalog_complete,
                    body_requested = CASE
                        WHEN excluded.body_requested THEN 1
                        ELSE remote_messages.body_requested
                    END,
                    body_complete = CASE
                        WHEN excluded.body_requested THEN excluded.body_complete
                        ELSE remote_messages.body_complete
                    END",
                params![
                    metadata.message_id.as_str(),
                    metadata.account_id.as_str(),
                    metadata.remote_mailbox,
                    uid_validity,
                    remote_uid,
                    provider_id,
                    metadata.catalog_complete,
                    metadata.body_requested,
                    metadata.body_complete,
                ],
            )
            .map_err(storage_error)?;
        transaction
            .execute(
                "DELETE FROM message_attachments WHERE message_id = ?1",
                [summary.id.as_str()],
            )
            .map_err(storage_error)?;
        for attachment in attachments {
            save_attachment(&transaction, attachment)?;
        }
        refresh_mailbox_counts(&transaction)?;
        transaction.commit().map_err(storage_error)
    }

    fn update_remote_message_flags(
        &mut self,
        message_id: &MessageId,
        flags: &[MessageFlag],
    ) -> Result<(), ApplicationError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(storage_error)?;
        let updated = transaction
            .execute(
                "UPDATE messages
                 SET flags = ?1
                 WHERE id = ?2
                   AND EXISTS (
                       SELECT 1 FROM remote_messages
                       WHERE remote_messages.message_id = messages.id
                   )",
                params![encode_flags(flags), message_id.as_str()],
            )
            .map_err(storage_error)?;
        if updated == 0 {
            return Err(ApplicationError::NotFound);
        }
        refresh_mailbox_counts(&transaction)?;
        transaction.commit().map_err(storage_error)
    }

    fn reconcile_remote_mailbox(
        &mut self,
        account_id: &AccountId,
        remote_mailbox: &str,
        active_identities: &[RemoteMessageIdentity],
    ) -> Result<Vec<MessageAttachment>, ApplicationError> {
        if remote_mailbox.is_empty()
            || active_identities
                .iter()
                .any(|identity| !identity.is_complete())
        {
            return Err(ApplicationError::Storage(
                "remote mailbox snapshot is invalid".into(),
            ));
        }
        let active_identities = active_identities.iter().collect::<HashSet<_>>();
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(storage_error)?;
        let stale_message_ids =
            mailbox_remote_identities(&transaction, account_id, remote_mailbox)?
                .into_iter()
                .filter(|(_, identity)| !active_identities.contains(identity))
                .map(|(message_id, _)| message_id)
                .collect::<Vec<_>>();

        let removed_attachments = delete_remote_message_ids(&transaction, &stale_message_ids)?;
        refresh_mailbox_counts(&transaction)?;
        transaction.commit().map_err(storage_error)?;
        Ok(removed_attachments)
    }

    fn remove_vanished_remote_messages(
        &mut self,
        account_id: &AccountId,
        remote_mailbox: &str,
        vanished_identities: &[RemoteMessageIdentity],
    ) -> Result<Vec<MessageAttachment>, ApplicationError> {
        if remote_mailbox.is_empty()
            || vanished_identities
                .iter()
                .any(|identity| !identity.is_complete())
        {
            return Err(ApplicationError::Storage(
                "vanished remote identity set is invalid".into(),
            ));
        }
        if vanished_identities.is_empty() {
            return Ok(Vec::new());
        }
        let vanished_identities = vanished_identities.iter().collect::<HashSet<_>>();
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(storage_error)?;
        let vanished_message_ids =
            mailbox_remote_identities(&transaction, account_id, remote_mailbox)?
                .into_iter()
                .filter(|(_, identity)| vanished_identities.contains(identity))
                .map(|(message_id, _)| message_id)
                .collect::<Vec<_>>();
        let removed_attachments = delete_remote_message_ids(&transaction, &vanished_message_ids)?;
        refresh_mailbox_counts(&transaction)?;
        transaction.commit().map_err(storage_error)?;
        Ok(removed_attachments)
    }

    fn pending_mail_mutations(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<PendingMailMutation>, ApplicationError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT message_id, account_id, source_mailbox, target_mailbox,
                        uid_validity, remote_uid, remote_id, seen, flagged
                 FROM pending_mail_mutations
                 WHERE account_id = ?1
                 ORDER BY updated_at_ms, message_id",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([account_id.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<u32>>(4)?,
                    row.get::<_, Option<u32>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, bool>(7)?,
                    row.get::<_, bool>(8)?,
                ))
            })
            .map_err(storage_error)?;

        rows.map(|row| {
            let (
                message_id,
                account_id,
                source_mailbox,
                target_mailbox,
                uid_validity,
                remote_uid,
                provider_id,
                seen,
                flagged,
            ) = row.map_err(storage_error)?;
            Ok(PendingMailMutation {
                message_id: MessageId::parse(message_id).map_err(invalid_data)?,
                account_id: AccountId::parse(account_id).map_err(invalid_data)?,
                source_mailbox,
                target_mailbox,
                identity: identity_from_columns(uid_validity, remote_uid, provider_id)?,
                seen,
                flagged,
            })
        })
        .collect()
    }

    fn complete_mail_mutation(
        &mut self,
        message_id: &MessageId,
        remove_local_message: bool,
    ) -> Result<(), ApplicationError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(storage_error)?;
        let pending_exists: bool = transaction
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM pending_mail_mutations WHERE message_id = ?1
                 )",
                [message_id.as_str()],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if !pending_exists {
            return Err(ApplicationError::NotFound);
        }

        if remove_local_message {
            transaction
                .execute("DELETE FROM messages WHERE id = ?1", [message_id.as_str()])
                .map_err(storage_error)?;
        } else {
            transaction
                .execute(
                    "DELETE FROM pending_mail_mutations WHERE message_id = ?1",
                    [message_id.as_str()],
                )
                .map_err(storage_error)?;
        }
        refresh_mailbox_counts(&transaction)?;
        transaction.commit().map_err(storage_error)
    }

    fn pending_draft_operations(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<PendingDraftOperation>, ApplicationError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT message_id, account_id, target_mailbox, operation,
                        previous_remote_mailbox, previous_uid_validity,
                        previous_remote_uid, previous_remote_id
                 FROM pending_draft_operations
                 WHERE account_id = ?1
                 ORDER BY updated_at_ms, message_id",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([account_id.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<u32>>(5)?,
                    row.get::<_, Option<u32>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                ))
            })
            .map_err(storage_error)?;
        rows.map(|row| {
            let (message_id, account_id, target_mailbox, action, mailbox, validity, uid, id) =
                row.map_err(storage_error)?;
            let previous_remote = match (mailbox, validity, uid, id) {
                (Some(remote_mailbox), validity, uid, id)
                    if validity.is_some() || uid.is_some() || id.is_some() =>
                {
                    Some(RemoteMessageMetadata {
                        message_id: MessageId::parse(&message_id).map_err(invalid_data)?,
                        account_id: AccountId::parse(&account_id).map_err(invalid_data)?,
                        remote_mailbox,
                        identity: identity_from_columns(validity, uid, id)?,
                        catalog_complete: true,
                        body_requested: true,
                        body_complete: true,
                    })
                }
                (None, None, None, None) => None,
                _ => {
                    return Err(ApplicationError::Storage(
                        "pending draft has a partial remote identity".into(),
                    ));
                }
            };
            let action = match action.as_str() {
                "upsert" => PendingDraftAction::Upsert,
                "delete" => PendingDraftAction::Delete,
                other => {
                    return Err(ApplicationError::Storage(format!(
                        "unknown pending draft action: {other}"
                    )));
                }
            };
            Ok(PendingDraftOperation {
                message_id: MessageId::parse(message_id).map_err(invalid_data)?,
                account_id: AccountId::parse(account_id).map_err(invalid_data)?,
                target_mailbox,
                action,
                previous_remote,
            })
        })
        .collect()
    }

    fn complete_draft_operation(
        &mut self,
        message_id: &MessageId,
        uploaded_remote: Option<&RemoteMessageMetadata>,
    ) -> Result<(), ApplicationError> {
        if uploaded_remote.is_some_and(|remote| remote.message_id != *message_id) {
            return Err(ApplicationError::Storage(
                "uploaded draft identity belongs to another message".into(),
            ));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(storage_error)?;
        let pending_exists: bool = transaction
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM pending_draft_operations WHERE message_id = ?1
                 )",
                [message_id.as_str()],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if !pending_exists {
            return Err(ApplicationError::NotFound);
        }
        if let Some(remote) = uploaded_remote {
            if remote.remote_mailbox.is_empty() || !remote.identity.is_complete() {
                return Err(ApplicationError::Storage(
                    "uploaded draft identity is incomplete".into(),
                ));
            }
            let (uid_validity, remote_uid, provider_id) = identity_columns(&remote.identity);
            transaction
                .execute(
                    "INSERT INTO remote_messages (
                        message_id, account_id, remote_mailbox, uid_validity,
                        remote_uid, remote_id, catalog_complete, body_requested,
                        body_complete
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, 1, 1)
                     ON CONFLICT(message_id) DO UPDATE SET
                        account_id = excluded.account_id,
                        remote_mailbox = excluded.remote_mailbox,
                        uid_validity = excluded.uid_validity,
                        remote_uid = excluded.remote_uid,
                        remote_id = excluded.remote_id,
                        catalog_complete = 1,
                        body_requested = 1,
                        body_complete = 1",
                    params![
                        message_id.as_str(),
                        remote.account_id.as_str(),
                        remote.remote_mailbox,
                        uid_validity,
                        remote_uid,
                        provider_id,
                    ],
                )
                .map_err(storage_error)?;
        } else {
            transaction
                .execute(
                    "DELETE FROM remote_messages WHERE message_id = ?1",
                    [message_id.as_str()],
                )
                .map_err(storage_error)?;
        }
        transaction
            .execute(
                "DELETE FROM pending_draft_operations WHERE message_id = ?1",
                [message_id.as_str()],
            )
            .map_err(storage_error)?;
        transaction.commit().map_err(storage_error)
    }

    fn pending_mail_mutation_count(&self) -> Result<u32, ApplicationError> {
        self.connection()?
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM pending_mail_mutations) +
                    (SELECT COUNT(*) FROM pending_draft_operations)",
                [],
                |row| row.get(0),
            )
            .map_err(storage_error)
    }
}

impl WorkspaceStore for SqliteMailStore {
    fn dark_mode_enabled(&self) -> Result<bool, ApplicationError> {
        let connection = self.connection()?;
        let value = connection
            .query_row(
                "SELECT value FROM workspace_preferences WHERE key = ?1",
                [DARK_MODE_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(storage_error)?;
        match value.as_deref() {
            None | Some("0") => Ok(false),
            Some("1") => Ok(true),
            Some(_) => Err(ApplicationError::Storage(
                "dark-mode preference contains an invalid value".into(),
            )),
        }
    }

    fn save_dark_mode_enabled(&mut self, enabled: bool) -> Result<(), ApplicationError> {
        self.connection()?
            .execute(
                "INSERT INTO workspace_preferences (key, value)
                 VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![DARK_MODE_KEY, if enabled { "1" } else { "0" }],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    fn favorite_mailbox_ids(&self) -> Result<Option<Vec<MailboxId>>, ApplicationError> {
        let connection = self.connection()?;
        let value = connection
            .query_row(
                "SELECT value FROM workspace_preferences WHERE key = ?1",
                [FAVORITE_MAILBOXES_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(storage_error)?;
        value
            .map(|value| {
                if value.is_empty() {
                    return Ok(Vec::new());
                }
                value
                    .lines()
                    .map(|id| MailboxId::parse(id).map_err(invalid_data))
                    .collect()
            })
            .transpose()
    }

    fn save_favorite_mailbox_ids(
        &mut self,
        mailbox_ids: &[MailboxId],
    ) -> Result<(), ApplicationError> {
        if mailbox_ids.len() > 100 {
            return Err(ApplicationError::Storage(
                "favorite mailbox count exceeds 100".into(),
            ));
        }
        let unique = mailbox_ids
            .iter()
            .map(MailboxId::as_str)
            .collect::<HashSet<_>>();
        if unique.len() != mailbox_ids.len() {
            return Err(ApplicationError::Storage(
                "favorite mailboxes contain a duplicate".into(),
            ));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(storage_error)?;
        for mailbox_id in mailbox_ids {
            let exists: bool = transaction
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM mailboxes WHERE id = ?1)",
                    [mailbox_id.as_str()],
                    |row| row.get(0),
                )
                .map_err(storage_error)?;
            if !exists {
                return Err(ApplicationError::NotFound);
            }
        }
        let value = mailbox_ids
            .iter()
            .map(MailboxId::as_str)
            .collect::<Vec<_>>()
            .join("\n");
        transaction
            .execute(
                "INSERT INTO workspace_preferences (key, value)
                 VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![FAVORITE_MAILBOXES_KEY, value],
            )
            .map_err(storage_error)?;
        transaction.commit().map_err(storage_error)
    }

    fn list_calendar_events(&self) -> Result<Vec<CalendarEvent>, ApplicationError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, title, starts_at_ms, ends_at_ms, location
                 FROM calendar_events
                 ORDER BY starts_at_ms, id",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })
            .map_err(storage_error)?;

        rows.map(|row| {
            let (id, title, starts_at_ms, ends_at_ms, location) = row.map_err(storage_error)?;
            Ok(CalendarEvent {
                id: WorkspaceItemId::parse(id).map_err(invalid_data)?,
                title,
                starts_at_ms,
                ends_at_ms,
                location,
            })
        })
        .collect()
    }

    fn save_calendar_event(&mut self, event: &CalendarEvent) -> Result<(), ApplicationError> {
        if event.ends_at_ms <= event.starts_at_ms {
            return Err(ApplicationError::Storage(
                "calendar event must end after it starts".into(),
            ));
        }
        self.connection()?
            .execute(
                "INSERT INTO calendar_events (id, title, starts_at_ms, ends_at_ms, location)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(id) DO UPDATE SET
                    title = excluded.title,
                    starts_at_ms = excluded.starts_at_ms,
                    ends_at_ms = excluded.ends_at_ms,
                    location = excluded.location",
                params![
                    event.id.as_str(),
                    event.title,
                    event.starts_at_ms,
                    event.ends_at_ms,
                    event.location,
                ],
            )
            .map(|_| ())
            .map_err(storage_error)
    }

    fn list_tasks(&self) -> Result<Vec<TaskItem>, ApplicationError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, title, due_at_ms, completed
                 FROM tasks
                 ORDER BY completed, due_at_ms IS NULL, due_at_ms, id",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, bool>(3)?,
                ))
            })
            .map_err(storage_error)?;

        rows.map(|row| {
            let (id, title, due_at_ms, completed) = row.map_err(storage_error)?;
            Ok(TaskItem {
                id: WorkspaceItemId::parse(id).map_err(invalid_data)?,
                title,
                due_at_ms,
                completed,
            })
        })
        .collect()
    }

    fn save_task(&mut self, task: &TaskItem) -> Result<(), ApplicationError> {
        self.connection()?
            .execute(
                "INSERT INTO tasks (id, title, due_at_ms, completed)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(id) DO UPDATE SET
                    title = excluded.title,
                    due_at_ms = excluded.due_at_ms,
                    completed = excluded.completed",
                params![task.id.as_str(), task.title, task.due_at_ms, task.completed],
            )
            .map(|_| ())
            .map_err(storage_error)
    }

    fn list_contacts(&self) -> Result<Vec<Contact>, ApplicationError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, display_name, email
                 FROM contacts
                 ORDER BY display_name COLLATE NOCASE, id",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(storage_error)?;

        rows.map(|row| {
            let (id, name, email) = row.map_err(storage_error)?;
            Ok(Contact {
                id: WorkspaceItemId::parse(id).map_err(invalid_data)?,
                email: MailAddress::new(email, Some(name.clone())).map_err(invalid_data)?,
                name,
            })
        })
        .collect()
    }

    fn save_contact(&mut self, contact: &Contact) -> Result<(), ApplicationError> {
        self.connection()?
            .execute(
                "INSERT INTO contacts (id, display_name, email)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET
                    display_name = excluded.display_name,
                    email = excluded.email",
                params![contact.id.as_str(), contact.name, contact.email.address()],
            )
            .map(|_| ())
            .map_err(storage_error)
    }
}

impl MailAccountStore for SqliteMailStore {
    fn list_mail_accounts(&self) -> Result<Vec<MailAccount>, ApplicationError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, display_name, email,
                        imap_host, imap_port, imap_security, imap_username,
                        smtp_host, smtp_port, smtp_security, smtp_username,
                        last_sync_at_ms, provider
                 FROM mail_accounts
                 ORDER BY display_name COLLATE NOCASE, id",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(RawMailAccount {
                    id: row.get(0)?,
                    display_name: row.get(1)?,
                    email: row.get(2)?,
                    imap_host: row.get(3)?,
                    imap_port: row.get(4)?,
                    imap_security: row.get(5)?,
                    imap_username: row.get(6)?,
                    smtp_host: row.get(7)?,
                    smtp_port: row.get(8)?,
                    smtp_security: row.get(9)?,
                    smtp_username: row.get(10)?,
                    last_sync_at_ms: row.get(11)?,
                    provider: row.get(12)?,
                })
            })
            .map_err(storage_error)?;

        rows.map(|row| {
            row.map_err(storage_error)
                .and_then(RawMailAccount::into_domain)
        })
        .collect()
    }

    fn save_mail_account(&mut self, account: &MailAccount) -> Result<(), ApplicationError> {
        self.connection()?
            .execute(
                "INSERT INTO mail_accounts (
                    id, display_name, email,
                    imap_host, imap_port, imap_security, imap_username,
                    smtp_host, smtp_port, smtp_security, smtp_username,
                    last_sync_at_ms, provider
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                 ON CONFLICT(id) DO UPDATE SET
                    provider = excluded.provider,
                    display_name = excluded.display_name,
                    email = excluded.email,
                    imap_host = excluded.imap_host,
                    imap_port = excluded.imap_port,
                    imap_security = excluded.imap_security,
                    imap_username = excluded.imap_username,
                    smtp_host = excluded.smtp_host,
                    smtp_port = excluded.smtp_port,
                    smtp_security = excluded.smtp_security,
                    smtp_username = excluded.smtp_username,
                    last_sync_at_ms = excluded.last_sync_at_ms",
                params![
                    account.id.as_str(),
                    account.display_name,
                    account.email.address(),
                    account.imap_host,
                    account.imap_port,
                    transport_security_to_str(account.imap_security),
                    account.imap_username,
                    account.smtp_host,
                    account.smtp_port,
                    transport_security_to_str(account.smtp_security),
                    account.smtp_username,
                    account.last_sync_at_ms,
                    mail_provider_to_str(account.provider),
                ],
            )
            .map(|_| ())
            .map_err(storage_error)
    }

    fn delete_mail_account(&mut self, account_id: &AccountId) -> Result<(), ApplicationError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(storage_error)?;
        let exists: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM mail_accounts WHERE id = ?1)",
                [account_id.as_str()],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if !exists {
            return Err(ApplicationError::NotFound);
        }
        transaction
            .execute(
                "DELETE FROM mailboxes WHERE account_id = ?1",
                [account_id.as_str()],
            )
            .map_err(storage_error)?;
        transaction
            .execute(
                "DELETE FROM mail_accounts WHERE id = ?1",
                [account_id.as_str()],
            )
            .map_err(storage_error)?;
        transaction.commit().map_err(storage_error)
    }

    fn update_account_last_sync(
        &mut self,
        account_id: &AccountId,
        timestamp_ms: i64,
    ) -> Result<(), ApplicationError> {
        let changed = self
            .connection()?
            .execute(
                "UPDATE mail_accounts SET last_sync_at_ms = ?1 WHERE id = ?2",
                params![timestamp_ms, account_id.as_str()],
            )
            .map_err(storage_error)?;
        if changed == 0 {
            Err(ApplicationError::NotFound)
        } else {
            Ok(())
        }
    }
}

impl SecretStore for SqliteMailStore {
    fn get(&self, account_id: &AccountId, key: &str) -> Result<Option<String>, ApplicationError> {
        validate_secret_key(key)?;
        self.connection()?
            .query_row(
                "SELECT value FROM account_secrets WHERE account_id = ?1 AND key = ?2",
                params![account_id.as_str(), key],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)
    }

    fn set(
        &mut self,
        account_id: &AccountId,
        key: &str,
        value: &str,
    ) -> Result<(), ApplicationError> {
        validate_secret_key(key)?;
        if value.is_empty() {
            return Err(ApplicationError::Storage(
                "account secret must not be empty".into(),
            ));
        }
        self.connection()?
            .execute(
                "INSERT INTO account_secrets (account_id, key, value)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(account_id, key) DO UPDATE SET value = excluded.value",
                params![account_id.as_str(), key, value],
            )
            .map(|_| ())
            .map_err(storage_error)
    }

    fn remove(&mut self, account_id: &AccountId, key: &str) -> Result<(), ApplicationError> {
        validate_secret_key(key)?;
        self.connection()?
            .execute(
                "DELETE FROM account_secrets WHERE account_id = ?1 AND key = ?2",
                params![account_id.as_str(), key],
            )
            .map(|_| ())
            .map_err(storage_error)
    }
}

fn validate_secret_key(key: &str) -> Result<(), ApplicationError> {
    if key.is_empty()
        || key.len() > 64
        || !key
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'.' | b'_' | b'-'))
    {
        return Err(ApplicationError::Storage(
            "account secret key is invalid".into(),
        ));
    }
    Ok(())
}

fn open_keyed_connection(path: &Path, key: &[u8; 32]) -> Result<Connection, ApplicationError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(storage_error)?;
    }
    let connection = Connection::open(path).map_err(storage_error)?;
    apply_sqlcipher_key(&connection, key)?;
    connection
        .query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|error| {
            ApplicationError::Storage(format!("encrypted profile could not be unlocked: {error}"))
        })?;
    Ok(connection)
}

fn apply_sqlcipher_key(connection: &Connection, key: &[u8; 32]) -> Result<(), ApplicationError> {
    let encoded = encode_hex(key);
    connection
        .execute_batch(&format!(
            "PRAGMA key = \"x'{encoded}'\";
             PRAGMA cipher_memory_security = ON;"
        ))
        .map_err(storage_error)
}

fn encode_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    bytes.iter().fold(
        String::with_capacity(bytes.len() * 2),
        |mut encoded, byte| {
            let _ = write!(encoded, "{byte:02x}");
            encoded
        },
    )
}

fn migrate_plaintext_database(path: &Path, key: &[u8; 32]) -> Result<(), ApplicationError> {
    if !path.exists() || path.metadata().map_err(storage_error)?.len() == 0 {
        return Ok(());
    }
    let mut header = [0_u8; SQLITE_HEADER.len()];
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_err(storage_error)?;
    if header != *SQLITE_HEADER {
        return Ok(());
    }

    // Consolidate a possible plaintext WAL before exporting into SQLCipher.
    {
        let connection = Connection::open(path).map_err(storage_error)?;
        connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode = DELETE;")
            .map_err(storage_error)?;
    }

    let encrypted_path = migration_sibling(path, "encrypted");
    let backup_path = migration_sibling(path, "plaintext-backup");
    remove_if_exists(&encrypted_path)?;
    remove_if_exists(&backup_path)?;

    let export_result = (|| -> Result<(), ApplicationError> {
        let connection = Connection::open(path).map_err(storage_error)?;
        let encoded = encode_hex(key);
        connection
            .execute(
                &format!("ATTACH DATABASE ?1 AS encrypted KEY \"x'{encoded}'\""),
                [encrypted_path.to_string_lossy().as_ref()],
            )
            .map_err(storage_error)?;
        connection
            .query_row("SELECT sqlcipher_export('encrypted')", [], |_| Ok(()))
            .map_err(storage_error)?;
        let version: u32 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(storage_error)?;
        connection
            .execute_batch(&format!(
                "PRAGMA encrypted.user_version = {version}; DETACH DATABASE encrypted;"
            ))
            .map_err(storage_error)?;
        drop(connection);

        let migrated =
            SqliteMailStore::from_connection(open_keyed_connection(&encrypted_path, key)?)?;
        migrated.integrity_check()?;
        drop(migrated);

        fs::rename(path, &backup_path).map_err(storage_error)?;
        if let Err(error) = fs::rename(&encrypted_path, path) {
            let _ = fs::rename(&backup_path, path);
            return Err(storage_error(error));
        }
        remove_if_exists(&backup_path)?;
        for suffix in ["-wal", "-shm"] {
            remove_if_exists(&path_with_suffix(path, suffix))?;
        }
        Ok(())
    })();
    if export_result.is_err() {
        let _ = remove_if_exists(&encrypted_path);
    }
    export_result
}

fn migration_sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{suffix}"));
    PathBuf::from(name)
}

fn path_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn remove_if_exists(path: &Path) -> Result<(), ApplicationError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(storage_error(error)),
    }
}

fn restrict_database_permissions(path: &Path) -> Result<(), ApplicationError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;

        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(storage_error)?;
    }
    Ok(())
}

fn validate_local_message(
    summary: &MessageSummary,
    body: &MessageBody,
    recipients: &MessageRecipients,
    attachments: &[MessageAttachment],
    draft: Option<&LocalDraftMetadata>,
) -> Result<(), ApplicationError> {
    if summary.id != body.message_id
        || summary.id != recipients.message_id
        || attachments
            .iter()
            .any(|attachment| attachment.message_id != summary.id)
        || draft.is_some_and(|metadata| metadata.message_id != summary.id)
    {
        return Err(ApplicationError::Storage(
            "local message identifiers differ".into(),
        ));
    }
    if summary.has_attachments == attachments.is_empty() {
        return Err(ApplicationError::Storage(
            "message attachment flag does not match metadata".into(),
        ));
    }
    if summary.flags.contains(&MessageFlag::Draft) != draft.is_some() {
        return Err(ApplicationError::Storage(
            "draft flag does not match editable draft metadata".into(),
        ));
    }
    Ok(())
}

struct DraftOperationContext {
    account_is_configured: bool,
    was_editable_draft: bool,
    previous_remote: Option<(String, RemoteMessageIdentity)>,
    target_mailbox: Option<String>,
}

fn draft_operation_context(
    connection: &Connection,
    summary: &MessageSummary,
) -> Result<DraftOperationContext, ApplicationError> {
    let account_is_configured = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM mail_accounts WHERE id = ?1)",
            [summary.account_id.as_str()],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    let was_editable_draft = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM local_drafts WHERE message_id = ?1)",
            [summary.id.as_str()],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    let previous_remote = query_remote_identity(connection, &summary.id)?;
    let target_mailbox = connection
        .query_row(
            "SELECT COALESCE(remote_name, display_name) FROM mailboxes WHERE id = ?1",
            [summary.mailbox_id.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    Ok(DraftOperationContext {
        account_is_configured,
        was_editable_draft,
        previous_remote,
        target_mailbox,
    })
}

fn save_local_draft_metadata(
    connection: &Connection,
    message_id: &MessageId,
    draft: Option<&LocalDraftMetadata>,
) -> Result<(), ApplicationError> {
    if let Some(draft) = draft {
        connection
            .execute(
                "INSERT INTO local_drafts (
                    message_id, to_recipients, cc_recipients, bcc_recipients,
                    editor_delta_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(message_id) DO UPDATE SET
                    to_recipients = excluded.to_recipients,
                    cc_recipients = excluded.cc_recipients,
                    bcc_recipients = excluded.bcc_recipients,
                    editor_delta_json = excluded.editor_delta_json",
                params![
                    draft.message_id.as_str(),
                    draft.to,
                    draft.cc,
                    draft.bcc,
                    draft.editor_delta_json,
                ],
            )
            .map_err(storage_error)?;
    } else {
        connection
            .execute(
                "DELETE FROM local_drafts WHERE message_id = ?1",
                [message_id.as_str()],
            )
            .map_err(storage_error)?;
    }
    Ok(())
}

fn queue_local_draft_operation(
    connection: &Connection,
    summary: &MessageSummary,
    draft: Option<&LocalDraftMetadata>,
    context: &DraftOperationContext,
) -> Result<(), ApplicationError> {
    if !context.account_is_configured {
        return Ok(());
    }
    match (
        draft,
        context.target_mailbox.as_deref(),
        context.previous_remote.as_ref(),
    ) {
        (Some(_), Some(target_mailbox), previous) => {
            let (uid_validity, remote_uid, provider_id) =
                previous.map_or((None, None, None), |remote| identity_columns(&remote.1));
            connection
                .execute(
                    "INSERT INTO pending_draft_operations (
                        message_id, account_id, target_mailbox, operation,
                        previous_remote_mailbox, previous_uid_validity,
                        previous_remote_uid, previous_remote_id, updated_at_ms
                     ) VALUES (?1, ?2, ?3, 'upsert', ?4, ?5, ?6, ?7,
                        CAST(strftime('%s', 'now') AS INTEGER) * 1000)
                     ON CONFLICT(message_id) DO UPDATE SET
                        account_id = excluded.account_id,
                        target_mailbox = excluded.target_mailbox,
                        operation = excluded.operation,
                        previous_remote_mailbox = excluded.previous_remote_mailbox,
                        previous_uid_validity = excluded.previous_uid_validity,
                        previous_remote_uid = excluded.previous_remote_uid,
                        previous_remote_id = excluded.previous_remote_id,
                        updated_at_ms = excluded.updated_at_ms",
                    params![
                        summary.id.as_str(),
                        summary.account_id.as_str(),
                        target_mailbox,
                        previous.map(|remote| remote.0.as_str()),
                        uid_validity,
                        remote_uid,
                        provider_id,
                    ],
                )
                .map_err(storage_error)?;
        }
        (None, _, Some(previous)) if context.was_editable_draft => {
            let (uid_validity, remote_uid, provider_id) = identity_columns(&previous.1);
            connection
                .execute(
                    "INSERT INTO pending_draft_operations (
                        message_id, account_id, target_mailbox, operation,
                        previous_remote_mailbox, previous_uid_validity,
                        previous_remote_uid, previous_remote_id, updated_at_ms
                     ) VALUES (?1, ?2, ?3, 'delete', ?3, ?4, ?5, ?6,
                        CAST(strftime('%s', 'now') AS INTEGER) * 1000)
                     ON CONFLICT(message_id) DO UPDATE SET
                        account_id = excluded.account_id,
                        target_mailbox = excluded.target_mailbox,
                        operation = excluded.operation,
                        previous_remote_mailbox = excluded.previous_remote_mailbox,
                        previous_uid_validity = excluded.previous_uid_validity,
                        previous_remote_uid = excluded.previous_remote_uid,
                        previous_remote_id = excluded.previous_remote_id,
                        updated_at_ms = excluded.updated_at_ms",
                    params![
                        summary.id.as_str(),
                        summary.account_id.as_str(),
                        previous.0.as_str(),
                        uid_validity,
                        remote_uid,
                        provider_id,
                    ],
                )
                .map_err(storage_error)?;
        }
        (None, _, _) if context.was_editable_draft => {
            connection
                .execute(
                    "DELETE FROM pending_draft_operations WHERE message_id = ?1",
                    [summary.id.as_str()],
                )
                .map_err(storage_error)?;
        }
        _ => {}
    }
    Ok(())
}

fn save_summary(connection: &Connection, message: &MessageSummary) -> Result<(), ApplicationError> {
    connection
        .execute(
            "
            INSERT INTO messages (
                id,
                account_id,
                mailbox_id,
                from_address,
                from_display_name,
                subject,
                preview,
                received_at_ms,
                flags,
                has_attachments
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(id) DO UPDATE SET
                account_id = excluded.account_id,
                mailbox_id = excluded.mailbox_id,
                from_address = excluded.from_address,
                from_display_name = excluded.from_display_name,
                subject = excluded.subject,
                preview = excluded.preview,
                received_at_ms = excluded.received_at_ms,
                flags = excluded.flags,
                has_attachments = excluded.has_attachments
            ",
            params![
                message.id.as_str(),
                message.account_id.as_str(),
                message.mailbox_id.as_str(),
                message.from.address(),
                message.from.display_name(),
                message.subject,
                message.preview,
                message.received_at_ms,
                encode_flags(&message.flags),
                message.has_attachments,
            ],
        )
        .map(|_| ())
        .map_err(storage_error)
}

fn save_message_body(connection: &Connection, body: &MessageBody) -> Result<(), ApplicationError> {
    connection
        .execute(
            "
            INSERT INTO message_bodies (message_id, plain_text, sanitized_html)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(message_id) DO UPDATE SET
                plain_text = excluded.plain_text,
                sanitized_html = excluded.sanitized_html
            ",
            params![
                body.message_id.as_str(),
                body.plain_text,
                body.sanitized_html
            ],
        )
        .map(|_| ())
        .map_err(storage_error)
}

fn save_message_recipients(
    connection: &Connection,
    recipients: &MessageRecipients,
) -> Result<(), ApplicationError> {
    let valid = [&recipients.to, &recipients.cc, &recipients.bcc]
        .into_iter()
        .all(|value| value.chars().count() <= 64_000 && !value.contains('\0'));
    if !valid {
        return Err(ApplicationError::Storage(
            "message recipient metadata is invalid".into(),
        ));
    }
    connection
        .execute(
            "INSERT INTO message_recipients (
                message_id, to_recipients, cc_recipients, bcc_recipients
             ) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(message_id) DO UPDATE SET
                to_recipients = excluded.to_recipients,
                cc_recipients = excluded.cc_recipients,
                bcc_recipients = excluded.bcc_recipients",
            params![
                recipients.message_id.as_str(),
                recipients.to,
                recipients.cc,
                recipients.bcc,
            ],
        )
        .map(|_| ())
        .map_err(storage_error)
}

type RawAttachment = (
    String,
    String,
    String,
    String,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn raw_attachment(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawAttachment> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
    ))
}

fn delete_remote_message_ids(
    transaction: &rusqlite::Transaction<'_>,
    message_ids: &[String],
) -> Result<Vec<MessageAttachment>, ApplicationError> {
    let mut removed_attachments = Vec::new();
    for message_id in message_ids {
        let mut statement = transaction
            .prepare(
                "SELECT id, message_id, file_name, content_type, size_bytes, object_key,
                        remote_section, transfer_encoding
                 FROM message_attachments
                 WHERE message_id = ?1",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([message_id], raw_attachment)
            .map_err(storage_error)?;
        removed_attachments.extend(
            rows.map(|row| row.map_err(storage_error).and_then(attachment_from_raw))
                .collect::<Result<Vec<_>, ApplicationError>>()?,
        );
        transaction
            .execute("DELETE FROM messages WHERE id = ?1", [message_id])
            .map_err(storage_error)?;
    }
    Ok(removed_attachments)
}

fn attachment_from_raw(raw: RawAttachment) -> Result<MessageAttachment, ApplicationError> {
    Ok(MessageAttachment {
        id: AttachmentId::parse(raw.0).map_err(invalid_data)?,
        message_id: MessageId::parse(raw.1).map_err(invalid_data)?,
        file_name: raw.2,
        content_type: raw.3,
        size_bytes: u64::try_from(raw.4).map_err(invalid_data)?,
        object_key: raw.5,
        remote_section: raw.6,
        transfer_encoding: raw.7,
    })
}

fn save_attachment(
    connection: &Connection,
    attachment: &MessageAttachment,
) -> Result<(), ApplicationError> {
    let valid_object_key = attachment.object_key.as_deref().is_none_or(|object_key| {
        let mut components = Path::new(object_key).components();
        matches!(components.next(), Some(Component::Normal(value)) if value == "attachments")
            && components.all(|component| matches!(component, Component::Normal(_)))
    });
    let valid_remote_section = attachment
        .remote_section
        .as_deref()
        .is_none_or(valid_attachment_section);
    let valid_transfer_encoding = attachment.transfer_encoding.as_deref().is_none_or(|value| {
        !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    });
    if attachment.file_name.is_empty()
        || attachment.file_name.chars().any(char::is_control)
        || attachment.content_type.is_empty()
        || attachment.content_type.chars().any(char::is_control)
        || !attachment.content_type.contains('/')
        || !valid_object_key
        || !valid_remote_section
        || !valid_transfer_encoding
        || attachment.object_key.is_none() && attachment.remote_section.is_none()
        || attachment.remote_section.is_some() != attachment.transfer_encoding.is_some()
    {
        return Err(ApplicationError::Storage(
            "invalid attachment metadata".into(),
        ));
    }
    let size_bytes = i64::try_from(attachment.size_bytes).map_err(storage_error)?;
    connection
        .execute(
            "INSERT INTO message_attachments (
                id, message_id, file_name, content_type, size_bytes, object_key,
                remote_section, transfer_encoding
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                attachment.id.as_str(),
                attachment.message_id.as_str(),
                attachment.file_name,
                attachment.content_type,
                size_bytes,
                attachment.object_key,
                attachment.remote_section,
                attachment.transfer_encoding,
            ],
        )
        .map(|_| ())
        .map_err(storage_error)
}

/// Accepts an IMAP MIME section (`TEXT`, `1`, `2.3`) or an opaque provider
/// attachment identifier such as a Graph attachment ID.
fn valid_attachment_section(value: &str) -> bool {
    let imap_section = value == "TEXT"
        || !value.is_empty()
            && value.len() <= 128
            && value
                .split('.')
                .all(|component| component.parse::<u32>().is_ok_and(|section| section > 0));
    let provider_id = !value.is_empty()
        && value.len() <= 1_024
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'=' | b'+' | b'/')
        });
    imap_section || provider_id
}

fn refresh_mailbox_counts(transaction: &rusqlite::Transaction<'_>) -> Result<(), ApplicationError> {
    transaction
        .execute(
            "
            UPDATE mailboxes
            SET
                total_count = (
                    SELECT COUNT(*) FROM messages
                    WHERE messages.mailbox_id = mailboxes.id
                ),
                unread_count = (
                    SELECT COUNT(*) FROM messages
                    WHERE messages.mailbox_id = mailboxes.id
                      AND (messages.flags & ?1) = 0
                )
            ",
            [flag_bit(MessageFlag::Seen)],
        )
        .map(|_| ())
        .map_err(storage_error)
}

fn migrate(connection: &mut Connection) -> Result<(), ApplicationError> {
    let version: u32 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(storage_error)?;

    if version > CURRENT_SCHEMA_VERSION {
        return Err(ApplicationError::Storage(format!(
            "database schema {version} is newer than supported schema {CURRENT_SCHEMA_VERSION}"
        )));
    }

    if version < 1 {
        apply_migration_v1(connection)?;
    }

    if version < 2 {
        apply_migration_v2(connection)?;
    }

    if version < 3 {
        apply_migration_v3(connection)?;
    }

    if version < 4 {
        apply_migration_v4(connection)?;
    }

    if version < 5 {
        apply_migration_v5(connection)?;
    }

    if version < 6 {
        apply_migration_v6(connection)?;
    }

    if version < 7 {
        apply_migration_v7(connection)?;
    }

    if version < 8 {
        apply_migration_v8(connection)?;
    }

    if version < 9 {
        apply_migration_v9(connection)?;
    }

    if version < 10 {
        apply_migration_v10(connection)?;
    }

    if version < 11 {
        apply_migration_v11(connection)?;
    }

    if version < 12 {
        apply_migration_v12(connection)?;
    }

    if version < 13 {
        apply_migration_v13(connection)?;
    }

    if version < 14 {
        apply_migration_v14(connection)?;
    }

    if version < 15 {
        apply_migration_v15(connection)?;
    }

    Ok(())
}

fn apply_migration_v1(connection: &mut Connection) -> Result<(), ApplicationError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    transaction
        .execute_batch(
            "
                CREATE TABLE mailboxes (
                    id TEXT PRIMARY KEY NOT NULL,
                    account_id TEXT NOT NULL,
                    display_name TEXT NOT NULL,
                    role TEXT NOT NULL,
                    unread_count INTEGER NOT NULL DEFAULT 0
                        CHECK (unread_count >= 0),
                    total_count INTEGER NOT NULL DEFAULT 0
                        CHECK (total_count >= 0)
                ) STRICT;

                CREATE INDEX mailboxes_account_id
                    ON mailboxes (account_id);

                CREATE TABLE messages (
                    id TEXT PRIMARY KEY NOT NULL,
                    account_id TEXT NOT NULL,
                    mailbox_id TEXT NOT NULL
                        REFERENCES mailboxes(id) ON DELETE CASCADE,
                    from_address TEXT NOT NULL,
                    from_display_name TEXT,
                    subject TEXT NOT NULL,
                    preview TEXT NOT NULL,
                    received_at_ms INTEGER NOT NULL,
                    flags INTEGER NOT NULL DEFAULT 0,
                    has_attachments INTEGER NOT NULL DEFAULT 0
                        CHECK (has_attachments IN (0, 1))
                ) STRICT;

                CREATE INDEX messages_mailbox_received
                    ON messages (mailbox_id, received_at_ms DESC);

                CREATE TABLE message_bodies (
                    message_id TEXT PRIMARY KEY NOT NULL
                        REFERENCES messages(id) ON DELETE CASCADE,
                    plain_text TEXT,
                    sanitized_html TEXT,
                    CHECK (plain_text IS NOT NULL OR sanitized_html IS NOT NULL)
                ) STRICT;

                PRAGMA user_version = 1;
                ",
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)
}

fn apply_migration_v2(connection: &mut Connection) -> Result<(), ApplicationError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    transaction
        .execute_batch(
            "
                CREATE TABLE mail_accounts (
                    id TEXT PRIMARY KEY NOT NULL,
                    display_name TEXT NOT NULL,
                    email TEXT NOT NULL,
                    imap_host TEXT NOT NULL,
                    imap_port INTEGER NOT NULL CHECK (imap_port BETWEEN 1 AND 65535),
                    imap_security TEXT NOT NULL CHECK (imap_security IN ('tls', 'starttls')),
                    imap_username TEXT NOT NULL,
                    smtp_host TEXT NOT NULL,
                    smtp_port INTEGER NOT NULL CHECK (smtp_port BETWEEN 1 AND 65535),
                    smtp_security TEXT NOT NULL CHECK (smtp_security IN ('tls', 'starttls')),
                    smtp_username TEXT NOT NULL,
                    last_sync_at_ms INTEGER
                ) STRICT;

                CREATE TABLE calendar_events (
                    id TEXT PRIMARY KEY NOT NULL,
                    title TEXT NOT NULL,
                    starts_at_ms INTEGER NOT NULL,
                    ends_at_ms INTEGER NOT NULL,
                    location TEXT,
                    CHECK (ends_at_ms > starts_at_ms)
                ) STRICT;

                CREATE INDEX calendar_events_start
                    ON calendar_events (starts_at_ms);

                CREATE TABLE tasks (
                    id TEXT PRIMARY KEY NOT NULL,
                    title TEXT NOT NULL,
                    due_at_ms INTEGER,
                    completed INTEGER NOT NULL DEFAULT 0
                        CHECK (completed IN (0, 1))
                ) STRICT;

                CREATE INDEX tasks_state_due
                    ON tasks (completed, due_at_ms);

                CREATE TABLE contacts (
                    id TEXT PRIMARY KEY NOT NULL,
                    display_name TEXT NOT NULL,
                    email TEXT NOT NULL
                ) STRICT;

                CREATE INDEX contacts_display_name
                    ON contacts (display_name COLLATE NOCASE);

                INSERT INTO calendar_events
                    (id, title, starts_at_ms, ends_at_ms, location)
                VALUES
                    ('demo.calendar.standup', 'Team-Stand-up',
                     1785223800000, 1785225600000, 'Besprechungsraum Nord'),
                    ('demo.calendar.planning', 'Projektplanung',
                     1785412800000, 1785416400000, NULL);

                INSERT INTO tasks (id, title, due_at_ms, completed)
                VALUES
                    ('demo.task.imap', 'IMAP-Testkonto vorbereiten',
                     1785708000000, 0),
                    ('demo.task.architecture', 'Architekturfeedback einarbeiten',
                     1785794400000, 0),
                    ('demo.task.prototype', 'Desktop-Prototyp prüfen', NULL, 1);

                INSERT INTO contacts (id, display_name, email)
                VALUES
                    ('demo.contact.anna', 'Anna Schneider', 'anna@example.org'),
                    ('demo.contact.jonas', 'Jonas Weber', 'jonas@example.org'),
                    ('demo.contact.team', 'MAICENTA Team', 'hello@maicenta.local');

                PRAGMA user_version = 2;
                ",
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)
}

fn apply_migration_v3(connection: &mut Connection) -> Result<(), ApplicationError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    transaction
        .execute_batch(
            "
                CREATE TABLE remote_messages (
                    message_id TEXT PRIMARY KEY NOT NULL
                        REFERENCES messages(id) ON DELETE CASCADE,
                    account_id TEXT NOT NULL,
                    remote_mailbox TEXT NOT NULL,
                    uid_validity INTEGER NOT NULL
                        CHECK (uid_validity BETWEEN 0 AND 4294967295),
                    remote_uid INTEGER NOT NULL
                        CHECK (remote_uid BETWEEN 1 AND 4294967295)
                ) STRICT;

                CREATE INDEX remote_messages_account
                    ON remote_messages (account_id);

                CREATE TABLE pending_mail_mutations (
                    message_id TEXT PRIMARY KEY NOT NULL
                        REFERENCES messages(id) ON DELETE CASCADE,
                    account_id TEXT NOT NULL,
                    source_mailbox TEXT NOT NULL,
                    target_mailbox TEXT,
                    uid_validity INTEGER NOT NULL
                        CHECK (uid_validity BETWEEN 0 AND 4294967295),
                    remote_uid INTEGER NOT NULL
                        CHECK (remote_uid BETWEEN 1 AND 4294967295),
                    seen INTEGER NOT NULL CHECK (seen IN (0, 1)),
                    flagged INTEGER NOT NULL CHECK (flagged IN (0, 1)),
                    updated_at_ms INTEGER NOT NULL
                ) STRICT;

                CREATE INDEX pending_mail_mutations_account
                    ON pending_mail_mutations (account_id, updated_at_ms);

                PRAGMA user_version = 3;
                ",
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)
}

fn apply_migration_v4(connection: &mut Connection) -> Result<(), ApplicationError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    transaction
        .execute_batch(
            "
                CREATE TABLE message_attachments (
                    id TEXT PRIMARY KEY NOT NULL,
                    message_id TEXT NOT NULL
                        REFERENCES messages(id) ON DELETE CASCADE,
                    file_name TEXT NOT NULL,
                    content_type TEXT NOT NULL,
                    size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
                    object_key TEXT NOT NULL UNIQUE
                ) STRICT;

                CREATE INDEX message_attachments_message
                    ON message_attachments (message_id, id);

                PRAGMA user_version = 4;
                ",
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)
}

fn apply_migration_v5(connection: &mut Connection) -> Result<(), ApplicationError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    transaction
        .execute_batch(
            "
                ALTER TABLE message_attachments RENAME TO message_attachments_v4;
                DROP INDEX message_attachments_message;

                CREATE TABLE message_attachments (
                    id TEXT PRIMARY KEY NOT NULL,
                    message_id TEXT NOT NULL
                        REFERENCES messages(id) ON DELETE CASCADE,
                    file_name TEXT NOT NULL,
                    content_type TEXT NOT NULL,
                    size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
                    object_key TEXT UNIQUE,
                    remote_section TEXT,
                    transfer_encoding TEXT,
                    CHECK (object_key IS NOT NULL OR remote_section IS NOT NULL),
                    CHECK (
                        (remote_section IS NULL AND transfer_encoding IS NULL)
                        OR
                        (remote_section IS NOT NULL AND transfer_encoding IS NOT NULL)
                    )
                ) STRICT;

                INSERT INTO message_attachments (
                    id, message_id, file_name, content_type, size_bytes, object_key,
                    remote_section, transfer_encoding
                )
                SELECT id, message_id, file_name, content_type, size_bytes, object_key,
                       NULL, NULL
                FROM message_attachments_v4;

                DROP TABLE message_attachments_v4;

                CREATE INDEX message_attachments_message
                    ON message_attachments (message_id, id);

                PRAGMA user_version = 5;
                ",
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)
}

fn apply_migration_v6(connection: &mut Connection) -> Result<(), ApplicationError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    transaction
        .execute_batch(
            "
                ALTER TABLE remote_messages
                ADD COLUMN body_complete INTEGER NOT NULL DEFAULT 1
                    CHECK (body_complete IN (0, 1));

                PRAGMA user_version = 6;
                ",
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)
}

fn apply_migration_v7(connection: &mut Connection) -> Result<(), ApplicationError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    transaction
        .execute_batch(
            "
                CREATE TABLE local_drafts (
                    message_id TEXT PRIMARY KEY NOT NULL
                        REFERENCES messages(id) ON DELETE CASCADE,
                    to_recipients TEXT NOT NULL,
                    cc_recipients TEXT NOT NULL,
                    bcc_recipients TEXT NOT NULL,
                    editor_delta_json TEXT NOT NULL
                ) STRICT;

                INSERT INTO local_drafts (
                    message_id, to_recipients, cc_recipients, bcc_recipients,
                    editor_delta_json
                )
                SELECT id, '', '', '', ''
                FROM messages
                WHERE (flags & 8) != 0 AND id LIKE 'local.%';

                PRAGMA user_version = 7;
                ",
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)
}

fn apply_migration_v8(connection: &mut Connection) -> Result<(), ApplicationError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    transaction
        .execute_batch(
            "
                CREATE TABLE account_secrets (
                    account_id TEXT PRIMARY KEY NOT NULL
                        REFERENCES mail_accounts(id) ON DELETE CASCADE,
                    password TEXT NOT NULL CHECK (length(password) > 0)
                ) STRICT;

                PRAGMA user_version = 8;
                ",
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)
}

fn apply_migration_v14(connection: &mut Connection) -> Result<(), ApplicationError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    transaction
        .execute_batch(
            "
                ALTER TABLE account_secrets RENAME TO account_passwords_legacy;

                CREATE TABLE account_secrets (
                    account_id TEXT NOT NULL
                        REFERENCES mail_accounts(id) ON DELETE CASCADE,
                    key TEXT NOT NULL CHECK (length(key) BETWEEN 1 AND 64),
                    value TEXT NOT NULL CHECK (length(value) > 0),
                    PRIMARY KEY (account_id, key)
                ) STRICT;

                INSERT INTO account_secrets (account_id, key, value)
                SELECT account_id, 'password', password
                FROM account_passwords_legacy;

                DROP TABLE account_passwords_legacy;
                PRAGMA user_version = 14;
                ",
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)
}

fn apply_migration_v9(connection: &mut Connection) -> Result<(), ApplicationError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    transaction
        .execute_batch(
            "
                CREATE VIRTUAL TABLE message_search USING fts5(
                    message_id UNINDEXED,
                    sender,
                    email,
                    subject,
                    preview,
                    body,
                    recipients,
                    tokenize = 'unicode61 remove_diacritics 2'
                );

                INSERT INTO message_search (
                    message_id, sender, email, subject, preview, body, recipients
                )
                SELECT
                    messages.id,
                    COALESCE(messages.from_display_name, ''),
                    messages.from_address,
                    messages.subject,
                    messages.preview,
                    COALESCE(message_bodies.plain_text, message_bodies.sanitized_html, ''),
                    COALESCE(local_drafts.to_recipients, '') || ' ' ||
                    COALESCE(local_drafts.cc_recipients, '') || ' ' ||
                    COALESCE(local_drafts.bcc_recipients, '')
                FROM messages
                LEFT JOIN message_bodies ON message_bodies.message_id = messages.id
                LEFT JOIN local_drafts ON local_drafts.message_id = messages.id;

                CREATE TRIGGER messages_search_insert
                AFTER INSERT ON messages BEGIN
                    INSERT INTO message_search (
                        message_id, sender, email, subject, preview, body, recipients
                    ) VALUES (
                        new.id,
                        COALESCE(new.from_display_name, ''),
                        new.from_address,
                        new.subject,
                        new.preview,
                        '',
                        ''
                    );
                END;

                CREATE TRIGGER messages_search_update
                AFTER UPDATE OF from_address, from_display_name, subject, preview ON messages BEGIN
                    UPDATE message_search SET
                        sender = COALESCE(new.from_display_name, ''),
                        email = new.from_address,
                        subject = new.subject,
                        preview = new.preview
                    WHERE message_id = old.id;
                END;

                CREATE TRIGGER messages_search_delete
                AFTER DELETE ON messages BEGIN
                    DELETE FROM message_search WHERE message_id = old.id;
                END;

                CREATE TRIGGER message_bodies_search_insert
                AFTER INSERT ON message_bodies BEGIN
                    UPDATE message_search
                    SET body = COALESCE(new.plain_text, new.sanitized_html, '')
                    WHERE message_id = new.message_id;
                END;

                CREATE TRIGGER message_bodies_search_update
                AFTER UPDATE OF plain_text, sanitized_html ON message_bodies BEGIN
                    UPDATE message_search
                    SET body = COALESCE(new.plain_text, new.sanitized_html, '')
                    WHERE message_id = new.message_id;
                END;

                CREATE TRIGGER local_drafts_search_insert
                AFTER INSERT ON local_drafts BEGIN
                    UPDATE message_search SET recipients =
                        new.to_recipients || ' ' ||
                        new.cc_recipients || ' ' ||
                        new.bcc_recipients
                    WHERE message_id = new.message_id;
                END;

                CREATE TRIGGER local_drafts_search_update
                AFTER UPDATE OF to_recipients, cc_recipients, bcc_recipients ON local_drafts BEGIN
                    UPDATE message_search SET recipients =
                        new.to_recipients || ' ' ||
                        new.cc_recipients || ' ' ||
                        new.bcc_recipients
                    WHERE message_id = new.message_id;
                END;

                CREATE TRIGGER local_drafts_search_delete
                AFTER DELETE ON local_drafts BEGIN
                    UPDATE message_search SET recipients = ''
                    WHERE message_id = old.message_id;
                END;

                PRAGMA user_version = 9;
                ",
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)
}

// The declarative SQL is intentionally kept together so the rebuilt FTS table
// and every trigger are reviewed and committed as one atomic migration.
#[allow(clippy::too_many_lines)]
fn apply_migration_v10(connection: &mut Connection) -> Result<(), ApplicationError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    transaction
        .execute_batch(
            "
                ALTER TABLE remote_messages
                ADD COLUMN catalog_complete INTEGER NOT NULL DEFAULT 0
                    CHECK (catalog_complete IN (0, 1));

                ALTER TABLE remote_messages
                ADD COLUMN body_requested INTEGER NOT NULL DEFAULT 1
                    CHECK (body_requested IN (0, 1));

                CREATE TABLE message_recipients (
                    message_id TEXT PRIMARY KEY NOT NULL
                        REFERENCES messages(id) ON DELETE CASCADE,
                    to_recipients TEXT NOT NULL DEFAULT '',
                    cc_recipients TEXT NOT NULL DEFAULT '',
                    bcc_recipients TEXT NOT NULL DEFAULT ''
                ) STRICT;

                INSERT INTO message_recipients (
                    message_id, to_recipients, cc_recipients, bcc_recipients
                )
                SELECT
                    message_id, to_recipients, cc_recipients, bcc_recipients
                FROM local_drafts;

                DROP TRIGGER messages_search_insert;
                DROP TRIGGER messages_search_update;
                DROP TRIGGER messages_search_delete;
                DROP TRIGGER message_bodies_search_insert;
                DROP TRIGGER message_bodies_search_update;
                DROP TRIGGER local_drafts_search_insert;
                DROP TRIGGER local_drafts_search_update;
                DROP TRIGGER local_drafts_search_delete;
                DROP TABLE message_search;

                CREATE VIRTUAL TABLE message_search USING fts5(
                    message_id UNINDEXED,
                    subject,
                    sender,
                    email,
                    recipients,
                    attachment_names,
                    preview,
                    body,
                    prefix = '2 3 4',
                    tokenize = 'unicode61 remove_diacritics 2'
                );

                INSERT INTO message_search (
                    message_id, subject, sender, email, recipients,
                    attachment_names, preview, body
                )
                SELECT
                    messages.id,
                    messages.subject,
                    COALESCE(messages.from_display_name, ''),
                    messages.from_address,
                    COALESCE(message_recipients.to_recipients, '') || ' ' ||
                    COALESCE(message_recipients.cc_recipients, '') || ' ' ||
                    COALESCE(message_recipients.bcc_recipients, ''),
                    COALESCE((
                        SELECT group_concat(file_name, ' ')
                        FROM message_attachments
                        WHERE message_attachments.message_id = messages.id
                    ), ''),
                    messages.preview,
                    COALESCE(message_bodies.plain_text, message_bodies.sanitized_html, '')
                FROM messages
                LEFT JOIN message_bodies ON message_bodies.message_id = messages.id
                LEFT JOIN message_recipients
                    ON message_recipients.message_id = messages.id;

                CREATE TRIGGER messages_search_insert
                AFTER INSERT ON messages BEGIN
                    INSERT INTO message_search (
                        message_id, subject, sender, email, recipients,
                        attachment_names, preview, body
                    ) VALUES (
                        new.id,
                        new.subject,
                        COALESCE(new.from_display_name, ''),
                        new.from_address,
                        '',
                        '',
                        new.preview,
                        ''
                    );
                END;

                CREATE TRIGGER messages_search_update
                AFTER UPDATE OF from_address, from_display_name, subject, preview ON messages BEGIN
                    UPDATE message_search SET
                        subject = new.subject,
                        sender = COALESCE(new.from_display_name, ''),
                        email = new.from_address,
                        preview = new.preview
                    WHERE message_id = old.id;
                END;

                CREATE TRIGGER messages_search_delete
                AFTER DELETE ON messages BEGIN
                    DELETE FROM message_search WHERE message_id = old.id;
                END;

                CREATE TRIGGER message_bodies_search_insert
                AFTER INSERT ON message_bodies BEGIN
                    UPDATE message_search
                    SET body = COALESCE(new.plain_text, new.sanitized_html, '')
                    WHERE message_id = new.message_id;
                END;

                CREATE TRIGGER message_bodies_search_update
                AFTER UPDATE OF plain_text, sanitized_html ON message_bodies BEGIN
                    UPDATE message_search
                    SET body = COALESCE(new.plain_text, new.sanitized_html, '')
                    WHERE message_id = new.message_id;
                END;

                CREATE TRIGGER message_bodies_search_delete
                AFTER DELETE ON message_bodies BEGIN
                    UPDATE message_search SET body = ''
                    WHERE message_id = old.message_id;
                END;

                CREATE TRIGGER message_recipients_search_insert
                AFTER INSERT ON message_recipients BEGIN
                    UPDATE message_search SET recipients =
                        new.to_recipients || ' ' ||
                        new.cc_recipients || ' ' ||
                        new.bcc_recipients
                    WHERE message_id = new.message_id;
                END;

                CREATE TRIGGER message_recipients_search_update
                AFTER UPDATE OF to_recipients, cc_recipients, bcc_recipients
                ON message_recipients BEGIN
                    UPDATE message_search SET recipients =
                        new.to_recipients || ' ' ||
                        new.cc_recipients || ' ' ||
                        new.bcc_recipients
                    WHERE message_id = new.message_id;
                END;

                CREATE TRIGGER message_recipients_search_delete
                AFTER DELETE ON message_recipients BEGIN
                    UPDATE message_search SET recipients = ''
                    WHERE message_id = old.message_id;
                END;

                CREATE TRIGGER message_attachments_search_insert
                AFTER INSERT ON message_attachments BEGIN
                    UPDATE message_search SET attachment_names = COALESCE((
                        SELECT group_concat(file_name, ' ')
                        FROM message_attachments
                        WHERE message_id = new.message_id
                    ), '')
                    WHERE message_id = new.message_id;
                END;

                CREATE TRIGGER message_attachments_search_update
                AFTER UPDATE OF message_id, file_name ON message_attachments BEGIN
                    UPDATE message_search SET attachment_names = COALESCE((
                        SELECT group_concat(file_name, ' ')
                        FROM message_attachments
                        WHERE message_id = old.message_id
                    ), '')
                    WHERE message_id = old.message_id;
                    UPDATE message_search SET attachment_names = COALESCE((
                        SELECT group_concat(file_name, ' ')
                        FROM message_attachments
                        WHERE message_id = new.message_id
                    ), '')
                    WHERE message_id = new.message_id;
                END;

                CREATE TRIGGER message_attachments_search_delete
                AFTER DELETE ON message_attachments BEGIN
                    UPDATE message_search SET attachment_names = COALESCE((
                        SELECT group_concat(file_name, ' ')
                        FROM message_attachments
                        WHERE message_id = old.message_id
                    ), '')
                    WHERE message_id = old.message_id;
                END;

                PRAGMA user_version = 10;
                ",
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)
}

fn apply_migration_v11(connection: &mut Connection) -> Result<(), ApplicationError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    transaction
        .execute_batch(
            "
                CREATE TABLE remote_mailbox_sync_states (
                    account_id TEXT NOT NULL
                        REFERENCES mail_accounts(id) ON DELETE CASCADE,
                    remote_mailbox TEXT NOT NULL CHECK (length(remote_mailbox) > 0),
                    uid_validity INTEGER NOT NULL
                        CHECK (uid_validity BETWEEN 0 AND 4294967295),
                    uid_next INTEGER
                        CHECK (uid_next BETWEEN 1 AND 4294967295),
                    highest_modseq TEXT,
                    catalog_complete INTEGER NOT NULL DEFAULT 0
                        CHECK (catalog_complete IN (0, 1)),
                    last_full_reconcile_at_ms INTEGER NOT NULL
                        CHECK (last_full_reconcile_at_ms >= 0),
                    PRIMARY KEY (account_id, remote_mailbox)
                ) STRICT;

                PRAGMA user_version = 11;
                ",
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)
}

fn apply_migration_v12(connection: &mut Connection) -> Result<(), ApplicationError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    transaction
        .execute_batch(
            "
                CREATE TABLE pending_draft_operations (
                    message_id TEXT PRIMARY KEY NOT NULL
                        REFERENCES messages(id) ON DELETE CASCADE,
                    account_id TEXT NOT NULL
                        REFERENCES mail_accounts(id) ON DELETE CASCADE,
                    target_mailbox TEXT NOT NULL CHECK (length(target_mailbox) > 0),
                    operation TEXT NOT NULL
                        CHECK (operation IN ('upsert', 'delete')),
                    previous_remote_mailbox TEXT,
                    previous_uid_validity INTEGER
                        CHECK (previous_uid_validity BETWEEN 0 AND 4294967295),
                    previous_remote_uid INTEGER
                        CHECK (previous_remote_uid BETWEEN 1 AND 4294967295),
                    updated_at_ms INTEGER NOT NULL,
                    CHECK (
                        (previous_remote_mailbox IS NULL
                            AND previous_uid_validity IS NULL
                            AND previous_remote_uid IS NULL)
                        OR
                        (previous_remote_mailbox IS NOT NULL
                            AND length(previous_remote_mailbox) > 0
                            AND previous_uid_validity IS NOT NULL
                            AND previous_remote_uid IS NOT NULL)
                    ),
                    CHECK (operation != 'delete' OR previous_remote_uid IS NOT NULL)
                ) STRICT;

                CREATE INDEX pending_draft_operations_account
                    ON pending_draft_operations (account_id, updated_at_ms);

                PRAGMA user_version = 12;
                ",
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)
}

fn apply_migration_v13(connection: &mut Connection) -> Result<(), ApplicationError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    transaction
        .execute_batch(
            "
                CREATE TABLE workspace_preferences (
                    key TEXT PRIMARY KEY NOT NULL CHECK (length(key) BETWEEN 1 AND 100),
                    value TEXT NOT NULL CHECK (length(value) <= 20000)
                ) STRICT;

                PRAGMA user_version = 13;
                ",
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)
}

/// Generalizes remote message identity from IMAP UIDs to provider IDs.
///
/// Every row keeps exactly one identity form: the IMAP `uid_validity`/`remote_uid`
/// pair or an opaque `remote_id`. Mailboxes gain a provider-side `remote_name`
/// so a folder can carry a display name that differs from its stable remote
/// identifier, accounts record their protocol family, and mailbox sync states
/// can hold an opaque delta cursor.
#[allow(clippy::too_many_lines)]
fn apply_migration_v15(connection: &mut Connection) -> Result<(), ApplicationError> {
    let transaction = connection.transaction().map_err(storage_error)?;
    transaction
        .execute_batch(
            "
                ALTER TABLE mail_accounts
                ADD COLUMN provider TEXT NOT NULL DEFAULT 'imap'
                    CHECK (provider IN ('imap', 'microsoft_graph'));

                ALTER TABLE mailboxes ADD COLUMN remote_name TEXT
                    CHECK (remote_name IS NULL OR length(remote_name) BETWEEN 1 AND 1024);

                UPDATE mailboxes SET remote_name = display_name
                WHERE account_id IN (SELECT id FROM mail_accounts);

                ALTER TABLE remote_mailbox_sync_states ADD COLUMN delta_cursor TEXT
                    CHECK (delta_cursor IS NULL OR length(delta_cursor) BETWEEN 1 AND 8192);

                CREATE TABLE remote_messages_v15 (
                    message_id TEXT PRIMARY KEY NOT NULL
                        REFERENCES messages(id) ON DELETE CASCADE,
                    account_id TEXT NOT NULL,
                    remote_mailbox TEXT NOT NULL,
                    uid_validity INTEGER
                        CHECK (uid_validity BETWEEN 0 AND 4294967295),
                    remote_uid INTEGER
                        CHECK (remote_uid BETWEEN 1 AND 4294967295),
                    remote_id TEXT
                        CHECK (length(remote_id) BETWEEN 1 AND 1024),
                    catalog_complete INTEGER NOT NULL DEFAULT 0
                        CHECK (catalog_complete IN (0, 1)),
                    body_requested INTEGER NOT NULL DEFAULT 1
                        CHECK (body_requested IN (0, 1)),
                    body_complete INTEGER NOT NULL DEFAULT 1
                        CHECK (body_complete IN (0, 1)),
                    CHECK (
                        (uid_validity IS NOT NULL AND remote_uid IS NOT NULL
                            AND remote_id IS NULL)
                        OR
                        (uid_validity IS NULL AND remote_uid IS NULL
                            AND remote_id IS NOT NULL)
                    )
                ) STRICT;

                INSERT INTO remote_messages_v15 (
                    message_id, account_id, remote_mailbox, uid_validity, remote_uid,
                    remote_id, catalog_complete, body_requested, body_complete
                )
                SELECT message_id, account_id, remote_mailbox, uid_validity, remote_uid,
                       NULL, catalog_complete, body_requested, body_complete
                FROM remote_messages;

                DROP TABLE remote_messages;
                ALTER TABLE remote_messages_v15 RENAME TO remote_messages;
                CREATE INDEX remote_messages_account
                    ON remote_messages (account_id);
                CREATE INDEX remote_messages_mailbox
                    ON remote_messages (account_id, remote_mailbox);

                CREATE TABLE pending_mail_mutations_v15 (
                    message_id TEXT PRIMARY KEY NOT NULL
                        REFERENCES messages(id) ON DELETE CASCADE,
                    account_id TEXT NOT NULL,
                    source_mailbox TEXT NOT NULL,
                    target_mailbox TEXT,
                    uid_validity INTEGER
                        CHECK (uid_validity BETWEEN 0 AND 4294967295),
                    remote_uid INTEGER
                        CHECK (remote_uid BETWEEN 1 AND 4294967295),
                    remote_id TEXT
                        CHECK (length(remote_id) BETWEEN 1 AND 1024),
                    seen INTEGER NOT NULL CHECK (seen IN (0, 1)),
                    flagged INTEGER NOT NULL CHECK (flagged IN (0, 1)),
                    updated_at_ms INTEGER NOT NULL,
                    CHECK (
                        (uid_validity IS NOT NULL AND remote_uid IS NOT NULL
                            AND remote_id IS NULL)
                        OR
                        (uid_validity IS NULL AND remote_uid IS NULL
                            AND remote_id IS NOT NULL)
                    )
                ) STRICT;

                INSERT INTO pending_mail_mutations_v15 (
                    message_id, account_id, source_mailbox, target_mailbox,
                    uid_validity, remote_uid, remote_id, seen, flagged, updated_at_ms
                )
                SELECT message_id, account_id, source_mailbox, target_mailbox,
                       uid_validity, remote_uid, NULL, seen, flagged, updated_at_ms
                FROM pending_mail_mutations;

                DROP TABLE pending_mail_mutations;
                ALTER TABLE pending_mail_mutations_v15 RENAME TO pending_mail_mutations;
                CREATE INDEX pending_mail_mutations_account
                    ON pending_mail_mutations (account_id, updated_at_ms);

                CREATE TABLE pending_draft_operations_v15 (
                    message_id TEXT PRIMARY KEY NOT NULL
                        REFERENCES messages(id) ON DELETE CASCADE,
                    account_id TEXT NOT NULL
                        REFERENCES mail_accounts(id) ON DELETE CASCADE,
                    target_mailbox TEXT NOT NULL CHECK (length(target_mailbox) > 0),
                    operation TEXT NOT NULL
                        CHECK (operation IN ('upsert', 'delete')),
                    previous_remote_mailbox TEXT
                        CHECK (previous_remote_mailbox IS NULL
                            OR length(previous_remote_mailbox) > 0),
                    previous_uid_validity INTEGER
                        CHECK (previous_uid_validity BETWEEN 0 AND 4294967295),
                    previous_remote_uid INTEGER
                        CHECK (previous_remote_uid BETWEEN 1 AND 4294967295),
                    previous_remote_id TEXT
                        CHECK (length(previous_remote_id) BETWEEN 1 AND 1024),
                    updated_at_ms INTEGER NOT NULL,
                    CHECK (
                        (previous_remote_mailbox IS NULL
                            AND previous_uid_validity IS NULL
                            AND previous_remote_uid IS NULL
                            AND previous_remote_id IS NULL)
                        OR
                        (previous_remote_mailbox IS NOT NULL
                            AND previous_uid_validity IS NOT NULL
                            AND previous_remote_uid IS NOT NULL
                            AND previous_remote_id IS NULL)
                        OR
                        (previous_remote_mailbox IS NOT NULL
                            AND previous_uid_validity IS NULL
                            AND previous_remote_uid IS NULL
                            AND previous_remote_id IS NOT NULL)
                    ),
                    CHECK (operation != 'delete' OR previous_remote_mailbox IS NOT NULL)
                ) STRICT;

                INSERT INTO pending_draft_operations_v15 (
                    message_id, account_id, target_mailbox, operation,
                    previous_remote_mailbox, previous_uid_validity,
                    previous_remote_uid, previous_remote_id, updated_at_ms
                )
                SELECT message_id, account_id, target_mailbox, operation,
                       previous_remote_mailbox, previous_uid_validity,
                       previous_remote_uid, NULL, updated_at_ms
                FROM pending_draft_operations;

                DROP TABLE pending_draft_operations;
                ALTER TABLE pending_draft_operations_v15
                    RENAME TO pending_draft_operations;
                CREATE INDEX pending_draft_operations_account
                    ON pending_draft_operations (account_id, updated_at_ms);

                PRAGMA user_version = 15;
                ",
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)
}

fn search_expression(
    query: &str,
    include_content: bool,
) -> Result<Option<String>, ApplicationError> {
    if query.chars().count() > 256 {
        return Err(ApplicationError::Storage(
            "search query exceeds 256 characters".into(),
        ));
    }
    let tokens = query
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .take(16)
        .map(|token| token.chars().take(64).collect::<String>())
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        return Ok(None);
    }
    let terms = tokens
        .into_iter()
        .map(|token| format!("\"{token}\"*"))
        .collect::<Vec<_>>()
        .join(" AND ");
    let columns = if include_content {
        "{subject sender email recipients attachment_names preview body}"
    } else {
        "{subject sender email recipients}"
    };
    Ok(Some(format!("{columns} : ({terms})")))
}

struct RawMailbox {
    id: String,
    account_id: String,
    display_name: String,
    remote_name: Option<String>,
    role: String,
    unread_count: u32,
    total_count: u32,
}

struct RawMailAccount {
    id: String,
    provider: String,
    display_name: String,
    email: String,
    imap_host: String,
    imap_port: u16,
    imap_security: String,
    imap_username: String,
    smtp_host: String,
    smtp_port: u16,
    smtp_security: String,
    smtp_username: String,
    last_sync_at_ms: Option<i64>,
}

impl RawMailAccount {
    fn into_domain(self) -> Result<MailAccount, ApplicationError> {
        Ok(MailAccount {
            id: AccountId::parse(self.id).map_err(invalid_data)?,
            provider: mail_provider_from_str(&self.provider)?,
            email: MailAddress::new(self.email, Some(self.display_name.clone()))
                .map_err(invalid_data)?,
            display_name: self.display_name,
            imap_host: self.imap_host,
            imap_port: self.imap_port,
            imap_security: transport_security_from_str(&self.imap_security)?,
            imap_username: self.imap_username,
            smtp_host: self.smtp_host,
            smtp_port: self.smtp_port,
            smtp_security: transport_security_from_str(&self.smtp_security)?,
            smtp_username: self.smtp_username,
            last_sync_at_ms: self.last_sync_at_ms,
        })
    }
}

impl RawMailbox {
    fn into_domain(self) -> Result<Mailbox, ApplicationError> {
        Ok(Mailbox {
            id: MailboxId::parse(self.id).map_err(invalid_data)?,
            account_id: AccountId::parse(self.account_id).map_err(invalid_data)?,
            display_name: self.display_name,
            remote_name: self.remote_name,
            role: mailbox_role_from_str(&self.role)?,
            unread_count: self.unread_count,
            total_count: self.total_count,
        })
    }
}

struct RawMessageSummary {
    id: String,
    account_id: String,
    mailbox_id: String,
    from_address: String,
    from_display_name: Option<String>,
    subject: String,
    preview: String,
    received_at_ms: i64,
    flags: u32,
    has_attachments: bool,
}

impl RawMessageSummary {
    fn into_domain(self) -> Result<MessageSummary, ApplicationError> {
        Ok(MessageSummary {
            id: MessageId::parse(self.id).map_err(invalid_data)?,
            account_id: AccountId::parse(self.account_id).map_err(invalid_data)?,
            mailbox_id: MailboxId::parse(self.mailbox_id).map_err(invalid_data)?,
            from: MailAddress::new(self.from_address, self.from_display_name)
                .map_err(invalid_data)?,
            subject: self.subject,
            preview: self.preview,
            received_at_ms: self.received_at_ms,
            flags: decode_flags(self.flags),
            has_attachments: self.has_attachments,
        })
    }
}

struct RawMessageBody {
    message_id: String,
    plain_text: Option<String>,
    sanitized_html: Option<String>,
}

impl RawMessageBody {
    fn into_domain(self) -> Result<MessageBody, ApplicationError> {
        Ok(MessageBody {
            message_id: MessageId::parse(self.message_id).map_err(invalid_data)?,
            plain_text: self.plain_text,
            sanitized_html: self.sanitized_html,
        })
    }
}

const fn mailbox_role_to_str(role: MailboxRole) -> &'static str {
    match role {
        MailboxRole::Inbox => "inbox",
        MailboxRole::Drafts => "drafts",
        MailboxRole::Sent => "sent",
        MailboxRole::Archive => "archive",
        MailboxRole::Trash => "trash",
        MailboxRole::Junk => "junk",
        MailboxRole::Custom => "custom",
    }
}

fn mailbox_role_from_str(value: &str) -> Result<MailboxRole, ApplicationError> {
    match value {
        "inbox" => Ok(MailboxRole::Inbox),
        "drafts" => Ok(MailboxRole::Drafts),
        "sent" => Ok(MailboxRole::Sent),
        "archive" => Ok(MailboxRole::Archive),
        "trash" => Ok(MailboxRole::Trash),
        "junk" => Ok(MailboxRole::Junk),
        "custom" => Ok(MailboxRole::Custom),
        other => Err(ApplicationError::Storage(format!(
            "unknown mailbox role: {other}"
        ))),
    }
}

const fn transport_security_to_str(value: TransportSecurity) -> &'static str {
    match value {
        TransportSecurity::Tls => "tls",
        TransportSecurity::StartTls => "starttls",
    }
}

const fn mail_provider_to_str(value: MailProvider) -> &'static str {
    match value {
        MailProvider::ImapSmtp => "imap",
        MailProvider::MicrosoftGraph => "microsoft_graph",
    }
}

fn mail_provider_from_str(value: &str) -> Result<MailProvider, ApplicationError> {
    match value {
        "imap" => Ok(MailProvider::ImapSmtp),
        "microsoft_graph" => Ok(MailProvider::MicrosoftGraph),
        other => Err(ApplicationError::Storage(format!(
            "unknown mail provider: {other}"
        ))),
    }
}

/// Splits one remote identity into the three nullable identity columns.
fn identity_columns(identity: &RemoteMessageIdentity) -> (Option<u32>, Option<u32>, Option<&str>) {
    match identity {
        RemoteMessageIdentity::ImapUid { uid_validity, uid } => {
            (Some(*uid_validity), Some(*uid), None)
        }
        RemoteMessageIdentity::ProviderId(id) => (None, None, Some(id.as_str())),
    }
}

fn identity_from_columns(
    uid_validity: Option<u32>,
    remote_uid: Option<u32>,
    provider_id: Option<String>,
) -> Result<RemoteMessageIdentity, ApplicationError> {
    match (uid_validity, remote_uid, provider_id) {
        (Some(uid_validity), Some(uid), None) => {
            Ok(RemoteMessageIdentity::ImapUid { uid_validity, uid })
        }
        (None, None, Some(id)) => Ok(RemoteMessageIdentity::ProviderId(id)),
        _ => Err(ApplicationError::Storage(
            "remote message row has an inconsistent identity".into(),
        )),
    }
}

struct RawRemoteMetadata {
    message_id: String,
    account_id: String,
    remote_mailbox: String,
    uid_validity: Option<u32>,
    remote_uid: Option<u32>,
    provider_id: Option<String>,
    catalog_complete: bool,
    body_requested: bool,
    body_complete: bool,
}

impl RawRemoteMetadata {
    fn into_domain(self) -> Result<RemoteMessageMetadata, ApplicationError> {
        Ok(RemoteMessageMetadata {
            message_id: MessageId::parse(self.message_id).map_err(invalid_data)?,
            account_id: AccountId::parse(self.account_id).map_err(invalid_data)?,
            remote_mailbox: self.remote_mailbox,
            identity: identity_from_columns(self.uid_validity, self.remote_uid, self.provider_id)?,
            catalog_complete: self.catalog_complete,
            body_requested: self.body_requested,
            body_complete: self.body_complete,
        })
    }
}

fn remote_metadata_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawRemoteMetadata> {
    Ok(RawRemoteMetadata {
        message_id: row.get(0)?,
        account_id: row.get(1)?,
        remote_mailbox: row.get(2)?,
        uid_validity: row.get(3)?,
        remote_uid: row.get(4)?,
        provider_id: row.get(5)?,
        catalog_complete: row.get(6)?,
        body_requested: row.get(7)?,
        body_complete: row.get(8)?,
    })
}

/// Loads the remote mailbox and identity of one cached message, if any.
fn query_remote_identity(
    connection: &Connection,
    message_id: &MessageId,
) -> Result<Option<(String, RemoteMessageIdentity)>, ApplicationError> {
    let row = connection
        .query_row(
            "SELECT remote_mailbox, uid_validity, remote_uid, remote_id
             FROM remote_messages WHERE message_id = ?1",
            [message_id.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<u32>>(1)?,
                    row.get::<_, Option<u32>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)?;
    row.map(|(remote_mailbox, uid_validity, remote_uid, provider_id)| {
        Ok((
            remote_mailbox,
            identity_from_columns(uid_validity, remote_uid, provider_id)?,
        ))
    })
    .transpose()
}

/// Lists every cached remote identity inside one remote mailbox.
fn mailbox_remote_identities(
    connection: &Connection,
    account_id: &AccountId,
    remote_mailbox: &str,
) -> Result<Vec<(String, RemoteMessageIdentity)>, ApplicationError> {
    let mut statement = connection
        .prepare(
            "SELECT message_id, uid_validity, remote_uid, remote_id
             FROM remote_messages
             WHERE account_id = ?1 AND remote_mailbox = ?2",
        )
        .map_err(storage_error)?;
    let rows = statement
        .query_map(params![account_id.as_str(), remote_mailbox], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<u32>>(1)?,
                row.get::<_, Option<u32>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(storage_error)?;
    rows.map(|row| {
        let (message_id, uid_validity, remote_uid, provider_id) = row.map_err(storage_error)?;
        Ok((
            message_id,
            identity_from_columns(uid_validity, remote_uid, provider_id)?,
        ))
    })
    .collect()
}

fn transport_security_from_str(value: &str) -> Result<TransportSecurity, ApplicationError> {
    match value {
        "tls" => Ok(TransportSecurity::Tls),
        "starttls" => Ok(TransportSecurity::StartTls),
        other => Err(ApplicationError::Storage(format!(
            "unknown transport security mode: {other}"
        ))),
    }
}

const fn flag_bit(flag: MessageFlag) -> u32 {
    match flag {
        MessageFlag::Seen => 1 << 0,
        MessageFlag::Answered => 1 << 1,
        MessageFlag::Flagged => 1 << 2,
        MessageFlag::Draft => 1 << 3,
        MessageFlag::Deleted => 1 << 4,
    }
}

fn encode_flags(flags: &[MessageFlag]) -> u32 {
    flags
        .iter()
        .fold(0, |encoded, flag| encoded | flag_bit(*flag))
}

fn decode_flags(encoded: u32) -> Vec<MessageFlag> {
    [
        MessageFlag::Seen,
        MessageFlag::Answered,
        MessageFlag::Flagged,
        MessageFlag::Draft,
        MessageFlag::Deleted,
    ]
    .into_iter()
    .filter(|flag| encoded & flag_bit(*flag) != 0)
    .collect()
}

fn storage_error(error: impl std::fmt::Display) -> ApplicationError {
    ApplicationError::Storage(error.to_string())
}

fn invalid_data(error: impl std::fmt::Display) -> ApplicationError {
    ApplicationError::Storage(format!("invalid data in local database: {error}"))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::{
            atomic::{AtomicU64, Ordering},
            mpsc,
        },
        thread,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    use maicenta_application::{
        ApplicationError, LocalDraftMetadata, LocalDraftStore, MailAccountStore, MailStore,
        MailSyncStore, PendingDraftAction, RemoteMailboxSyncState, RemoteMessageIdentity,
        RemoteMessageMetadata, SecretStore, WorkspaceStore,
    };
    use maicenta_domain::{
        AccountId, AttachmentId, CalendarEvent, Contact, MailAccount, MailAddress, MailProvider,
        Mailbox, MailboxId, MailboxRole, MessageAttachment, MessageBody, MessageFlag, MessageId,
        MessageRecipients, MessageSummary, TaskItem, TransportSecurity, WorkspaceItemId,
    };

    use rusqlite::Connection;

    use super::{
        CURRENT_SCHEMA_VERSION, SQLITE_HEADER, SqliteMailStore, apply_migration_v1,
        apply_migration_v2, apply_migration_v3, apply_migration_v4, valid_attachment_section,
    };

    static NEXT_DATABASE_ID: AtomicU64 = AtomicU64::new(0);

    struct TemporaryDatabase {
        path: PathBuf,
    }

    impl TemporaryDatabase {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_nanos();
            let serial = NEXT_DATABASE_ID.fetch_add(1, Ordering::Relaxed);
            Self {
                path: std::env::temp_dir().join(format!(
                    "maicenta-storage-{}-{unique}-{serial}.sqlite",
                    std::process::id()
                )),
            }
        }
    }

    impl Drop for TemporaryDatabase {
        fn drop(&mut self) {
            for suffix in ["", "-shm", "-wal"] {
                let mut path = self.path.as_os_str().to_owned();
                path.push(suffix);
                let _ = fs::remove_file(PathBuf::from(path));
            }
        }
    }

    fn mailbox() -> Mailbox {
        Mailbox {
            id: MailboxId::parse("inbox").expect("valid id"),
            account_id: AccountId::parse("personal").expect("valid id"),
            display_name: "Posteingang".into(),
            remote_name: Some("Posteingang".into()),
            role: MailboxRole::Inbox,
            unread_count: 1,
            total_count: 1,
        }
    }

    fn summary() -> MessageSummary {
        MessageSummary {
            id: MessageId::parse("message-1").expect("valid id"),
            account_id: AccountId::parse("personal").expect("valid id"),
            mailbox_id: MailboxId::parse("inbox").expect("valid id"),
            from: MailAddress::new("anna@example.org", Some("Anna".into())).expect("valid address"),
            subject: "Projektplanung".into(),
            preview: "Die aktuellen Punkte".into(),
            received_at_ms: 1_785_312_000_000,
            flags: vec![MessageFlag::Flagged],
            has_attachments: true,
        }
    }

    fn remote_metadata(message_id: MessageId) -> RemoteMessageMetadata {
        RemoteMessageMetadata {
            message_id,
            account_id: AccountId::parse("personal").expect("valid id"),
            remote_mailbox: "Posteingang".into(),
            identity: RemoteMessageIdentity::ImapUid {
                uid_validity: 42,
                uid: 7,
            },
            catalog_complete: true,
            body_requested: true,
            body_complete: true,
        }
    }

    fn attachment(message_id: MessageId) -> MessageAttachment {
        MessageAttachment {
            id: AttachmentId::parse("attachment.test.1").expect("attachment id"),
            message_id,
            file_name: "Projektübersicht.pdf".into(),
            content_type: "application/pdf".into(),
            size_bytes: 245_000,
            object_key: Some("attachments/attachment.test.1.bin".into()),
            remote_section: None,
            transfer_encoding: None,
        }
    }

    fn recipients(message_id: MessageId) -> MessageRecipients {
        MessageRecipients {
            message_id,
            to: "Tim Test <tim@example.org>".into(),
            cc: "Projektgruppe <projekt@example.org>".into(),
            bcc: String::new(),
        }
    }

    fn assert_remote_reconciliation(
        store: &mut SqliteMailStore,
        message: &MessageSummary,
        metadata: &RemoteMessageMetadata,
        remote_attachment: MessageAttachment,
    ) {
        assert_eq!(
            store
                .remote_messages_for_account(&message.account_id)
                .expect("account remote identities"),
            vec![metadata.clone()]
        );
        store
            .update_remote_message_flags(&message.id, &[MessageFlag::Seen, MessageFlag::Answered])
            .expect("apply server flags");
        let updated = store
            .list_messages(&message.mailbox_id, 20)
            .expect("updated message");
        assert!(updated[0].flags.contains(&MessageFlag::Seen));
        assert!(updated[0].flags.contains(&MessageFlag::Answered));
        assert!(
            store
                .reconcile_remote_mailbox(
                    &metadata.account_id,
                    &metadata.remote_mailbox,
                    std::slice::from_ref(&metadata.identity),
                )
                .expect("matching mailbox snapshot")
                .is_empty()
        );
        assert_eq!(
            store
                .reconcile_remote_mailbox(
                    &metadata.account_id,
                    &metadata.remote_mailbox,
                    &[RemoteMessageIdentity::ImapUid {
                        uid_validity: 43,
                        uid: 7,
                    }],
                )
                .expect("changed UIDVALIDITY"),
            vec![remote_attachment]
        );
        assert_eq!(
            store.remote_message_metadata(&message.id),
            Err(ApplicationError::NotFound)
        );
    }

    fn archive_mailbox() -> Mailbox {
        Mailbox {
            id: MailboxId::parse("archive").expect("valid id"),
            account_id: AccountId::parse("personal").expect("valid id"),
            display_name: "Archiv".into(),
            remote_name: None,
            role: MailboxRole::Archive,
            unread_count: 0,
            total_count: 0,
        }
    }

    fn mail_account() -> MailAccount {
        MailAccount {
            id: AccountId::parse("work-account").expect("account id"),
            provider: MailProvider::ImapSmtp,
            display_name: "Arbeit".into(),
            email: MailAddress::new("user@example.org", Some("Arbeit".into()))
                .expect("account address"),
            imap_host: "imap.example.org".into(),
            imap_port: 993,
            imap_security: TransportSecurity::Tls,
            imap_username: "user@example.org".into(),
            smtp_host: "smtp.example.org".into(),
            smtp_port: 587,
            smtp_security: TransportSecurity::StartTls,
            smtp_username: "user@example.org".into(),
            last_sync_at_ms: None,
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn queues_and_completes_the_full_remote_draft_lifecycle() {
        let mut store = SqliteMailStore::open_in_memory().expect("profile");
        let account = mail_account();
        store.save_mail_account(&account).expect("account");
        let drafts = Mailbox {
            id: MailboxId::parse("work.drafts").expect("draft mailbox id"),
            account_id: account.id.clone(),
            display_name: "Drafts".into(),
            remote_name: None,
            role: MailboxRole::Drafts,
            unread_count: 0,
            total_count: 0,
        };
        let sent = Mailbox {
            id: MailboxId::parse("work.sent").expect("sent mailbox id"),
            account_id: account.id.clone(),
            display_name: "Sent".into(),
            remote_name: None,
            role: MailboxRole::Sent,
            unread_count: 0,
            total_count: 0,
        };
        store
            .save_mailboxes(&[drafts.clone(), sent.clone()])
            .expect("mailboxes");
        let mut message = summary();
        message.id = MessageId::parse("local.work.draft").expect("message id");
        message.account_id = account.id.clone();
        message.mailbox_id = drafts.id;
        message.flags = vec![MessageFlag::Seen, MessageFlag::Draft];
        message.has_attachments = false;
        let body = MessageBody {
            message_id: message.id.clone(),
            plain_text: Some("Entwurfstext".into()),
            sanitized_html: Some("<p>Entwurfstext</p>".into()),
        };
        let recipients = MessageRecipients {
            message_id: message.id.clone(),
            to: "anna@example.org".into(),
            cc: String::new(),
            bcc: String::new(),
        };
        let draft = LocalDraftMetadata {
            message_id: message.id.clone(),
            to: recipients.to.clone(),
            cc: String::new(),
            bcc: String::new(),
            editor_delta_json: "[]".into(),
        };
        store
            .save_local_message(&message, &body, &recipients, &[], Some(&draft))
            .expect("local draft");
        let queued = store
            .pending_draft_operations(&account.id)
            .expect("draft queue");
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].action, PendingDraftAction::Upsert);
        assert_eq!(queued[0].target_mailbox, "Drafts");
        assert!(queued[0].previous_remote.is_none());
        assert_eq!(store.pending_mail_mutation_count(), Ok(1));

        let remote = RemoteMessageMetadata {
            message_id: message.id.clone(),
            account_id: account.id.clone(),
            remote_mailbox: "Drafts".into(),
            identity: RemoteMessageIdentity::ImapUid {
                uid_validity: 91,
                uid: 14,
            },
            catalog_complete: true,
            body_requested: true,
            body_complete: true,
        };
        store
            .complete_draft_operation(&message.id, Some(&remote))
            .expect("complete upload");
        assert_eq!(store.pending_mail_mutation_count(), Ok(0));
        assert_eq!(
            store
                .remote_message_metadata(&message.id)
                .expect("uploaded identity"),
            remote
        );

        message.subject = "Bearbeiteter Entwurf".into();
        store
            .save_local_message(&message, &body, &recipients, &[], Some(&draft))
            .expect("edited draft");
        let replacement = store
            .pending_draft_operations(&account.id)
            .expect("replacement queue");
        assert_eq!(replacement[0].action, PendingDraftAction::Upsert);
        assert_eq!(replacement[0].previous_remote.as_ref(), Some(&remote));

        message.mailbox_id = sent.id;
        message.flags.retain(|flag| *flag != MessageFlag::Draft);
        store
            .save_local_message(&message, &body, &recipients, &[], None)
            .expect("sent local draft");
        let deletion = store
            .pending_draft_operations(&account.id)
            .expect("deletion queue");
        assert_eq!(deletion.len(), 1);
        assert_eq!(deletion[0].action, PendingDraftAction::Delete);
        assert_eq!(deletion[0].previous_remote.as_ref(), Some(&remote));
        store
            .complete_draft_operation(&message.id, None)
            .expect("complete deletion");
        assert_eq!(
            store.remote_message_metadata(&message.id),
            Err(ApplicationError::NotFound)
        );
        assert_eq!(store.pending_mail_mutation_count(), Ok(0));
    }

    #[test]
    fn retaining_an_editable_server_draft_does_not_queue_an_upload() {
        let mut store = SqliteMailStore::open_in_memory().expect("profile");
        let account = mail_account();
        store.save_mail_account(&account).expect("account");
        let drafts = Mailbox {
            id: MailboxId::parse("work.remote-drafts").expect("draft mailbox id"),
            account_id: account.id.clone(),
            display_name: "Drafts".into(),
            remote_name: None,
            role: MailboxRole::Drafts,
            unread_count: 0,
            total_count: 0,
        };
        store
            .save_mailboxes(std::slice::from_ref(&drafts))
            .expect("draft mailbox");
        let mut message = summary();
        message.id = MessageId::parse("work.remote.draft").expect("message id");
        message.account_id = account.id.clone();
        message.mailbox_id = drafts.id;
        message.flags = vec![MessageFlag::Seen, MessageFlag::Draft];
        message.has_attachments = false;
        let body = MessageBody {
            message_id: message.id.clone(),
            plain_text: Some("Serverentwurf".into()),
            sanitized_html: Some("<p>Serverentwurf</p>".into()),
        };
        let recipients = MessageRecipients {
            message_id: message.id.clone(),
            to: "anna@example.org".into(),
            cc: String::new(),
            bcc: String::new(),
        };
        let remote = RemoteMessageMetadata {
            message_id: message.id.clone(),
            account_id: account.id.clone(),
            remote_mailbox: "Drafts".into(),
            identity: RemoteMessageIdentity::ImapUid {
                uid_validity: 77,
                uid: 9,
            },
            catalog_complete: true,
            body_requested: true,
            body_complete: true,
        };
        store
            .save_remote_message(&message, &body, &recipients, &remote, &[])
            .expect("remote draft");
        let editable = LocalDraftMetadata {
            message_id: message.id.clone(),
            to: recipients.to,
            cc: recipients.cc,
            bcc: recipients.bcc,
            editor_delta_json: String::new(),
        };
        store
            .save_synchronized_draft_metadata(&editable)
            .expect("editable remote draft");

        assert_eq!(
            store
                .local_draft_metadata(&message.id)
                .expect("draft metadata"),
            Some(editable)
        );
        assert!(
            store
                .pending_draft_operations(&account.id)
                .expect("draft queue")
                .is_empty()
        );
    }

    #[test]
    fn accepts_imap_sections_and_provider_attachment_identifiers() {
        assert!(valid_attachment_section("TEXT"));
        assert!(valid_attachment_section("2.1"));
        assert!(valid_attachment_section("AAMkAGI2THVSAAA=_x-y/z+"));
        assert!(!valid_attachment_section(""));
        assert!(!valid_attachment_section("../secret"));
        assert!(!valid_attachment_section("has space"));
    }

    #[test]
    fn initializes_current_schema() {
        let store = SqliteMailStore::open_in_memory().expect("open database");
        assert_eq!(
            store.schema_version().expect("schema version"),
            CURRENT_SCHEMA_VERSION
        );
    }

    #[test]
    fn pages_message_summaries_without_loading_the_whole_mailbox() {
        let mut store = SqliteMailStore::open_in_memory().expect("open database");
        store.save_mailboxes(&[mailbox()]).expect("save mailbox");
        let messages = (0..5)
            .map(|index| {
                let mut message = summary();
                message.id = MessageId::parse(format!("message-{index}")).expect("message id");
                message.received_at_ms += i64::from(index);
                message
            })
            .collect::<Vec<_>>();
        store.save_summaries(&messages).expect("save summaries");

        let page = store
            .list_message_page(&mailbox().id, 1, 2)
            .expect("second page");

        assert_eq!(
            page.into_iter()
                .map(|message| message.id.to_string())
                .collect::<Vec<_>>(),
            ["message-3", "message-2"]
        );
    }

    #[test]
    fn creates_and_reopens_an_encrypted_profile_database() {
        let database = TemporaryDatabase::new();
        let key = [0x31; 32];
        let wrong_key = [0x32; 32];
        let store = SqliteMailStore::open_encrypted(&database.path, &key)
            .expect("create encrypted database");
        assert_eq!(
            store.schema_version().expect("schema version"),
            CURRENT_SCHEMA_VERSION
        );
        store.integrity_check().expect("encrypted integrity");
        drop(store);

        let bytes = fs::read(&database.path).expect("database bytes");
        assert_ne!(&bytes[..16], SQLITE_HEADER);
        assert!(SqliteMailStore::open_encrypted(&database.path, &wrong_key).is_err());
        SqliteMailStore::open_encrypted(&database.path, &key).expect("reopen with profile key");
    }

    #[test]
    fn migrates_a_plaintext_profile_without_losing_data() {
        let database = TemporaryDatabase::new();
        let mut plaintext = SqliteMailStore::open(&database.path).expect("plaintext profile");
        plaintext
            .save_mailboxes(&[mailbox()])
            .expect("save plaintext mailbox");
        drop(plaintext);

        let key = [0x44; 32];
        let encrypted = SqliteMailStore::open_encrypted(&database.path, &key)
            .expect("migrate plaintext profile");
        assert_eq!(
            encrypted
                .list_mailboxes(&mailbox().account_id)
                .expect("migrated mailboxes"),
            vec![mailbox()]
        );
        drop(encrypted);
        let bytes = fs::read(&database.path).expect("database bytes");
        assert_ne!(&bytes[..16], SQLITE_HEADER);
    }

    #[test]
    fn stores_independent_account_secrets_inside_the_encrypted_profile() {
        let mut store = SqliteMailStore::open_in_memory().expect("profile");
        let account = mail_account();
        store.save_mail_account(&account).expect("save account");
        store
            .set(&account.id, "password", "app-password")
            .expect("save password");
        store
            .set(&account.id, "oauth.refresh_token", "refresh-token")
            .expect("save refresh token");
        assert_eq!(
            store.get(&account.id, "password").expect("load password"),
            Some("app-password".into())
        );
        assert_eq!(
            store
                .get(&account.id, "oauth.refresh_token")
                .expect("load refresh token"),
            Some("refresh-token".into())
        );
        store
            .remove(&account.id, "password")
            .expect("remove password");
        assert_eq!(
            store
                .get(&account.id, "password")
                .expect("removed password"),
            None
        );
        assert_eq!(
            store
                .get(&account.id, "oauth.refresh_token")
                .expect("refresh token retained"),
            Some("refresh-token".into())
        );
        store
            .delete_mail_account(&account.id)
            .expect("delete account");
        assert_eq!(
            store
                .get(&account.id, "oauth.refresh_token")
                .expect("deleted secret"),
            None
        );
    }

    #[test]
    fn migrates_existing_password_rows_to_named_secrets() {
        let connection = Connection::open_in_memory().expect("database");
        connection
            .execute_batch(
                "
                PRAGMA foreign_keys = ON;
                CREATE TABLE mail_accounts (
                    id TEXT PRIMARY KEY NOT NULL
                ) STRICT;
                CREATE TABLE account_secrets (
                    account_id TEXT PRIMARY KEY NOT NULL
                        REFERENCES mail_accounts(id) ON DELETE CASCADE,
                    password TEXT NOT NULL CHECK (length(password) > 0)
                ) STRICT;
                -- Minimal v13 shapes of the tables that later migrations rebuild.
                CREATE TABLE mailboxes (
                    id TEXT PRIMARY KEY NOT NULL,
                    account_id TEXT NOT NULL,
                    display_name TEXT NOT NULL
                ) STRICT;
                CREATE TABLE messages (id TEXT PRIMARY KEY NOT NULL) STRICT;
                CREATE TABLE remote_messages (
                    message_id TEXT PRIMARY KEY NOT NULL
                        REFERENCES messages(id) ON DELETE CASCADE,
                    account_id TEXT NOT NULL,
                    remote_mailbox TEXT NOT NULL,
                    uid_validity INTEGER NOT NULL,
                    remote_uid INTEGER NOT NULL,
                    body_complete INTEGER NOT NULL DEFAULT 1,
                    catalog_complete INTEGER NOT NULL DEFAULT 0,
                    body_requested INTEGER NOT NULL DEFAULT 1
                ) STRICT;
                CREATE TABLE pending_mail_mutations (
                    message_id TEXT PRIMARY KEY NOT NULL,
                    account_id TEXT NOT NULL,
                    source_mailbox TEXT NOT NULL,
                    target_mailbox TEXT,
                    uid_validity INTEGER NOT NULL,
                    remote_uid INTEGER NOT NULL,
                    seen INTEGER NOT NULL,
                    flagged INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                ) STRICT;
                CREATE TABLE remote_mailbox_sync_states (
                    account_id TEXT NOT NULL,
                    remote_mailbox TEXT NOT NULL,
                    uid_validity INTEGER NOT NULL,
                    PRIMARY KEY (account_id, remote_mailbox)
                ) STRICT;
                CREATE TABLE pending_draft_operations (
                    message_id TEXT PRIMARY KEY NOT NULL,
                    account_id TEXT NOT NULL,
                    target_mailbox TEXT NOT NULL,
                    operation TEXT NOT NULL,
                    previous_remote_mailbox TEXT,
                    previous_uid_validity INTEGER,
                    previous_remote_uid INTEGER,
                    updated_at_ms INTEGER NOT NULL
                ) STRICT;
                INSERT INTO mail_accounts (id) VALUES ('work');
                INSERT INTO account_secrets (account_id, password)
                VALUES ('work', 'existing-password');
                PRAGMA user_version = 13;
                ",
            )
            .expect("legacy schema");

        let store = SqliteMailStore::from_connection(connection).expect("migrated store");
        let account_id = AccountId::parse("work").expect("account id");
        assert_eq!(
            store
                .get(&account_id, "password")
                .expect("migrated password"),
            Some("existing-password".into())
        );
        assert_eq!(
            store.schema_version().expect("schema version"),
            CURRENT_SCHEMA_VERSION
        );
    }

    #[test]
    fn persists_incremental_remote_mailbox_state() {
        let mut store = SqliteMailStore::open_in_memory().expect("profile");
        let account = mail_account();
        store.save_mail_account(&account).expect("save account");
        let state = RemoteMailboxSyncState {
            account_id: account.id.clone(),
            remote_mailbox: "INBOX".into(),
            uid_validity: 42,
            uid_next: Some(8_192),
            highest_modseq: Some(u64::MAX - 7),
            delta_cursor: Some(
                "https://graph.microsoft.com/v1.0/me/mailFolders/x/messages/delta?$deltatoken=abc"
                    .into(),
            ),
            catalog_complete: true,
            last_full_reconcile_at_ms: 1_785_830_400_000,
        };

        store
            .save_remote_mailbox_sync_state(&state)
            .expect("save mailbox state");

        assert_eq!(
            store
                .remote_mailbox_sync_states(&account.id)
                .expect("load mailbox states"),
            [state]
        );
        store
            .delete_mail_account(&account.id)
            .expect("delete account");
        assert!(
            store
                .remote_mailbox_sync_states(&account.id)
                .expect("states after account deletion")
                .is_empty()
        );
    }

    #[test]
    fn removes_only_generation_matched_vanished_remote_messages() {
        let mut store = SqliteMailStore::open_in_memory().expect("profile");
        let mailbox = mailbox();
        let message = summary();
        let body = MessageBody {
            message_id: message.id.clone(),
            plain_text: Some("Nachricht mit späterer Serverlöschung".into()),
            sanitized_html: None,
        };
        let metadata = remote_metadata(message.id.clone());
        let stored_attachment = attachment(message.id.clone());
        store
            .save_mailboxes(std::slice::from_ref(&mailbox))
            .expect("mailbox");
        store
            .save_remote_message(
                &message,
                &body,
                &recipients(message.id.clone()),
                &metadata,
                std::slice::from_ref(&stored_attachment),
            )
            .expect("remote message");

        assert!(
            store
                .remove_vanished_remote_messages(
                    &metadata.account_id,
                    &metadata.remote_mailbox,
                    &[RemoteMessageIdentity::ImapUid {
                        uid_validity: 43,
                        uid: 7,
                    }],
                )
                .expect("different UIDVALIDITY")
                .is_empty()
        );
        assert!(
            store
                .remove_vanished_remote_messages(
                    &metadata.account_id,
                    &metadata.remote_mailbox,
                    &[RemoteMessageIdentity::ImapUid {
                        uid_validity: 42,
                        uid: 8,
                    }],
                )
                .expect("different UID")
                .is_empty()
        );
        assert_eq!(
            store
                .remove_vanished_remote_messages(
                    &metadata.account_id,
                    &metadata.remote_mailbox,
                    std::slice::from_ref(&metadata.identity),
                )
                .expect("matching VANISHED UID"),
            [stored_attachment]
        );
        assert!(
            store
                .list_messages(&mailbox.id, 20)
                .expect("mailbox after deletion")
                .is_empty()
        );
    }

    #[test]
    fn searches_message_content_and_keeps_the_index_current() {
        let mut store = SqliteMailStore::open_in_memory().expect("profile");
        let mailbox = mailbox();
        store
            .save_mailboxes(std::slice::from_ref(&mailbox))
            .expect("mailbox");
        let mut message = summary();
        let body = MessageBody {
            message_id: message.id.clone(),
            plain_text: Some("Der vertrauliche Verschlüsselungsbericht ist fertig.".into()),
            sanitized_html: None,
        };
        store.save_message(&message, &body).expect("message");

        let results = store
            .search_messages("verschlusselungsbericht", true, 20)
            .expect("body search");
        assert_eq!(results, vec![message.clone()]);
        assert!(
            store
                .search_messages("verschlusselungsbericht", false, 20)
                .expect("metadata-only search")
                .is_empty()
        );

        message.subject = "Roadmap".into();
        store
            .save_remote_message(
                &message,
                &body,
                &recipients(message.id.clone()),
                &remote_metadata(message.id.clone()),
                std::slice::from_ref(&attachment(message.id.clone())),
            )
            .expect("updated searchable metadata");
        assert_eq!(
            store
                .search_messages("roadm", false, 20)
                .expect("prefix search"),
            vec![message.clone()]
        );
        assert_eq!(
            store
                .search_messages("projekt@example", false, 20)
                .expect("recipient search"),
            vec![message.clone()]
        );
        assert!(
            store
                .search_messages("projektubersicht", false, 20)
                .expect("metadata search excludes attachments")
                .is_empty()
        );
        assert_eq!(
            store
                .search_messages("projektubersicht", true, 20)
                .expect("attachment-name search"),
            vec![message.clone()]
        );

        let mut body_only_message = summary();
        body_only_message.id = MessageId::parse("message-2").expect("message id");
        body_only_message.subject = "Interne Notiz".into();
        body_only_message.has_attachments = false;
        let body_only = MessageBody {
            message_id: body_only_message.id.clone(),
            plain_text: Some("Roadmap steht ausschließlich im Inhalt.".into()),
            sanitized_html: None,
        };
        store
            .save_message(&body_only_message, &body_only)
            .expect("body-only match");
        let ranked = store
            .search_messages("Roadmap", true, 20)
            .expect("weighted search");
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0], message);
        assert!(
            store
                .search_messages("***", false, 20)
                .expect("empty query")
                .is_empty()
        );
    }

    #[test]
    fn catalog_refresh_preserves_an_existing_cached_body_and_preview() {
        let mut store = SqliteMailStore::open_in_memory().expect("profile");
        let mailbox = mailbox();
        store
            .save_mailboxes(std::slice::from_ref(&mailbox))
            .expect("mailbox");
        let message = summary();
        let body = MessageBody {
            message_id: message.id.clone(),
            plain_text: Some("Bereits vollständig geladener Inhalt".into()),
            sanitized_html: None,
        };
        let participants = recipients(message.id.clone());
        let metadata = remote_metadata(message.id.clone());
        let stored_attachment = attachment(message.id.clone());
        store
            .save_remote_message(
                &message,
                &body,
                &participants,
                &metadata,
                std::slice::from_ref(&stored_attachment),
            )
            .expect("cached message");

        let mut catalog_summary = message.clone();
        catalog_summary.preview.clear();
        let empty_body = MessageBody {
            message_id: message.id.clone(),
            plain_text: None,
            sanitized_html: None,
        };
        let catalog_metadata = RemoteMessageMetadata {
            catalog_complete: true,
            body_requested: false,
            body_complete: false,
            ..metadata.clone()
        };
        store
            .save_remote_message(
                &catalog_summary,
                &empty_body,
                &participants,
                &catalog_metadata,
                std::slice::from_ref(&stored_attachment),
            )
            .expect("catalog refresh");

        assert_eq!(store.message_body(&message.id).expect("body"), body);
        assert_eq!(
            store.list_messages(&mailbox.id, 20).expect("messages")[0].preview,
            message.preview
        );
        assert_eq!(
            store
                .remote_message_metadata(&message.id)
                .expect("metadata"),
            metadata
        );
        assert_eq!(
            store.list_attachments(&message.id).expect("attachments"),
            vec![stored_attachment]
        );
    }

    #[test]
    #[ignore = "manual encrypted full-text search benchmark"]
    fn benchmarks_encrypted_full_text_search() {
        const MESSAGE_COUNT: u32 = 5_000;
        const SEARCH_COUNT: u32 = 100;

        let database = TemporaryDatabase::new();
        let mut store =
            SqliteMailStore::open_encrypted(&database.path, &[42; 32]).expect("encrypted profile");
        let mailbox = mailbox();
        store
            .save_mailboxes(std::slice::from_ref(&mailbox))
            .expect("mailbox");

        let indexing_started = Instant::now();
        for index in 0..MESSAGE_COUNT {
            let id = MessageId::parse(format!("benchmark.message.{index}"))
                .expect("benchmark message id");
            let summary = MessageSummary {
                id: id.clone(),
                account_id: mailbox.account_id.clone(),
                mailbox_id: mailbox.id.clone(),
                from: MailAddress::new(
                    format!("sender{index}@example.test"),
                    Some(format!("Benchmark Sender {index}")),
                )
                .expect("sender"),
                subject: format!("Projektbericht Nummer {index}"),
                preview: "Indizierbarer Vorschautext fuer die Leistungsmessung".into(),
                received_at_ms: i64::from(index),
                flags: Vec::new(),
                has_attachments: false,
            };
            let marker = if index % 100 == 0 {
                " quantennotiz"
            } else {
                ""
            };
            let body = MessageBody {
                message_id: id,
                plain_text: Some(format!(
                    "Verschluesselter Nachrichteninhalt {index}{marker}"
                )),
                sanitized_html: None,
            };
            store.save_message(&summary, &body).expect("message");
        }
        let indexing_elapsed = indexing_started.elapsed();

        let searching_started = Instant::now();
        for _ in 0..SEARCH_COUNT {
            let results = store
                .search_messages("quantennot", true, 100)
                .expect("full-text search");
            assert_eq!(results.len(), 50);
        }
        let searching_elapsed = searching_started.elapsed();
        eprintln!(
            "encrypted FTS benchmark: indexed {MESSAGE_COUNT} messages in {indexing_elapsed:?}; \
             {SEARCH_COUNT} searches in {searching_elapsed:?} ({:?} average)",
            searching_elapsed / SEARCH_COUNT
        );
    }

    #[test]
    fn upgrades_an_existing_v2_profile_without_losing_workspace_data() {
        let database = TemporaryDatabase::new();
        {
            let mut connection = Connection::open(&database.path).expect("open v2 database");
            apply_migration_v1(&mut connection).expect("migration v1");
            apply_migration_v2(&mut connection).expect("migration v2");
        }

        let store = SqliteMailStore::open(&database.path).expect("upgrade database");
        assert_eq!(
            store.schema_version().expect("schema version"),
            CURRENT_SCHEMA_VERSION
        );
        assert_eq!(store.list_tasks().expect("seeded tasks").len(), 3);
        assert_eq!(store.pending_mail_mutation_count(), Ok(0));
    }

    #[test]
    fn upgrades_v4_attachment_objects_to_optional_remote_metadata() {
        let database = TemporaryDatabase::new();
        {
            let mut connection = Connection::open(&database.path).expect("open v4 database");
            apply_migration_v1(&mut connection).expect("migration v1");
            apply_migration_v2(&mut connection).expect("migration v2");
            apply_migration_v3(&mut connection).expect("migration v3");
            apply_migration_v4(&mut connection).expect("migration v4");
            connection
                .execute_batch(
                    "
                    INSERT INTO mailboxes (
                        id, account_id, display_name, role, unread_count, total_count
                    ) VALUES ('v4.inbox', 'personal', 'Posteingang', 'inbox', 0, 1);
                    INSERT INTO mailboxes (
                        id, account_id, display_name, role, unread_count, total_count
                    ) VALUES ('v4.drafts', 'personal', 'Entwürfe', 'drafts', 0, 1);
                    INSERT INTO messages (
                        id, account_id, mailbox_id, from_address, subject, preview,
                        received_at_ms, flags, has_attachments
                    ) VALUES (
                        'v4.message', 'personal', 'v4.inbox', 'sender@example.org',
                        'Altbestand', 'Anhang', 1, 0, 1
                    );
                    INSERT INTO messages (
                        id, account_id, mailbox_id, from_address, subject, preview,
                        received_at_ms, flags, has_attachments
                    ) VALUES (
                        'local.v4-draft', 'personal', 'v4.drafts', 'sender@example.org',
                        'Alter Entwurf', 'Entwurf', 2, 8, 0
                    );
                    INSERT INTO message_attachments (
                        id, message_id, file_name, content_type, size_bytes, object_key
                    ) VALUES (
                        'attachment.v4', 'v4.message', 'alt.pdf', 'application/pdf',
                        123, 'attachments/attachment.v4.bin'
                    );
                    INSERT INTO remote_messages (
                        message_id, account_id, remote_mailbox, uid_validity, remote_uid
                    ) VALUES (
                        'v4.message', 'personal', 'Posteingang', 42, 7
                    );
                    ",
                )
                .expect("insert v4 attachment");
        }

        let store = SqliteMailStore::open(&database.path).expect("upgrade database");
        let stored = store
            .attachment(&AttachmentId::parse("attachment.v4").expect("attachment id"))
            .expect("migrated attachment");
        assert_eq!(
            stored.object_key.as_deref(),
            Some("attachments/attachment.v4.bin")
        );
        assert_eq!(stored.remote_section, None);
        assert_eq!(stored.transfer_encoding, None);
        assert!(
            store
                .remote_message_metadata(&MessageId::parse("v4.message").expect("message id"))
                .expect("migrated remote identity")
                .body_complete
        );
        let migrated_draft = store
            .local_draft_metadata(
                &MessageId::parse("local.v4-draft").expect("local draft message id"),
            )
            .expect("migrated local draft")
            .expect("editable metadata");
        assert!(migrated_draft.to.is_empty());
        assert!(migrated_draft.editor_delta_json.is_empty());
    }

    #[test]
    fn persists_and_loads_complete_mail_data() {
        let mut store = SqliteMailStore::open_in_memory().expect("open database");
        let mailbox = mailbox();
        let summary = summary();
        let body = MessageBody {
            message_id: summary.id.clone(),
            plain_text: Some("Hallo Anna".into()),
            sanitized_html: None,
        };

        store
            .save_mailboxes(std::slice::from_ref(&mailbox))
            .expect("save mailbox");
        store
            .save_summaries(std::slice::from_ref(&summary))
            .expect("save summary");
        store.save_body(&body).expect("save body");

        assert_eq!(
            store
                .list_mailboxes(&mailbox.account_id)
                .expect("load mailboxes"),
            vec![mailbox]
        );
        assert_eq!(
            store
                .list_messages(&summary.mailbox_id, 20)
                .expect("load summaries"),
            vec![summary.clone()]
        );
        assert_eq!(store.message_body(&summary.id).expect("load body"), body);
    }

    #[test]
    fn data_survives_closing_and_reopening_database_file() {
        let database = TemporaryDatabase::new();
        let mailbox = mailbox();

        {
            let mut store = SqliteMailStore::open(&database.path).expect("open database");
            store
                .save_mailboxes(std::slice::from_ref(&mailbox))
                .expect("save mailbox");
        }

        let reopened = SqliteMailStore::open(&database.path).expect("reopen database");
        assert_eq!(
            reopened
                .list_mailboxes(&mailbox.account_id)
                .expect("load mailboxes"),
            vec![mailbox]
        );
    }

    #[test]
    fn coordinates_operations_across_connections_to_the_same_profile() {
        let database = TemporaryDatabase::new();
        let first = SqliteMailStore::open(&database.path).expect("first connection");
        let second = SqliteMailStore::open(&database.path).expect("second connection");
        let first_operation = first.connection().expect("first operation");
        let (sender, receiver) = mpsc::channel();

        let worker = thread::spawn(move || {
            let result = second.schema_version();
            sender.send(result).expect("send result");
        });
        assert!(matches!(
            receiver.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));

        drop(first_operation);
        assert_eq!(
            receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("coordinated operation"),
            Ok(CURRENT_SCHEMA_VERSION)
        );
        worker.join().expect("worker");
    }

    #[test]
    fn upsert_replaces_mutable_message_fields() {
        let mut store = SqliteMailStore::open_in_memory().expect("open database");
        store.save_mailboxes(&[mailbox()]).expect("save mailbox");

        let mut message = summary();
        store
            .save_summaries(&[message.clone()])
            .expect("save summary");
        message.subject = "Aktualisierte Planung".into();
        message.flags.push(MessageFlag::Seen);
        store
            .save_summaries(&[message.clone()])
            .expect("update summary");

        let loaded = store
            .list_messages(&message.mailbox_id, 20)
            .expect("load summary");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].subject, message.subject);
        assert!(loaded[0].flags.contains(&MessageFlag::Seen));
        assert!(loaded[0].flags.contains(&MessageFlag::Flagged));
    }

    #[test]
    fn reports_missing_message_body() {
        let store = SqliteMailStore::open_in_memory().expect("open database");
        let missing = MessageId::parse("missing").expect("valid id");

        assert_eq!(
            store.message_body(&missing),
            Err(ApplicationError::NotFound)
        );
    }

    #[test]
    fn persists_message_and_mailbox_mutations_transactionally() {
        let mut store = SqliteMailStore::open_in_memory().expect("open database");
        let inbox = mailbox();
        let archive = archive_mailbox();
        let message = summary();
        store
            .save_mailboxes(&[inbox.clone(), archive.clone()])
            .expect("save mailboxes");
        store
            .save_summaries(std::slice::from_ref(&message))
            .expect("save message");

        store
            .update_message_state(&message.id, &archive.id, false, false)
            .expect("move and update flags");
        let archived = store.list_messages(&archive.id, 20).expect("load archive");
        assert_eq!(archived.len(), 1);
        assert!(archived[0].flags.contains(&MessageFlag::Seen));
        assert!(!archived[0].flags.contains(&MessageFlag::Flagged));

        let mailboxes = store
            .list_mailboxes(&inbox.account_id)
            .expect("load counts");
        let stored_inbox = mailboxes
            .iter()
            .find(|mailbox| mailbox.id == inbox.id)
            .expect("inbox");
        let stored_archive = mailboxes
            .iter()
            .find(|mailbox| mailbox.id == archive.id)
            .expect("archive");
        assert_eq!(
            (stored_inbox.total_count, stored_inbox.unread_count),
            (0, 0)
        );
        assert_eq!(
            (stored_archive.total_count, stored_archive.unread_count),
            (1, 0)
        );

        store
            .rename_mailbox(&archive.id, "Später")
            .expect("rename archive");
        store
            .delete_mailbox(&archive.id, &inbox.id)
            .expect("delete archive");
        assert_eq!(
            store
                .list_messages(&inbox.id, 20)
                .expect("load inbox")
                .len(),
            1
        );
        assert!(
            store
                .list_mailboxes(&inbox.account_id)
                .expect("load mailboxes")
                .iter()
                .all(|mailbox| mailbox.id != archive.id)
        );
    }

    #[test]
    fn persists_and_replaces_attachment_metadata_transactionally() {
        let mut store = SqliteMailStore::open_in_memory().expect("open database");
        let inbox = mailbox();
        let mut message = summary();
        let body = MessageBody {
            message_id: message.id.clone(),
            plain_text: Some("Attachment body".into()),
            sanitized_html: None,
        };
        let attachment = attachment(message.id.clone());
        store
            .save_mailboxes(std::slice::from_ref(&inbox))
            .expect("save mailbox");
        store
            .save_message_with_attachments(&message, &body, std::slice::from_ref(&attachment))
            .expect("save attachment metadata");

        assert_eq!(
            store.list_attachments(&message.id).expect("attachments"),
            vec![attachment.clone()]
        );
        assert_eq!(
            store.attachment(&attachment.id).expect("attachment"),
            attachment
        );

        message.has_attachments = false;
        store
            .save_message_with_attachments(&message, &body, &[])
            .expect("remove attachment metadata");
        assert!(
            store
                .list_attachments(&message.id)
                .expect("attachments")
                .is_empty()
        );

        message.has_attachments = true;
        let remote_attachment = MessageAttachment {
            id: AttachmentId::parse("attachment.remote.test").expect("attachment id"),
            message_id: message.id.clone(),
            file_name: "server.pdf".into(),
            content_type: "application/pdf".into(),
            size_bytes: 123,
            object_key: None,
            remote_section: Some("TEXT".into()),
            transfer_encoding: Some("base64".into()),
        };
        let metadata = remote_metadata(message.id.clone());
        store
            .save_remote_message(
                &message,
                &body,
                &recipients(message.id.clone()),
                &metadata,
                std::slice::from_ref(&remote_attachment),
            )
            .expect("save server attachment metadata");
        assert_eq!(
            store.list_attachments(&message.id).expect("attachments"),
            vec![remote_attachment.clone()]
        );
        assert_eq!(
            store
                .remote_message_metadata(&message.id)
                .expect("remote identity"),
            metadata
        );
        assert_remote_reconciliation(&mut store, &message, &metadata, remote_attachment);
    }

    #[test]
    fn remote_mutations_are_compacted_and_survive_reopening() {
        let database = TemporaryDatabase::new();
        let inbox = mailbox();
        let archive = archive_mailbox();
        let message = summary();
        let body = MessageBody {
            message_id: message.id.clone(),
            plain_text: Some("Persisted remote message".into()),
            sanitized_html: None,
        };

        {
            let mut store = SqliteMailStore::open(&database.path).expect("open database");
            store
                .save_mailboxes(&[inbox.clone(), archive.clone()])
                .expect("save mailboxes");
            store
                .save_remote_message(
                    &message,
                    &body,
                    &recipients(message.id.clone()),
                    &remote_metadata(message.id.clone()),
                    &[],
                )
                .expect("save remote message");
            assert_eq!(
                store
                    .update_message_state(&message.id, &archive.id, false, false)
                    .expect("queue first mutation"),
                1
            );
            assert_eq!(
                store
                    .update_message_state(&message.id, &inbox.id, true, true)
                    .expect("compact mutation"),
                1
            );
        }

        let mut reopened = SqliteMailStore::open(&database.path).expect("reopen database");
        let pending = reopened
            .pending_mail_mutations(&inbox.account_id)
            .expect("pending mutations");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].message_id, message.id);
        assert_eq!(pending[0].source_mailbox, "Posteingang");
        assert_eq!(pending[0].target_mailbox, None);
        assert!(!pending[0].seen);
        assert!(pending[0].flagged);

        reopened
            .complete_mail_mutation(&message.id, false)
            .expect("complete mutation");
        assert_eq!(reopened.pending_mail_mutation_count(), Ok(0));
        assert_eq!(
            reopened.message_body(&message.id).expect("message remains"),
            body
        );
    }

    #[test]
    fn completing_a_remote_move_removes_the_stale_local_identity() {
        let mut store = SqliteMailStore::open_in_memory().expect("open database");
        let inbox = mailbox();
        let archive = archive_mailbox();
        let message = summary();
        let body = MessageBody {
            message_id: message.id.clone(),
            plain_text: Some("Moved message".into()),
            sanitized_html: None,
        };
        store
            .save_mailboxes(&[inbox.clone(), archive.clone()])
            .expect("save mailboxes");
        store
            .save_remote_message(
                &message,
                &body,
                &recipients(message.id.clone()),
                &remote_metadata(message.id.clone()),
                &[],
            )
            .expect("save remote message");
        store
            .update_message_state(&message.id, &archive.id, false, false)
            .expect("queue move");

        store
            .complete_mail_mutation(&message.id, true)
            .expect("complete move");

        assert_eq!(store.pending_mail_mutation_count(), Ok(0));
        assert_eq!(
            store.message_body(&message.id),
            Err(ApplicationError::NotFound)
        );
        assert!(
            store
                .list_messages(&archive.id, 20)
                .expect("archive")
                .is_empty()
        );
    }

    #[test]
    fn moving_a_message_between_accounts_is_rejected() {
        let mut store = SqliteMailStore::open_in_memory().expect("open database");
        let inbox = mailbox();
        let other_mailbox = Mailbox {
            id: MailboxId::parse("other.archive").expect("mailbox id"),
            account_id: AccountId::parse("other").expect("account id"),
            display_name: "Archive".into(),
            remote_name: None,
            role: MailboxRole::Archive,
            unread_count: 0,
            total_count: 0,
        };
        let message = summary();
        store
            .save_mailboxes(&[inbox.clone(), other_mailbox.clone()])
            .expect("save mailboxes");
        store
            .save_summaries(std::slice::from_ref(&message))
            .expect("save message");

        let result = store.update_message_state(&message.id, &other_mailbox.id, false, false);
        assert!(matches!(result, Err(ApplicationError::Storage(_))));
        assert_eq!(store.list_messages(&inbox.id, 20).expect("inbox").len(), 1);
    }

    #[test]
    fn persists_ordered_and_explicitly_empty_favorite_mailboxes() {
        let database = TemporaryDatabase::new();
        let inbox = mailbox();
        let archive = archive_mailbox();
        {
            let mut store = SqliteMailStore::open(&database.path).expect("database");
            store
                .save_mailboxes(&[inbox.clone(), archive.clone()])
                .expect("mailboxes");
            assert_eq!(store.favorite_mailbox_ids(), Ok(None));
            store
                .save_favorite_mailbox_ids(&[archive.id.clone(), inbox.id.clone()])
                .expect("favorites");
        }

        let mut reopened = SqliteMailStore::open(&database.path).expect("reopen database");
        assert_eq!(
            reopened.favorite_mailbox_ids(),
            Ok(Some(vec![archive.id, inbox.id]))
        );
        reopened
            .save_favorite_mailbox_ids(&[])
            .expect("empty favorites");
        assert_eq!(reopened.favorite_mailbox_ids(), Ok(Some(Vec::new())));
        assert_eq!(
            reopened.save_favorite_mailbox_ids(&[
                MailboxId::parse("missing.folder").expect("mailbox id")
            ]),
            Err(ApplicationError::NotFound)
        );
    }

    #[test]
    fn persists_dark_mode_inside_workspace_preferences() {
        let database = TemporaryDatabase::new();
        {
            let mut store = SqliteMailStore::open(&database.path).expect("database");
            assert_eq!(store.dark_mode_enabled(), Ok(false));
            store.save_dark_mode_enabled(true).expect("dark mode");
        }

        let mut reopened = SqliteMailStore::open(&database.path).expect("reopen database");
        assert_eq!(reopened.dark_mode_enabled(), Ok(true));
        reopened.save_dark_mode_enabled(false).expect("light mode");
        assert_eq!(reopened.dark_mode_enabled(), Ok(false));
    }

    #[test]
    fn persists_workspace_items_and_account_configuration_across_restarts() {
        let database = TemporaryDatabase::new();
        let event = CalendarEvent {
            id: WorkspaceItemId::parse("local.event.persisted").expect("event id"),
            title: "Persistenter Termin".into(),
            starts_at_ms: 1_785_830_400_000,
            ends_at_ms: 1_785_834_000_000,
            location: Some("Lokal".into()),
        };
        let task = TaskItem {
            id: WorkspaceItemId::parse("local.task.persisted").expect("task id"),
            title: "Persistente Aufgabe".into(),
            due_at_ms: Some(1_785_916_800_000),
            completed: true,
        };
        let contact = Contact {
            id: WorkspaceItemId::parse("local.contact.persisted").expect("contact id"),
            name: "Testkontakt".into(),
            email: MailAddress::new("contact@example.org", Some("Testkontakt".into()))
                .expect("contact address"),
        };
        let account = mail_account();

        {
            let mut store = SqliteMailStore::open(&database.path).expect("open database");
            store.save_calendar_event(&event).expect("save event");
            store.save_task(&task).expect("save task");
            store.save_contact(&contact).expect("save contact");
            store
                .save_mail_account(&account)
                .expect("save mail account");
            store
                .update_account_last_sync(&account.id, 1_785_830_400_000)
                .expect("update sync time");
        }

        let reopened = SqliteMailStore::open(&database.path).expect("reopen database");
        assert!(
            reopened
                .list_calendar_events()
                .expect("events")
                .contains(&event)
        );
        assert!(reopened.list_tasks().expect("tasks").contains(&task));
        assert!(
            reopened
                .list_contacts()
                .expect("contacts")
                .contains(&contact)
        );
        let stored_account = reopened
            .list_mail_accounts()
            .expect("accounts")
            .into_iter()
            .find(|stored| stored.id == account.id)
            .expect("stored account");
        assert_eq!(stored_account.display_name, account.display_name);
        assert_eq!(stored_account.last_sync_at_ms, Some(1_785_830_400_000));
    }

    #[test]
    fn deleting_an_account_removes_its_cached_mail() {
        let mut store = SqliteMailStore::open_in_memory().expect("open database");
        let account = mail_account();
        let account_mailbox = Mailbox {
            id: MailboxId::parse("work-account.inbox").expect("mailbox id"),
            account_id: account.id.clone(),
            display_name: "Posteingang".into(),
            remote_name: None,
            role: MailboxRole::Inbox,
            unread_count: 0,
            total_count: 0,
        };
        let account_message_id = MessageId::parse("work-account.message.1").expect("message id");
        let account_message = MessageSummary {
            id: account_message_id.clone(),
            account_id: account.id.clone(),
            mailbox_id: account_mailbox.id.clone(),
            from: MailAddress::new("sender@example.org", Some("Sender".into())).expect("sender"),
            subject: "Cached message".into(),
            preview: "Cached".into(),
            received_at_ms: 1_785_830_400_000,
            flags: Vec::new(),
            has_attachments: false,
        };
        store.save_mail_account(&account).expect("save account");
        store
            .save_mailboxes(&[account_mailbox])
            .expect("save account mailbox");
        store
            .save_message(
                &account_message,
                &MessageBody {
                    message_id: account_message_id.clone(),
                    plain_text: Some("Cached".into()),
                    sanitized_html: None,
                },
            )
            .expect("save cached account message");

        store
            .delete_mail_account(&account.id)
            .expect("delete account");

        assert!(store.list_mail_accounts().expect("accounts").is_empty());
        assert!(
            store
                .list_mailboxes(&account.id)
                .expect("account mailboxes")
                .is_empty()
        );
        assert_eq!(
            store.message_body(&account_message_id),
            Err(ApplicationError::NotFound)
        );
    }
}
