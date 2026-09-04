//! Use-case boundaries connecting the MAICENTA domain to external adapters.

use std::{error::Error, fmt, future::Future};

use maicenta_domain::{
    AccountId, AttachmentId, CalendarEvent, Contact, MailAccount, Mailbox, MailboxId,
    MessageAttachment, MessageBody, MessageFlag, MessageId, MessageRecipients, MessageSummary,
    TaskItem,
};

/// Provider-side identity of one message inside its remote mailbox.
///
/// IMAP addresses a message by its UID, which is only meaningful together with
/// the mailbox UIDVALIDITY generation. API-based providers such as Microsoft
/// Graph assign an opaque, stable string identifier instead. Storage and
/// synchronization treat both forms as one value so a connector never has to
/// emulate the other's numbering.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum RemoteMessageIdentity {
    /// IMAP UID inside one UIDVALIDITY generation.
    ImapUid { uid_validity: u32, uid: u32 },
    /// Opaque provider-assigned identifier (for example a Graph message ID).
    ProviderId(String),
}

impl RemoteMessageIdentity {
    /// Whether the identity carries enough information to address a message.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        match self {
            Self::ImapUid { uid, .. } => *uid != 0,
            Self::ProviderId(id) => !id.trim().is_empty() && id.len() <= 1_024,
        }
    }

    /// Returns the IMAP UID pair when this is an IMAP identity.
    #[must_use]
    pub fn imap_uid(&self) -> Option<(u32, u32)> {
        match self {
            Self::ImapUid { uid_validity, uid } => Some((*uid_validity, *uid)),
            Self::ProviderId(_) => None,
        }
    }

    /// Returns the opaque provider identifier when this is an API identity.
    #[must_use]
    pub fn provider_id(&self) -> Option<&str> {
        match self {
            Self::ProviderId(id) => Some(id),
            Self::ImapUid { .. } => None,
        }
    }
}

/// Stable remote identity attached to one locally cached message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteMessageMetadata {
    pub message_id: MessageId,
    pub account_id: AccountId,
    /// IMAP mailbox name or opaque provider folder ID.
    pub remote_mailbox: String,
    pub identity: RemoteMessageIdentity,
    /// Whether sender, recipients, subject, and attachment metadata were
    /// obtained from a complete bounded header/BODYSTRUCTURE fetch.
    pub catalog_complete: bool,
    /// True when a body download was attempted or explicitly requested. A
    /// header-only catalogue entry keeps this false and is not retried during
    /// every routine synchronization.
    pub body_requested: bool,
    pub body_complete: bool,
}

/// Persistent incremental state for one remote mailbox.
///
/// IMAP mailboxes use the UIDVALIDITY/UIDNEXT/HIGHESTMODSEQ triple. API-based
/// providers keep an opaque delta cursor instead and leave the IMAP fields at
/// their neutral values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteMailboxSyncState {
    pub account_id: AccountId,
    pub remote_mailbox: String,
    pub uid_validity: u32,
    pub uid_next: Option<u32>,
    pub highest_modseq: Option<u64>,
    /// Opaque provider continuation cursor, for example a Graph `deltaLink`
    /// or the `nextLink` of an unfinished initial delta round.
    pub delta_cursor: Option<String>,
    pub catalog_complete: bool,
    pub last_full_reconcile_at_ms: i64,
}

/// Editable composition state retained for one locally created draft.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalDraftMetadata {
    pub message_id: MessageId,
    pub to: String,
    pub cc: String,
    pub bcc: String,
    /// Serialized Quill Delta used to reopen the rich-text editor losslessly.
    pub editor_delta_json: String,
}

/// Desired remote state compacted from one or more local user actions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingMailMutation {
    pub message_id: MessageId,
    pub account_id: AccountId,
    pub source_mailbox: String,
    pub target_mailbox: Option<String>,
    pub identity: RemoteMessageIdentity,
    pub seen: bool,
    pub flagged: bool,
}

/// Remote work retained for a locally editable server draft.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PendingDraftAction {
    Upsert,
    Delete,
}

/// One durable draft operation, including the server identity that an edit or
/// send must remove before the current local state is uploaded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingDraftOperation {
    pub message_id: MessageId,
    pub account_id: AccountId,
    pub target_mailbox: String,
    pub action: PendingDraftAction,
    pub previous_remote: Option<RemoteMessageMetadata>,
}

/// Error category shared across application ports.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApplicationError {
    AuthenticationRequired,
    ConnectionUnavailable,
    NotFound,
    PermissionDenied,
    Storage(String),
    Provider(String),
}

impl fmt::Display for ApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthenticationRequired => formatter.write_str("authentication required"),
            Self::ConnectionUnavailable => formatter.write_str("connection unavailable"),
            Self::NotFound => formatter.write_str("item not found"),
            Self::PermissionDenied => formatter.write_str("permission denied"),
            Self::Storage(message) => write!(formatter, "storage error: {message}"),
            Self::Provider(message) => write!(formatter, "provider error: {message}"),
        }
    }
}

impl Error for ApplicationError {}

/// Local persistence contract used by mail use cases.
///
/// Production uses `SQLite`; tests can use an in-memory adapter.
pub trait MailStore: Send + Sync {
    /// Lists locally known mailboxes for an account.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the local data cannot be read.
    fn list_mailboxes(&self, account_id: &AccountId) -> Result<Vec<Mailbox>, ApplicationError>;

    /// Lists locally cached message summaries, newest first.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the local data cannot be read.
    fn list_messages(
        &self,
        mailbox_id: &MailboxId,
        limit: usize,
    ) -> Result<Vec<MessageSummary>, ApplicationError>;

    /// Lists one page of locally cached message summaries, newest first.
    ///
    /// Adapters may override this for an efficient database `OFFSET`. The
    /// default keeps existing in-memory adapters source-compatible.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the local data cannot be read.
    fn list_message_page(
        &self,
        mailbox_id: &MailboxId,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<MessageSummary>, ApplicationError> {
        let requested = offset
            .checked_add(limit)
            .ok_or_else(|| ApplicationError::Storage("message page range is too large".into()))?;
        Ok(self
            .list_messages(mailbox_id, requested)?
            .into_iter()
            .skip(offset)
            .collect())
    }

    /// Loads one locally cached message summary.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError::NotFound`] when the message is unknown, or
    /// a storage error when the local data cannot be read.
    fn message_summary(&self, message_id: &MessageId) -> Result<MessageSummary, ApplicationError>;

    /// Searches the encrypted profile catalogue, ordered by field-aware
    /// relevance and recency. Body, preview, and attachment-name matching are
    /// included only when `include_content` is true.
    ///
    /// # Errors
    ///
    /// Returns a validation or storage error when the encrypted search index
    /// cannot execute the query.
    fn search_messages(
        &self,
        query: &str,
        include_content: bool,
        limit: usize,
    ) -> Result<Vec<MessageSummary>, ApplicationError>;

    /// Loads the cached body for one message.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError::NotFound`] when the body is not cached, or
    /// a storage error when local data cannot be read.
    fn message_body(&self, message_id: &MessageId) -> Result<MessageBody, ApplicationError>;

    /// Loads the cached envelope recipients for one message.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError::NotFound`] when no envelope is cached, or
    /// a storage error when local data cannot be read.
    fn message_recipients(
        &self,
        message_id: &MessageId,
    ) -> Result<MessageRecipients, ApplicationError>;

    /// Lists attachment metadata for one cached message.
    ///
    /// # Errors
    ///
    /// Returns a storage error when attachment metadata cannot be read.
    fn list_attachments(
        &self,
        message_id: &MessageId,
    ) -> Result<Vec<MessageAttachment>, ApplicationError>;

    /// Loads one attachment metadata record.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError::NotFound`] when the attachment is unknown.
    fn attachment(
        &self,
        attachment_id: &AttachmentId,
    ) -> Result<MessageAttachment, ApplicationError>;

    /// Persists mailbox metadata atomically.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the transaction cannot be committed.
    fn save_mailboxes(&mut self, mailboxes: &[Mailbox]) -> Result<(), ApplicationError>;

    /// Persists message summaries atomically.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the transaction cannot be committed.
    fn save_summaries(&mut self, messages: &[MessageSummary]) -> Result<(), ApplicationError>;

    /// Persists a sanitized message body.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the body cannot be committed.
    fn save_body(&mut self, body: &MessageBody) -> Result<(), ApplicationError>;

    /// Persists a message summary and sanitized body in one transaction.
    ///
    /// # Errors
    ///
    /// Returns a storage error when either part cannot be committed.
    fn save_message(
        &mut self,
        summary: &MessageSummary,
        body: &MessageBody,
    ) -> Result<(), ApplicationError>;

    /// Persists a summary, sanitized body, and attachment metadata in one
    /// database transaction.
    ///
    /// # Errors
    ///
    /// Returns a storage error when any related record cannot be committed.
    fn save_message_with_attachments(
        &mut self,
        summary: &MessageSummary,
        body: &MessageBody,
        attachments: &[MessageAttachment],
    ) -> Result<(), ApplicationError>;

    /// Updates the mailbox and user-controlled read/flagged state of a message.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError::NotFound`] when the message does not exist,
    /// or a storage error when the transaction cannot be committed.
    fn update_message_state(
        &mut self,
        message_id: &MessageId,
        mailbox_id: &MailboxId,
        unread: bool,
        flagged: bool,
    ) -> Result<u32, ApplicationError>;

    /// Renames one locally stored mailbox.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError::NotFound`] when the mailbox does not exist,
    /// or a storage error when the change cannot be committed.
    fn rename_mailbox(
        &mut self,
        mailbox_id: &MailboxId,
        display_name: &str,
    ) -> Result<(), ApplicationError>;

    /// Deletes a mailbox after moving its messages into `fallback_mailbox_id`.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError::NotFound`] when either mailbox does not
    /// exist, or a storage error when the transaction cannot be committed.
    fn delete_mailbox(
        &mut self,
        mailbox_id: &MailboxId,
        fallback_mailbox_id: &MailboxId,
    ) -> Result<(), ApplicationError>;
}

/// Persistence contract for locally editable message drafts.
pub trait LocalDraftStore: Send + Sync {
    /// Loads editable metadata for a local draft, if available.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the metadata cannot be read.
    fn local_draft_metadata(
        &self,
        message_id: &MessageId,
    ) -> Result<Option<LocalDraftMetadata>, ApplicationError>;

    /// Saves a local message and replaces or removes its editable draft
    /// metadata in the same transaction.
    ///
    /// # Errors
    ///
    /// Returns a storage error when any related record cannot be committed.
    fn save_local_message(
        &mut self,
        summary: &MessageSummary,
        body: &MessageBody,
        recipients: &MessageRecipients,
        attachments: &[MessageAttachment],
        draft: Option<&LocalDraftMetadata>,
    ) -> Result<(), ApplicationError>;

    /// Retains editable metadata for a fully cached server draft without
    /// enqueuing another upload of the unchanged remote message.
    ///
    /// # Errors
    ///
    /// Returns a validation or storage error when the message is not a cached
    /// remote draft or the metadata cannot be committed.
    fn save_synchronized_draft_metadata(
        &mut self,
        draft: &LocalDraftMetadata,
    ) -> Result<(), ApplicationError>;
}

/// Persistence used by the IMAP synchronization adapter.
pub trait MailSyncStore: Send + Sync {
    /// Lists incremental synchronization states for one account.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the checkpoint table cannot be read.
    fn remote_mailbox_sync_states(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<RemoteMailboxSyncState>, ApplicationError>;

    /// Creates or replaces one mailbox synchronization state.
    ///
    /// # Errors
    ///
    /// Returns a validation or storage error when the state cannot be saved.
    fn save_remote_mailbox_sync_state(
        &mut self,
        state: &RemoteMailboxSyncState,
    ) -> Result<(), ApplicationError>;

    /// Lists all stable server identities cached for one account.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the metadata cannot be read.
    fn remote_messages_for_account(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<RemoteMessageMetadata>, ApplicationError>;

    /// Loads the stable server identity recorded for one cached message.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError::NotFound`] when the message has no remote
    /// identity, or a storage error when the metadata cannot be read.
    fn remote_message_metadata(
        &self,
        message_id: &MessageId,
    ) -> Result<RemoteMessageMetadata, ApplicationError>;

    /// Persists a remote summary, safe body, IMAP identity, and any locally
    /// cached attachment metadata atomically.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the transaction cannot be committed.
    fn save_remote_message(
        &mut self,
        summary: &MessageSummary,
        body: &MessageBody,
        recipients: &MessageRecipients,
        metadata: &RemoteMessageMetadata,
        attachments: &[MessageAttachment],
    ) -> Result<(), ApplicationError>;

    /// Applies a server-originated flag snapshot to one cached message.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError::NotFound`] when the remote identity no
    /// longer exists, or a storage error when the update cannot be committed.
    fn update_remote_message_flags(
        &mut self,
        message_id: &MessageId,
        flags: &[MessageFlag],
    ) -> Result<(), ApplicationError>;

    /// Removes cached remote messages whose identity is no longer present in
    /// the complete mailbox snapshot.
    ///
    /// An IMAP row from another UIDVALIDITY generation never equals an active
    /// identity and is therefore removed as well. Returns attachment metadata
    /// for object cleanup after the transaction.
    ///
    /// # Errors
    ///
    /// Returns a storage error when reconciliation cannot be committed.
    fn reconcile_remote_mailbox(
        &mut self,
        account_id: &AccountId,
        remote_mailbox: &str,
        active_identities: &[RemoteMessageIdentity],
    ) -> Result<Vec<MessageAttachment>, ApplicationError>;

    /// Removes exact remote identities confirmed as vanished by the provider,
    /// for example through QRESYNC `VANISHED` or a Graph delta removal.
    ///
    /// Returns attachment metadata for object cleanup after the transaction.
    /// Identities from another UIDVALIDITY generation are never matched.
    ///
    /// # Errors
    ///
    /// Returns a validation or storage error when the deletion cannot be
    /// committed.
    fn remove_vanished_remote_messages(
        &mut self,
        account_id: &AccountId,
        remote_mailbox: &str,
        vanished_identities: &[RemoteMessageIdentity],
    ) -> Result<Vec<MessageAttachment>, ApplicationError>;

    /// Lists compacted pending mutations for one account.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the queue cannot be read.
    fn pending_mail_mutations(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<PendingMailMutation>, ApplicationError>;

    /// Completes one mutation, optionally removing a locally moved message so
    /// its new server UID can be downloaded cleanly.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError::NotFound`] when no queued mutation exists.
    fn complete_mail_mutation(
        &mut self,
        message_id: &MessageId,
        remove_local_message: bool,
    ) -> Result<(), ApplicationError>;

    /// Lists queued draft uploads and deletions for one account.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the durable queue cannot be read.
    fn pending_draft_operations(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<PendingDraftOperation>, ApplicationError>;

    /// Completes one draft operation. An uploaded draft receives its new
    /// server identity; a deletion removes the obsolete identity.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError::NotFound`] when no operation is queued, or
    /// a storage error when the transaction cannot be committed.
    fn complete_draft_operation(
        &mut self,
        message_id: &MessageId,
        uploaded_remote: Option<&RemoteMessageMetadata>,
    ) -> Result<(), ApplicationError>;

    /// Counts all pending remote mail mutations and draft operations.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the queue cannot be read.
    fn pending_mail_mutation_count(&self) -> Result<u32, ApplicationError>;
}

/// Local persistence contract for the personal workspace modules.
pub trait WorkspaceStore: Send + Sync {
    /// Loads whether the profile explicitly uses the dark color scheme.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the preference cannot be read or decoded.
    fn dark_mode_enabled(&self) -> Result<bool, ApplicationError>;

    /// Persists the profile's dark-color-scheme preference.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the preference cannot be committed.
    fn save_dark_mode_enabled(&mut self, enabled: bool) -> Result<(), ApplicationError>;

    /// Loads the user's ordered favorite mailboxes. `None` means the profile
    /// has not initialized this preference yet.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the preference cannot be read or decoded.
    fn favorite_mailbox_ids(&self) -> Result<Option<Vec<MailboxId>>, ApplicationError>;

    /// Replaces the complete ordered favorite-mailbox preference.
    ///
    /// # Errors
    ///
    /// Returns a validation or storage error when an identifier is unknown,
    /// duplicated, or the preference cannot be committed.
    fn save_favorite_mailbox_ids(
        &mut self,
        mailbox_ids: &[MailboxId],
    ) -> Result<(), ApplicationError>;

    /// Loads the mailboxes whose folder subtree the user collapsed.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the preference cannot be read or decoded.
    fn collapsed_mailbox_ids(&self) -> Result<Vec<MailboxId>, ApplicationError>;

    /// Replaces the set of collapsed mailboxes.
    ///
    /// # Errors
    ///
    /// Returns a validation or storage error when the set is too large,
    /// contains duplicates, or cannot be committed.
    fn save_collapsed_mailbox_ids(
        &mut self,
        mailbox_ids: &[MailboxId],
    ) -> Result<(), ApplicationError>;

    /// Loads calendar events ordered by their start time.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the data cannot be read.
    fn list_calendar_events(&self) -> Result<Vec<CalendarEvent>, ApplicationError>;

    /// Creates or updates a local calendar event.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the change cannot be committed.
    fn save_calendar_event(&mut self, event: &CalendarEvent) -> Result<(), ApplicationError>;

    /// Loads tasks, with unfinished tasks first.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the data cannot be read.
    fn list_tasks(&self) -> Result<Vec<TaskItem>, ApplicationError>;

    /// Creates or updates a local task.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the change cannot be committed.
    fn save_task(&mut self, task: &TaskItem) -> Result<(), ApplicationError>;

    /// Loads contacts ordered by display name.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the data cannot be read.
    fn list_contacts(&self) -> Result<Vec<Contact>, ApplicationError>;

    /// Creates or updates a local contact.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the change cannot be committed.
    fn save_contact(&mut self, contact: &Contact) -> Result<(), ApplicationError>;
}

/// Local persistence contract for mail account configuration.
///
/// Secrets remain behind [`SecretStore`] and are never exposed through account
/// snapshots, even when both are backed by the same encrypted profile vault.
pub trait MailAccountStore: Send + Sync {
    /// Lists configured mail accounts.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the configuration cannot be read.
    fn list_mail_accounts(&self) -> Result<Vec<MailAccount>, ApplicationError>;

    /// Creates or updates account settings without changing its secret.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the settings cannot be committed.
    fn save_mail_account(&mut self, account: &MailAccount) -> Result<(), ApplicationError>;

    /// Deletes one account configuration and all of its locally cached mail.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError::NotFound`] when the account is missing, or
    /// a storage error when the transaction cannot be committed.
    fn delete_mail_account(&mut self, account_id: &AccountId) -> Result<(), ApplicationError>;

    /// Records the completion time of a successful synchronization.
    ///
    /// # Errors
    ///
    /// Returns [`ApplicationError::NotFound`] when the account is missing, or
    /// a storage error when the timestamp cannot be committed.
    fn update_account_last_sync(
        &mut self,
        account_id: &AccountId,
        timestamp_ms: i64,
    ) -> Result<(), ApplicationError>;
}

/// Remote mail-provider contract used by synchronization.
///
/// Implementations may use IMAP initially and JMAP or provider-specific APIs
/// later. Provider credentials are supplied by a secret-store adapter, never
/// embedded in this interface or persisted in domain models.
pub trait MailConnector: Send + Sync {
    fn fetch_mailboxes(
        &self,
        account_id: &AccountId,
    ) -> impl Future<Output = Result<Vec<Mailbox>, ApplicationError>> + Send;

    fn fetch_summaries(
        &self,
        mailbox: &Mailbox,
        since_ms: Option<i64>,
    ) -> impl Future<Output = Result<Vec<MessageSummary>, ApplicationError>> + Send;

    fn fetch_body(
        &self,
        message_id: &MessageId,
    ) -> impl Future<Output = Result<MessageBody, ApplicationError>> + Send;

    fn set_flags(
        &self,
        message_id: &MessageId,
        flags: &[MessageFlag],
    ) -> impl Future<Output = Result<(), ApplicationError>> + Send;
}

/// Secret lookup boundary implemented inside the encrypted profile vault.
///
/// The operating-system credential store protects only the profile master key;
/// account passwords and provider tokens remain in portable encrypted storage.
pub trait SecretStore: Send + Sync {
    /// Reads a secret for an account.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the encrypted secret cannot be read.
    fn get(&self, account_id: &AccountId, key: &str) -> Result<Option<String>, ApplicationError>;

    /// Creates or replaces a secret for an account.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the encrypted secret cannot be written.
    fn set(
        &mut self,
        account_id: &AccountId,
        key: &str,
        value: &str,
    ) -> Result<(), ApplicationError>;

    /// Removes a secret from the encrypted profile.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the encrypted secret cannot be removed.
    fn remove(&mut self, account_id: &AccountId, key: &str) -> Result<(), ApplicationError>;
}

#[cfg(test)]
mod tests {
    use maicenta_domain::{AccountId, Mailbox, MailboxId, MailboxRole};

    use super::{ApplicationError, MailStore};

    #[derive(Default)]
    struct MemoryMailStore {
        mailboxes: Vec<Mailbox>,
    }

    impl MailStore for MemoryMailStore {
        fn list_mailboxes(&self, account_id: &AccountId) -> Result<Vec<Mailbox>, ApplicationError> {
            Ok(self
                .mailboxes
                .iter()
                .filter(|mailbox| &mailbox.account_id == account_id)
                .cloned()
                .collect())
        }

        fn list_messages(
            &self,
            _mailbox_id: &MailboxId,
            _limit: usize,
        ) -> Result<Vec<maicenta_domain::MessageSummary>, ApplicationError> {
            Ok(Vec::new())
        }

        fn message_summary(
            &self,
            _message_id: &maicenta_domain::MessageId,
        ) -> Result<maicenta_domain::MessageSummary, ApplicationError> {
            Err(ApplicationError::NotFound)
        }

        fn search_messages(
            &self,
            _query: &str,
            _include_content: bool,
            _limit: usize,
        ) -> Result<Vec<maicenta_domain::MessageSummary>, ApplicationError> {
            Ok(Vec::new())
        }

        fn message_body(
            &self,
            _message_id: &maicenta_domain::MessageId,
        ) -> Result<maicenta_domain::MessageBody, ApplicationError> {
            Err(ApplicationError::NotFound)
        }

        fn message_recipients(
            &self,
            _message_id: &maicenta_domain::MessageId,
        ) -> Result<maicenta_domain::MessageRecipients, ApplicationError> {
            Err(ApplicationError::NotFound)
        }

        fn list_attachments(
            &self,
            _message_id: &maicenta_domain::MessageId,
        ) -> Result<Vec<maicenta_domain::MessageAttachment>, ApplicationError> {
            Ok(Vec::new())
        }

        fn attachment(
            &self,
            _attachment_id: &maicenta_domain::AttachmentId,
        ) -> Result<maicenta_domain::MessageAttachment, ApplicationError> {
            Err(ApplicationError::NotFound)
        }

        fn save_mailboxes(&mut self, mailboxes: &[Mailbox]) -> Result<(), ApplicationError> {
            self.mailboxes.extend_from_slice(mailboxes);
            Ok(())
        }

        fn save_summaries(
            &mut self,
            _messages: &[maicenta_domain::MessageSummary],
        ) -> Result<(), ApplicationError> {
            Ok(())
        }

        fn save_body(
            &mut self,
            _body: &maicenta_domain::MessageBody,
        ) -> Result<(), ApplicationError> {
            Ok(())
        }

        fn save_message(
            &mut self,
            _summary: &maicenta_domain::MessageSummary,
            _body: &maicenta_domain::MessageBody,
        ) -> Result<(), ApplicationError> {
            Ok(())
        }

        fn save_message_with_attachments(
            &mut self,
            _summary: &maicenta_domain::MessageSummary,
            _body: &maicenta_domain::MessageBody,
            _attachments: &[maicenta_domain::MessageAttachment],
        ) -> Result<(), ApplicationError> {
            Ok(())
        }

        fn update_message_state(
            &mut self,
            _message_id: &maicenta_domain::MessageId,
            _mailbox_id: &MailboxId,
            _unread: bool,
            _flagged: bool,
        ) -> Result<u32, ApplicationError> {
            Ok(0)
        }

        fn rename_mailbox(
            &mut self,
            _mailbox_id: &MailboxId,
            _display_name: &str,
        ) -> Result<(), ApplicationError> {
            Ok(())
        }

        fn delete_mailbox(
            &mut self,
            _mailbox_id: &MailboxId,
            _fallback_mailbox_id: &MailboxId,
        ) -> Result<(), ApplicationError> {
            Ok(())
        }
    }

    #[test]
    fn mail_store_port_can_be_backed_by_memory() {
        let account = AccountId::parse("personal").expect("valid id");
        let mut store = MemoryMailStore::default();
        store
            .save_mailboxes(&[Mailbox {
                id: MailboxId::parse("inbox").expect("valid id"),
                account_id: account.clone(),
                display_name: "Inbox".into(),
                remote_name: Some("INBOX".into()),
                role: MailboxRole::Inbox,
                unread_count: 0,
                total_count: 0,
            }])
            .expect("save mailbox");

        assert_eq!(store.list_mailboxes(&account).expect("list").len(), 1);
    }
}
