use std::{
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

use maicenta_application::{
    LocalDraftMetadata, LocalDraftStore, MailAccountStore, MailStore, MailSyncStore,
    PendingDraftAction, RemoteMailboxSyncState, RemoteMessageMetadata, SecretStore, WorkspaceStore,
};
use maicenta_domain::{
    AccountId, AttachmentId, CalendarEvent, Contact, MailAccount, MailAddress, Mailbox, MailboxId,
    MailboxRole, MessageAttachment, MessageBody, MessageFlag, MessageId, MessageRecipients,
    MessageSummary, TaskItem, TransportSecurity, WorkspaceItemId,
};
use maicenta_mail_connector::{
    ConnectorError, KnownRemoteMessage, MailCredential, OutgoingAttachment, OutgoingMessage,
    RemoteDraftIdentity, RemoteDraftOperation, RemoteMailboxCheckpoint, RemoteMutation,
    apply_draft_operations, apply_mailbox_mutations, delete_legacy_password,
    download_attachment_part, download_message_content, load_legacy_password,
    send_message as send_smtp_message, stable_remote_key, synchronize_mailboxes, test_account,
    wait_for_mailbox_change,
};
use maicenta_rendering::{
    MessageRenderer, RenderPolicy, RenderedMessage, decode_attachment_part,
    safe_attachment_file_name, sanitize_composed_html,
};
use maicenta_storage::SqliteMailStore;
use maicenta_vault::{ProfileVault, stage_archive};
use zeroize::Zeroizing;

const DEMO_ACCOUNT_ID: &str = "personal";
const DEMO_INBOX_ID: &str = "personal.inbox";
const MAX_OUTGOING_ATTACHMENTS: usize = 10;
const MAX_OUTGOING_ATTACHMENT_BYTES: u64 = 18 * 1024 * 1024;
const MAX_ON_DEMAND_ATTACHMENT_BYTES: usize = 100 * 1024 * 1024;
const INITIAL_MESSAGES_PER_MAILBOX: usize = 100;
const MAX_MESSAGE_PAGE_SIZE: usize = 200;
// QRESYNC-aware servers report deletions incrementally. Other servers need a
// bounded UID SEARCH ALL pass; doing that every 15 minutes keeps removals
// reasonably fresh without rescanning large folders on every five-minute poll.
const FULL_MAILBOX_RECONCILE_INTERVAL_MS: i64 = 15 * 60 * 1_000;
const ACCOUNT_PASSWORD_KEY: &str = "password";
const OAUTH_PROVIDER_KEY: &str = "oauth.provider";
const OAUTH_CLIENT_ID_KEY: &str = "oauth.client_id";
const OAUTH_ACCESS_TOKEN_KEY: &str = "oauth.access_token";
const OAUTH_REFRESH_TOKEN_KEY: &str = "oauth.refresh_token";
const OAUTH_EXPIRES_AT_KEY: &str = "oauth.expires_at_ms";
const OAUTH_TOKEN_ENDPOINT_KEY: &str = "oauth.token_endpoint";
const OAUTH_SCOPES_KEY: &str = "oauth.scopes";
const GOOGLE_TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
const MICROSOFT_TOKEN_ENDPOINT: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";
const OAUTH_REFRESH_SKEW_MS: i64 = 120_000;

static PROFILE_VAULTS: OnceLock<Mutex<HashMap<PathBuf, Arc<ProfileVault>>>> = OnceLock::new();

#[flutter_rust_bridge::frb(init)]
pub fn init_app() {
    flutter_rust_bridge::setup_default_user_utils();
}

fn profile_vault(database_path: &Path) -> Result<Arc<ProfileVault>, String> {
    let cache = PROFILE_VAULTS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(vault) = cache
        .lock()
        .map_err(|_| "profile vault cache was poisoned".to_owned())?
        .get(database_path)
        .cloned()
    {
        return Ok(vault);
    }

    #[cfg(not(test))]
    let vault = ProfileVault::open(database_path).map_err(|error| error.to_string())?;
    #[cfg(test)]
    let vault = ProfileVault::for_test(
        format!(
            "{:032x}",
            stable_remote_key(&database_path.to_string_lossy())
                .bytes()
                .fold(0_u128, |value, byte| (value << 4) ^ u128::from(byte))
        ),
        [0x5a; 32],
    );
    let vault = Arc::new(vault);
    cache
        .lock()
        .map_err(|_| "profile vault cache was poisoned".to_owned())?
        .insert(database_path.to_owned(), Arc::clone(&vault));
    Ok(vault)
}

fn replace_cached_vault(database_path: &Path, vault: ProfileVault) -> Result<(), String> {
    PROFILE_VAULTS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "profile vault cache was poisoned".to_owned())?
        .insert(database_path.to_owned(), Arc::new(vault));
    Ok(())
}

fn open_profile_store(database_path: impl AsRef<Path>) -> Result<SqliteMailStore, String> {
    let database_path = database_path.as_ref();
    let vault = profile_vault(database_path)?;
    SqliteMailStore::open_encrypted(database_path, vault.key().as_bytes())
        .map_err(|error| error.to_string())
}

enum StoredMailCredential {
    Password(Zeroizing<String>),
    OAuth2(Zeroizing<String>),
}

impl StoredMailCredential {
    fn connector_credential(&self) -> MailCredential<'_> {
        match self {
            Self::Password(password) => MailCredential::Password(password.as_str()),
            Self::OAuth2(access_token) => MailCredential::OAuth2AccessToken(access_token.as_str()),
        }
    }
}

fn required_secret(
    store: &SqliteMailStore,
    account_id: &AccountId,
    key: &str,
    description: &str,
) -> Result<String, String> {
    store
        .get(account_id, key)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("no {description} is stored in the encrypted profile"))
}

async fn load_account_credential(
    database_path: &Path,
    account_id: &AccountId,
) -> Result<StoredMailCredential, String> {
    let store = open_profile_store(database_path)?;
    let access_token = store
        .get(account_id, OAUTH_ACCESS_TOKEN_KEY)
        .map_err(|error| error.to_string())?;
    let Some(access_token) = access_token else {
        return required_secret(&store, account_id, ACCOUNT_PASSWORD_KEY, "password")
            .map(Zeroizing::new)
            .map(StoredMailCredential::Password);
    };
    let expires_at_ms = required_secret(
        &store,
        account_id,
        OAUTH_EXPIRES_AT_KEY,
        "OAuth token expiry",
    )?
    .parse::<i64>()
    .map_err(|_| "the stored OAuth token expiry is invalid".to_owned())?;
    if expires_at_ms > current_timestamp_ms()?.saturating_add(OAUTH_REFRESH_SKEW_MS) {
        return Ok(StoredMailCredential::OAuth2(Zeroizing::new(access_token)));
    }
    let client_id = required_secret(&store, account_id, OAUTH_CLIENT_ID_KEY, "OAuth client ID")?;
    let refresh_token = required_secret(
        &store,
        account_id,
        OAUTH_REFRESH_TOKEN_KEY,
        "OAuth refresh token",
    )?;
    let token_endpoint = required_secret(
        &store,
        account_id,
        OAUTH_TOKEN_ENDPOINT_KEY,
        "OAuth token endpoint",
    )?;
    let scopes = required_secret(&store, account_id, OAUTH_SCOPES_KEY, "OAuth scopes")?;
    drop(store);

    validate_oauth_token_endpoint(&token_endpoint)?;
    let response = reqwest::Client::new()
        .post(&token_endpoint)
        .form(&[
            ("client_id", client_id.as_str()),
            ("refresh_token", refresh_token.as_str()),
            ("grant_type", "refresh_token"),
            ("scope", scopes.as_str()),
        ])
        .send()
        .await
        .map_err(|error| format!("OAuth token refresh failed: {error}"))?;
    let status = response.status();
    let payload: serde_json::Value = response
        .json()
        .await
        .map_err(|error| format!("OAuth token response was invalid: {error}"))?;
    if !status.is_success() {
        let detail = payload
            .get("error_description")
            .or_else(|| payload.get("error"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("the authorization server rejected the refresh token");
        return Err(format!("OAuth token refresh failed: {detail}"));
    }
    let refreshed_access_token = payload
        .get("access_token")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "OAuth token response did not contain an access token".to_owned())?;
    let expires_in_seconds = payload
        .get("expires_in")
        .and_then(serde_json::Value::as_i64)
        .filter(|value| *value > 0)
        .ok_or_else(|| "OAuth token response did not contain a valid lifetime".to_owned())?;
    let new_expiry =
        current_timestamp_ms()?.saturating_add(expires_in_seconds.saturating_mul(1_000));
    let rotated_refresh_token = payload
        .get("refresh_token")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty());
    let mut store = open_profile_store(database_path)?;
    store
        .set(account_id, OAUTH_ACCESS_TOKEN_KEY, refreshed_access_token)
        .map_err(|error| error.to_string())?;
    store
        .set(account_id, OAUTH_EXPIRES_AT_KEY, &new_expiry.to_string())
        .map_err(|error| error.to_string())?;
    if let Some(rotated_refresh_token) = rotated_refresh_token {
        store
            .set(account_id, OAUTH_REFRESH_TOKEN_KEY, rotated_refresh_token)
            .map_err(|error| error.to_string())?;
    }
    Ok(StoredMailCredential::OAuth2(Zeroizing::new(
        refreshed_access_token.to_owned(),
    )))
}

fn validate_oauth_token_endpoint(value: &str) -> Result<(), String> {
    if matches!(value, GOOGLE_TOKEN_ENDPOINT | MICROSOFT_TOKEN_ENDPOINT) {
        Ok(())
    } else {
        Err("the stored OAuth token endpoint is not trusted".into())
    }
}

fn migrate_legacy_credentials(store: &mut SqliteMailStore) -> Result<(), String> {
    let accounts = store
        .list_mail_accounts()
        .map_err(|error| error.to_string())?;
    for account in accounts {
        if store
            .get(&account.id, ACCOUNT_PASSWORD_KEY)
            .map_err(|error| error.to_string())?
            .is_some()
        {
            continue;
        }
        let Ok(password) = load_legacy_password(&account) else {
            continue;
        };
        store
            .set(&account.id, ACCOUNT_PASSWORD_KEY, &password)
            .map_err(|error| error.to_string())?;
        let _ = delete_legacy_password(&account);
    }
    Ok(())
}

/// Mailbox data transferred to the Flutter interface.
pub struct MailboxDto {
    pub id: String,
    pub account_id: String,
    pub display_name: String,
    pub role: String,
    pub unread_count: u32,
    pub total_count: u32,
}

/// Message data transferred to the Flutter interface.
pub struct MessageDto {
    pub id: String,
    pub account_id: String,
    pub mailbox_id: String,
    pub sender: String,
    pub email: String,
    pub subject: String,
    pub preview: String,
    pub body: String,
    pub plain_text: String,
    pub received_at_ms: i64,
    pub unread: bool,
    pub flagged: bool,
    pub draft: bool,
    pub editable_draft: bool,
    pub draft_synchronized: bool,
    pub draft_to: String,
    pub draft_cc: String,
    pub draft_bcc: String,
    pub to_recipients: String,
    pub cc_recipients: String,
    pub bcc_recipients: String,
    pub editor_delta_json: String,
    pub has_attachment: bool,
    pub attachments: Vec<MessageAttachmentDto>,
}

/// Downloadable attachment metadata transferred to Flutter.
pub struct MessageAttachmentDto {
    pub id: String,
    pub file_name: String,
    pub content_type: String,
    pub size_bytes: u32,
    pub available_locally: bool,
}

/// Complete initial state required by the mail workspace.
pub struct WorkspaceSnapshot {
    pub mailboxes: Vec<MailboxDto>,
    pub favorite_mailbox_ids: Vec<String>,
    pub dark_mode_enabled: bool,
    pub messages: Vec<MessageDto>,
    pub calendar_events: Vec<CalendarEventDto>,
    pub tasks: Vec<TaskDto>,
    pub contacts: Vec<ContactDto>,
    pub mail_accounts: Vec<MailAccountDto>,
    pub sync_warnings: Vec<String>,
    /// Compact IMAP catalogue entries still scheduled for automatic
    /// continuation after the current synchronization pass.
    pub catalog_messages_remaining: u32,
    pub delta_mailboxes_synchronized: u32,
    pub full_mailboxes_reconciled: u32,
    pub qresync_mailboxes_synchronized: u32,
    pub pending_mail_operations: u32,
}

/// Result of pushing the durable draft queue for one account.
pub struct DraftSyncDto {
    pub synchronized: u32,
    pub pending: u32,
    pub warnings: Vec<String>,
}

/// Result of one bounded IMAP IDLE wait for the currently visible mailbox.
pub struct MailboxIdleDto {
    pub idle_supported: bool,
    pub changed: bool,
}

pub struct CalendarEventDto {
    pub id: String,
    pub title: String,
    pub starts_at_ms: i64,
    pub ends_at_ms: i64,
    pub location: Option<String>,
}

pub struct TaskDto {
    pub id: String,
    pub title: String,
    pub due_at_ms: Option<i64>,
    pub completed: bool,
}

pub struct ContactDto {
    pub id: String,
    pub name: String,
    pub email: String,
}

pub struct MailAccountDto {
    pub id: String,
    pub display_name: String,
    pub email: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub imap_security: String,
    pub imap_username: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_security: String,
    pub smtp_username: String,
    pub authentication: String,
    pub oauth_provider: Option<String>,
    pub last_sync_at_ms: Option<i64>,
}

/// User-created local message passed from the composition workflow.
pub struct LocalMessageInput {
    pub id: String,
    pub account_id: String,
    pub mailbox_id: String,
    pub sender: String,
    pub email: String,
    pub subject: String,
    pub preview: String,
    pub plain_text: String,
    pub html_text: String,
    pub attachment_paths: Vec<String>,
    pub retained_attachment_ids: Vec<String>,
    pub draft_to: String,
    pub draft_cc: String,
    pub draft_bcc: String,
    pub editor_delta_json: String,
    pub received_at_ms: i64,
    pub unread: bool,
    pub flagged: bool,
    pub draft: bool,
    pub has_attachment: bool,
}

pub struct LocalCalendarEventInput {
    pub id: String,
    pub title: String,
    pub starts_at_ms: i64,
    pub ends_at_ms: i64,
    pub location: Option<String>,
}

pub struct LocalTaskInput {
    pub id: String,
    pub title: String,
    pub due_at_ms: Option<i64>,
    pub completed: bool,
}

pub struct LocalContactInput {
    pub id: String,
    pub name: String,
    pub email: String,
}

/// Complete user-supplied IMAP/SMTP configuration. `password` is accepted only
/// by credential-related functions and is never included in snapshots.
pub struct MailAccountInput {
    pub id: String,
    pub display_name: String,
    pub email: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub imap_security: String,
    pub imap_username: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_security: String,
    pub smtp_username: String,
}

/// OAuth token set returned by an Authorization Code + PKCE exchange.
/// It is accepted only by credential functions and stored in the encrypted
/// profile; tokens are never returned in workspace snapshots.
pub struct OAuthTokenInput {
    pub provider: String,
    pub client_id: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at_ms: i64,
    pub token_endpoint: String,
    pub scopes: String,
}

/// Rich message submitted from the desktop composer.
pub struct OutgoingMessageInput {
    pub account_id: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub plain_text: String,
    pub html_text: String,
    pub attachment_paths: Vec<String>,
    pub stored_attachment_ids: Vec<String>,
    pub high_importance: bool,
}

/// Opens the local profile database and returns its mail workspace snapshot.
///
/// The first prototype run seeds deterministic local sample data. Subsequent
/// launches read the same persisted records from SQLite.
///
/// # Errors
///
/// Returns a user-presentable description when the profile database cannot be
/// opened, migrated, seeded, or read.
pub fn open_workspace(database_path: String) -> Result<WorkspaceSnapshot, String> {
    let account_id = AccountId::parse(DEMO_ACCOUNT_ID).map_err(|error| error.to_string())?;
    let database_path = PathBuf::from(database_path);
    let mut store = open_profile_store(&database_path)?;
    migrate_legacy_credentials(&mut store)?;
    profile_vault(&database_path)?
        .migrate_plaintext_objects(&profile_object_root(&database_path))
        .map_err(|error| error.to_string())?;

    if store
        .list_mailboxes(&account_id)
        .map_err(|error| error.to_string())?
        .is_empty()
    {
        seed_prototype(&mut store, &account_id)?;
    } else {
        upgrade_prototype_content(&mut store)?;
    }

    load_snapshot(&store)
}

/// Searches mail metadata or the broader locally cached content inside the
/// encrypted profile.
///
/// The default scope covers subject, sender, address, and all retained
/// recipients. When `include_content` is true it additionally covers preview,
/// cached body text, and attachment names. The encrypted FTS index returns
/// field-weighted matches without exposing terms or indexed text outside the
/// profile.
///
/// # Errors
///
/// Returns an error when the query is excessive or the encrypted index cannot
/// be read.
pub fn search_profile_messages(
    database_path: String,
    query: String,
    include_content: bool,
    limit: u32,
) -> Result<Vec<MessageDto>, String> {
    let store = open_profile_store(database_path)?;
    let limit = usize::try_from(limit).map_err(|error| error.to_string())?;
    let summaries = store
        .search_messages(&query, include_content, limit)
        .map_err(|error| error.to_string())?;
    let synchronized_drafts = synchronized_draft_ids(&store, &summaries)?;
    summaries
        .into_iter()
        .map(|summary| {
            let body = store
                .message_body(&summary.id)
                .map_err(|error| error.to_string())?;
            let attachments = store
                .list_attachments(&summary.id)
                .map_err(|error| error.to_string())?;
            let recipients = store
                .message_recipients(&summary.id)
                .map_err(|error| error.to_string())?;
            let draft = store
                .local_draft_metadata(&summary.id)
                .map_err(|error| error.to_string())?;
            let draft_synchronized = synchronized_drafts.contains(summary.id.as_str());
            Ok(message_dto(
                summary,
                body,
                recipients,
                attachments,
                draft,
                draft_synchronized,
            ))
        })
        .collect()
}

/// Loads one bounded page from an already encrypted local mailbox catalogue.
///
/// This never performs network I/O. Header-only entries are returned with an
/// empty body and can be populated on demand through
/// [`load_remote_message_content`].
///
/// # Errors
///
/// Returns a validation or encrypted-profile storage error.
pub fn load_mailbox_messages(
    database_path: String,
    mailbox_id: String,
    offset: u32,
    limit: u32,
) -> Result<Vec<MessageDto>, String> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let store = open_profile_store(database_path)?;
    let mailbox_id = MailboxId::parse(mailbox_id).map_err(|error| error.to_string())?;
    let offset = usize::try_from(offset).map_err(|error| error.to_string())?;
    let limit = usize::try_from(limit)
        .map_err(|error| error.to_string())?
        .min(MAX_MESSAGE_PAGE_SIZE);
    let summaries = store
        .list_message_page(&mailbox_id, offset, limit)
        .map_err(|error| error.to_string())?;
    message_dtos(&store, summaries)
}

/// Persists a locally composed message and its body atomically.
///
/// # Errors
///
/// Returns a user-presentable description when identifiers, the sender
/// address, or the storage transaction are invalid.
pub fn save_local_message(
    database_path: String,
    input: LocalMessageInput,
) -> Result<MessageDto, String> {
    let database_path = PathBuf::from(database_path);
    let mut store = open_profile_store(&database_path)?;
    let message_id = MessageId::parse(input.id).map_err(|error| error.to_string())?;
    let previous_attachments = store
        .list_attachments(&message_id)
        .map_err(|error| error.to_string())?;
    let retained_ids = input
        .retained_attachment_ids
        .iter()
        .map(|id| AttachmentId::parse(id).map_err(|error| error.to_string()))
        .collect::<Result<HashSet<_>, _>>()?;
    if retained_ids.len() != input.retained_attachment_ids.len() {
        return Err("retained attachment identifiers contain duplicates".into());
    }
    let retained_attachments = previous_attachments
        .iter()
        .filter(|attachment| retained_ids.contains(&attachment.id))
        .cloned()
        .collect::<Vec<_>>();
    if retained_attachments.len() != retained_ids.len()
        || retained_attachments
            .iter()
            .any(|attachment| !attachment.is_available_locally())
    {
        return Err("a retained draft attachment is missing or not local".into());
    }
    let selected_attachments = load_outgoing_attachments(&input.attachment_paths)?;
    validate_combined_outgoing_attachments(&retained_attachments, &selected_attachments)?;
    if input.has_attachment != !(retained_attachments.is_empty() && selected_attachments.is_empty())
    {
        return Err("attachment indicator does not match selected files".into());
    }
    let mut flags = Vec::new();
    if !input.unread {
        flags.push(MessageFlag::Seen);
    }
    if input.flagged {
        flags.push(MessageFlag::Flagged);
    }
    if input.draft {
        flags.push(MessageFlag::Draft);
    }
    let summary = MessageSummary {
        id: message_id.clone(),
        account_id: AccountId::parse(input.account_id).map_err(|error| error.to_string())?,
        mailbox_id: MailboxId::parse(input.mailbox_id).map_err(|error| error.to_string())?,
        from: MailAddress::new(input.email, Some(input.sender))
            .map_err(|error| error.to_string())?,
        subject: input.subject,
        preview: input.preview,
        received_at_ms: input.received_at_ms,
        flags,
        has_attachments: input.has_attachment,
    };
    let body = MessageBody {
        message_id: message_id.clone(),
        plain_text: Some(input.plain_text),
        sanitized_html: Some(sanitize_composed_html(&input.html_text)),
    };
    let recipients = MessageRecipients {
        message_id: message_id.clone(),
        to: input.draft_to.clone(),
        cc: input.draft_cc.clone(),
        bcc: input.draft_bcc.clone(),
    };
    let draft_metadata = input.draft.then(|| LocalDraftMetadata {
        message_id: message_id.clone(),
        to: input.draft_to,
        cc: input.draft_cc,
        bcc: input.draft_bcc,
        editor_delta_json: input.editor_delta_json,
    });
    if let Some(draft) = &draft_metadata {
        validate_draft_metadata(draft)?;
    }
    // Do not create object files until every user-provided message field has
    // passed domain validation. From here on, failures are rolled back below.
    let new_attachments =
        persist_attachment_objects(&database_path, &message_id, &selected_attachments)?;
    let mut attachments = retained_attachments;
    attachments.extend(new_attachments.iter().cloned());
    if let Err(error) = store.save_local_message(
        &summary,
        &body,
        &recipients,
        &attachments,
        draft_metadata.as_ref(),
    ) {
        remove_attachment_objects(&database_path, &new_attachments);
        return Err(error.to_string());
    }
    let replaced_attachments = previous_attachments
        .iter()
        .filter(|attachment| !retained_ids.contains(&attachment.id))
        .cloned()
        .collect::<Vec<_>>();
    remove_attachment_objects(&database_path, &replaced_attachments);
    Ok(message_dto(
        summary,
        body,
        recipients,
        attachments,
        draft_metadata,
        false,
    ))
}

/// Downloads and stores the bounded display body for one header-only remote
/// search result.
///
/// # Errors
///
/// Returns an error when the remote identity is stale, the account credential
/// is unavailable, or the bounded IMAP body download cannot be persisted.
pub async fn load_remote_message_content(
    database_path: String,
    message_id: String,
) -> Result<Option<MessageDto>, String> {
    let message_id = MessageId::parse(message_id).map_err(|error| error.to_string())?;
    let store = open_profile_store(&database_path)?;
    let metadata = store
        .remote_message_metadata(&message_id)
        .map_err(|error| error.to_string())?;
    let account = store
        .list_mail_accounts()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|account| account.id == metadata.account_id)
        .ok_or_else(|| "mail account for this message no longer exists".to_owned())?;
    drop(store);
    let credential = load_account_credential(Path::new(&database_path), &account.id).await?;

    let remote = match download_message_content(
        &account,
        credential.connector_credential(),
        &metadata.remote_mailbox,
        metadata.uid_validity,
        metadata.remote_uid,
    )
    .await
    {
        Ok(remote) => remote,
        Err(ConnectorError::RemoteMessageMissing(_)) => {
            let mut store = open_profile_store(&database_path)?;
            let removed_attachments = store
                .remove_vanished_remote_messages(
                    &metadata.account_id,
                    &metadata.remote_mailbox,
                    metadata.uid_validity,
                    &[metadata.remote_uid],
                )
                .map_err(|error| error.to_string())?;
            remove_attachment_objects(Path::new(&database_path), &removed_attachments);
            return Ok(None);
        }
        Err(error) => return Err(error.to_string()),
    };
    let mut store = open_profile_store(&database_path)?;
    let mut warnings = Vec::new();
    cache_remote_message(
        &database_path,
        &account,
        remote,
        message_id,
        current_timestamp_ms()?,
        &mut store,
        &mut warnings,
    )
    .map(Some)
}

/// Waits for a push-style IMAP IDLE notification on one configured mailbox.
///
/// The call is bounded by `timeout_seconds`. If the server lacks QRESYNC, a
/// received IDLE notification marks only this mailbox for a full UID safety
/// reconciliation during the immediately following synchronization.
///
/// # Errors
///
/// Returns an account, credential, connection, or protocol error. Local and
/// virtual folders return an unsupported result without opening a connection.
pub async fn wait_for_mailbox_idle_change(
    database_path: String,
    mailbox_id: String,
    timeout_seconds: u32,
) -> Result<MailboxIdleDto, String> {
    let mailbox_id = MailboxId::parse(mailbox_id).map_err(|error| error.to_string())?;
    let store = open_profile_store(&database_path)?;
    let accounts = store
        .list_mail_accounts()
        .map_err(|error| error.to_string())?;
    let mut selected = None;
    for account in accounts {
        let mailbox = store
            .list_mailboxes(&account.id)
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|mailbox| mailbox.id == mailbox_id);
        if let Some(mailbox) = mailbox {
            selected = Some((account, mailbox.display_name));
            break;
        }
    }
    let Some((account, remote_mailbox)) = selected else {
        return Ok(MailboxIdleDto {
            idle_supported: false,
            changed: false,
        });
    };
    drop(store);

    let credential = load_account_credential(Path::new(&database_path), &account.id).await?;
    let result = wait_for_mailbox_change(
        &account,
        credential.connector_credential(),
        &remote_mailbox,
        std::time::Duration::from_secs(u64::from(timeout_seconds)),
    )
    .await
    .map_err(|error| error.to_string())?;

    if result.changed && !result.qresync_supported {
        let mut store = open_profile_store(&database_path)?;
        if let Some(mut state) = store
            .remote_mailbox_sync_states(&account.id)
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|state| state.remote_mailbox == remote_mailbox)
        {
            state.last_full_reconcile_at_ms = 0;
            store
                .save_remote_mailbox_sync_state(&state)
                .map_err(|error| error.to_string())?;
        }
    }

    Ok(MailboxIdleDto {
        idle_supported: result.idle_supported,
        changed: result.changed,
    })
}

/// Saves one cached or server-backed attachment to a user-selected destination.
///
/// # Errors
///
/// Returns an error when metadata is unknown or stale, the local object is
/// invalid, the server download cannot be verified, or the destination cannot
/// be written. Server-backed sections are fetched with IMAP `BODY.PEEK`.
pub async fn export_attachment(
    database_path: String,
    attachment_id: String,
    destination_path: String,
) -> Result<(), String> {
    let database_path = PathBuf::from(database_path);
    let store = open_profile_store(&database_path)?;
    let attachment = store
        .attachment(&AttachmentId::parse(attachment_id).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;

    if attachment.object_key.is_some() {
        return copy_local_attachment(&database_path, &attachment, &destination_path);
    }
    if attachment.size_bytes > MAX_ON_DEMAND_ATTACHMENT_BYTES as u64 {
        return Err("attachment exceeds the 100 MiB download limit".into());
    }
    let remote_section = attachment
        .remote_section
        .clone()
        .ok_or_else(|| "attachment has neither a local object nor a remote section".to_owned())?;
    let transfer_encoding = attachment
        .transfer_encoding
        .clone()
        .ok_or_else(|| "attachment transfer encoding is missing".to_owned())?;
    let metadata = store
        .remote_message_metadata(&attachment.message_id)
        .map_err(|error| error.to_string())?;
    let account = store
        .list_mail_accounts()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|account| account.id == metadata.account_id)
        .ok_or_else(|| "mail account for this attachment no longer exists".to_owned())?;
    drop(store);
    let credential = load_account_credential(&database_path, &account.id).await?;
    let encoded = download_attachment_part(
        &account,
        credential.connector_credential(),
        &metadata.remote_mailbox,
        metadata.uid_validity,
        metadata.remote_uid,
        &remote_section,
    )
    .await
    .map_err(|error| error.to_string())?;
    let decoded =
        decode_attachment_part(&encoded, &transfer_encoding, MAX_ON_DEMAND_ATTACHMENT_BYTES)
            .map_err(|error| error.to_string())?;
    write_exported_attachment(Path::new(&destination_path), &decoded)
}

/// Writes a complete password-protected profile backup.
///
/// The archive contains the already encrypted database, encrypted local
/// objects, and account credentials. Its profile key is wrapped with an
/// Argon2id-derived export key; the password itself is never stored.
///
/// # Errors
///
/// Returns an error when the profile cannot be checkpointed, an object is
/// unsafe, the password is too short, or the destination cannot be written.
pub fn export_profile(
    database_path: String,
    destination_path: String,
    password: String,
) -> Result<(), String> {
    let password = Zeroizing::new(password);
    let database_path = PathBuf::from(database_path);
    let store = open_profile_store(&database_path)?;
    store.checkpoint().map_err(|error| error.to_string())?;
    let vault = profile_vault(&database_path)?;
    let object_root = profile_object_root(&database_path);
    vault
        .migrate_plaintext_objects(&object_root)
        .map_err(|error| error.to_string())?;
    vault
        .export_archive(
            &database_path,
            &object_root,
            Path::new(&destination_path),
            password.as_str(),
        )
        .map_err(|error| error.to_string())
}

/// Replaces the active local profile with an authenticated portable backup.
///
/// Import first extracts and validates the encrypted database in a staging
/// directory. Existing local data is retained until the replacement and new
/// OS-protected profile key have both been installed.
///
/// # Errors
///
/// Returns an error for a wrong password, a modified archive, an invalid
/// database key, or a failed atomic replacement.
pub fn import_profile(
    database_path: String,
    source_path: String,
    password: String,
) -> Result<WorkspaceSnapshot, String> {
    let password = Zeroizing::new(password);
    let database_path = PathBuf::from(database_path);
    let parent = database_path
        .parent()
        .ok_or_else(|| "profile database has no parent directory".to_owned())?;
    let serial = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let staging_root = parent.join(format!(".maicenta-import-{}-{serial}", std::process::id()));
    let staged = stage_archive(Path::new(&source_path), &staging_root, password.as_str())
        .map_err(|error| error.to_string())?;

    // Opening the staged database authenticates SQLCipher pages and applies
    // only normal forward schema migrations before any active data is moved.
    let staged_store =
        SqliteMailStore::open_encrypted(&staged.database_path, staged.vault.key().as_bytes())
            .map_err(|error| error.to_string())?;
    staged_store
        .schema_version()
        .map_err(|error| error.to_string())?;
    staged_store
        .integrity_check()
        .map_err(|error| error.to_string())?;
    staged_store
        .checkpoint()
        .map_err(|error| error.to_string())?;
    drop(staged_store);

    let previous_vault = profile_vault(&database_path).ok();
    if database_path.exists() {
        let current_store = open_profile_store(&database_path)?;
        current_store
            .checkpoint()
            .map_err(|error| error.to_string())?;
        drop(current_store);
    }
    install_staged_profile(&database_path, &staged.database_path, &staged.object_root)?;
    #[cfg(not(test))]
    if let Err(error) = staged.vault.install(&database_path) {
        rollback_imported_profile(&database_path)?;
        let _ = fs::remove_dir_all(&staging_root);
        return Err(error.to_string());
    }
    replace_cached_vault(&database_path, staged.vault.clone())?;
    cleanup_import_backups(&database_path)?;
    let _ = fs::remove_dir_all(&staging_root);
    #[cfg(not(test))]
    if let Some(previous) = previous_vault {
        if previous.profile_id() != staged.vault.profile_id() {
            let _ = previous.remove_from_key_store();
        }
    }
    #[cfg(test)]
    drop(previous_vault);

    let mut store = open_profile_store(&database_path)?;
    migrate_legacy_credentials(&mut store)?;
    load_snapshot(&store)
}

fn import_backup_path(path: &Path) -> PathBuf {
    let mut backup = path.as_os_str().to_owned();
    backup.push(".import-backup");
    PathBuf::from(backup)
}

fn install_staged_profile(
    database_path: &Path,
    staged_database: &Path,
    staged_objects: &Path,
) -> Result<(), String> {
    let object_root = profile_object_root(database_path);
    let database_backup = import_backup_path(database_path);
    let object_backup = import_backup_path(&object_root);
    if database_backup.exists() || object_backup.exists() {
        return Err("an earlier profile import backup requires manual recovery".into());
    }
    if database_path.exists() {
        fs::rename(database_path, &database_backup).map_err(|error| error.to_string())?;
    }
    if let Err(error) = fs::rename(staged_database, database_path) {
        if database_backup.exists() {
            let _ = fs::rename(&database_backup, database_path);
        }
        return Err(error.to_string());
    }
    if object_root.exists() {
        if let Err(error) = fs::rename(&object_root, &object_backup) {
            let _ = fs::rename(database_path, staged_database);
            if database_backup.exists() {
                let _ = fs::rename(&database_backup, database_path);
            }
            return Err(error.to_string());
        }
    }
    if staged_objects.exists() {
        if let Err(error) = fs::rename(staged_objects, &object_root) {
            if object_backup.exists() {
                let _ = fs::rename(&object_backup, &object_root);
            }
            let _ = fs::rename(database_path, staged_database);
            if database_backup.exists() {
                let _ = fs::rename(&database_backup, database_path);
            }
            return Err(error.to_string());
        }
    }
    Ok(())
}

#[cfg(not(test))]
fn rollback_imported_profile(database_path: &Path) -> Result<(), String> {
    let object_root = profile_object_root(database_path);
    let database_backup = import_backup_path(database_path);
    let object_backup = import_backup_path(&object_root);
    if database_path.exists() {
        fs::remove_file(database_path).map_err(|error| error.to_string())?;
    }
    if database_backup.exists() {
        fs::rename(&database_backup, database_path).map_err(|error| error.to_string())?;
    }
    if object_root.exists() {
        fs::remove_dir_all(&object_root).map_err(|error| error.to_string())?;
    }
    if object_backup.exists() {
        fs::rename(&object_backup, &object_root).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn cleanup_import_backups(database_path: &Path) -> Result<(), String> {
    let database_backup = import_backup_path(database_path);
    let object_backup = import_backup_path(&profile_object_root(database_path));
    if database_backup.exists() {
        fs::remove_file(database_backup).map_err(|error| error.to_string())?;
    }
    if object_backup.exists() {
        fs::remove_dir_all(object_backup).map_err(|error| error.to_string())?;
    }
    for suffix in ["-wal", "-shm"] {
        let mut path = database_path.as_os_str().to_owned();
        path.push(suffix);
        let path = PathBuf::from(path);
        if path.exists() {
            fs::remove_file(path).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
fn export_local_attachment(
    database_path: String,
    attachment_id: String,
    destination_path: String,
) -> Result<(), String> {
    let database_path = PathBuf::from(database_path);
    let store = open_profile_store(&database_path)?;
    let attachment = store
        .attachment(&AttachmentId::parse(attachment_id).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    copy_local_attachment(&database_path, &attachment, &destination_path)
}

fn copy_local_attachment(
    database_path: &Path,
    attachment: &MessageAttachment,
    destination_path: &str,
) -> Result<(), String> {
    let object_key = attachment
        .object_key
        .as_deref()
        .ok_or_else(|| "attachment is not cached locally".to_owned())?;
    let source = attachment_object_path(database_path, object_key)?;
    let source_metadata = fs::symlink_metadata(&source).map_err(|error| {
        format!(
            "stored attachment {} cannot be inspected: {error}",
            source.display()
        )
    })?;
    if !source_metadata.file_type().is_file() {
        return Err("stored attachment does not match its metadata".into());
    }
    let body = profile_vault(database_path)?
        .read_object(&source, object_key, attachment.size_bytes)
        .map_err(|error| error.to_string())?;
    if u64::try_from(body.len()).map_err(|error| error.to_string())? != attachment.size_bytes {
        return Err("exported attachment size does not match its metadata".into());
    }
    write_exported_attachment(Path::new(destination_path), &body)
}

fn write_exported_attachment(destination: &Path, body: &[u8]) -> Result<(), String> {
    let mut output = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(destination)
        .map_err(|error| {
            format!(
                "attachment destination {} cannot be opened: {error}",
                destination.display()
            )
        })?;
    output.write_all(body).map_err(|error| {
        format!(
            "attachment destination {} cannot be written: {error}",
            destination.display()
        )
    })?;
    output.sync_all().map_err(|error| {
        format!(
            "attachment destination {} cannot be finalized: {error}",
            destination.display()
        )
    })
}

fn persist_attachment_objects(
    database_path: &Path,
    message_id: &MessageId,
    outgoing: &[OutgoingAttachment],
) -> Result<Vec<MessageAttachment>, String> {
    if outgoing.is_empty() {
        return Ok(Vec::new());
    }
    let attachment_directory = profile_object_root(database_path).join("attachments");
    fs::create_dir_all(&attachment_directory).map_err(|error| {
        format!(
            "attachment object directory {} cannot be created: {error}",
            attachment_directory.display()
        )
    })?;
    let serial = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let mut stored = Vec::with_capacity(outgoing.len());
    let vault = profile_vault(database_path)?;

    for (index, source) in outgoing.iter().enumerate() {
        let size_bytes = u64::try_from(source.body.len()).map_err(|error| error.to_string())?;
        let digest = stable_remote_key(&format!("{}:{serial}:{index}", message_id.as_str()));
        let id = AttachmentId::parse(format!("attachment.{digest}"))
            .map_err(|error| error.to_string())?;
        let object_key = format!("attachments/{id}.bin");
        let final_path = attachment_object_path(database_path, &object_key)?;
        let write_result = vault
            .write_object(&final_path, &object_key, &source.body)
            .map_err(|error| error.to_string());
        if let Err(error) = write_result {
            remove_attachment_objects(database_path, &stored);
            return Err(error);
        }
        stored.push(MessageAttachment {
            id,
            message_id: message_id.clone(),
            file_name: source.file_name.clone(),
            content_type: source.content_type.clone(),
            size_bytes,
            object_key: Some(object_key),
            remote_section: None,
            transfer_encoding: None,
        });
    }

    Ok(stored)
}

fn profile_object_root(database_path: &Path) -> PathBuf {
    database_path.with_extension("objects")
}

fn attachment_object_path(database_path: &Path, object_key: &str) -> Result<PathBuf, String> {
    let relative = Path::new(object_key);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || !object_key.starts_with("attachments/")
    {
        return Err("attachment object key is unsafe".into());
    }
    Ok(profile_object_root(database_path).join(relative))
}

fn remove_attachment_objects(database_path: &Path, attachments: &[MessageAttachment]) {
    for attachment in attachments {
        let Some(object_key) = attachment.object_key.as_deref() else {
            continue;
        };
        if let Ok(path) = attachment_object_path(database_path, object_key) {
            let _ = fs::remove_file(path);
        }
    }
}

/// Persists mailbox placement and user-controlled flags for one message.
///
/// # Errors
///
/// Returns a user-presentable description when identifiers are invalid, the
/// message is missing, or the transaction fails.
pub fn update_local_message(
    database_path: String,
    message_id: String,
    mailbox_id: String,
    unread: bool,
    flagged: bool,
) -> Result<u32, String> {
    let mut store = open_profile_store(database_path)?;
    store
        .update_message_state(
            &MessageId::parse(message_id).map_err(|error| error.to_string())?,
            &MailboxId::parse(mailbox_id).map_err(|error| error.to_string())?,
            unread,
            flagged,
        )
        .map_err(|error| error.to_string())
}

/// Creates a user-defined local mailbox.
///
/// # Errors
///
/// Returns a user-presentable description when the identifier, display name,
/// or storage transaction is invalid.
pub fn create_local_mailbox(
    database_path: String,
    mailbox_id: String,
    display_name: String,
) -> Result<(), String> {
    let display_name = validated_display_name(display_name)?;
    let mut store = open_profile_store(database_path)?;
    store
        .save_mailboxes(&[Mailbox {
            id: MailboxId::parse(mailbox_id).map_err(|error| error.to_string())?,
            account_id: AccountId::parse(DEMO_ACCOUNT_ID).map_err(|error| error.to_string())?,
            display_name,
            role: MailboxRole::Custom,
            unread_count: 0,
            total_count: 0,
        }])
        .map_err(|error| error.to_string())
}

/// Renames one locally stored mailbox.
///
/// # Errors
///
/// Returns a user-presentable description when the identifier, display name,
/// or storage operation is invalid.
pub fn rename_local_mailbox(
    database_path: String,
    mailbox_id: String,
    display_name: String,
) -> Result<(), String> {
    let display_name = validated_display_name(display_name)?;
    let mut store = open_profile_store(database_path)?;
    store
        .rename_mailbox(
            &MailboxId::parse(mailbox_id).map_err(|error| error.to_string())?,
            &display_name,
        )
        .map_err(|error| error.to_string())
}

/// Deletes a custom mailbox and moves its contents to the fallback mailbox.
///
/// # Errors
///
/// Returns a user-presentable description when identifiers are invalid, the
/// target is not a custom mailbox, or the transaction fails.
pub fn delete_local_mailbox(
    database_path: String,
    mailbox_id: String,
    fallback_mailbox_id: String,
) -> Result<(), String> {
    let account_id = AccountId::parse(DEMO_ACCOUNT_ID).map_err(|error| error.to_string())?;
    let mailbox_id = MailboxId::parse(mailbox_id).map_err(|error| error.to_string())?;
    let fallback_mailbox_id =
        MailboxId::parse(fallback_mailbox_id).map_err(|error| error.to_string())?;
    let mut store = open_profile_store(database_path)?;
    let mailbox = store
        .list_mailboxes(&account_id)
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|mailbox| mailbox.id == mailbox_id)
        .ok_or_else(|| "item not found".to_owned())?;
    if mailbox.role != MailboxRole::Custom {
        return Err("only custom local mailboxes can be deleted".into());
    }
    store
        .delete_mailbox(&mailbox_id, &fallback_mailbox_id)
        .map_err(|error| error.to_string())
}

/// Replaces the ordered favorite-mailbox list stored in the encrypted profile.
/// The exact mailbox identifiers remain provider-independent local keys.
///
/// # Errors
///
/// Returns an error when an identifier is invalid, duplicated, unknown, or
/// the encrypted preference cannot be committed.
pub fn save_favorite_mailboxes(
    database_path: String,
    mailbox_ids: Vec<String>,
) -> Result<(), String> {
    let mailbox_ids = mailbox_ids
        .into_iter()
        .map(|id| MailboxId::parse(id).map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    open_profile_store(Path::new(&database_path))?
        .save_favorite_mailbox_ids(&mailbox_ids)
        .map_err(|error| error.to_string())
}

/// Persists the selected light or dark desktop color scheme in the encrypted
/// profile.
///
/// # Errors
///
/// Returns an error when the profile cannot be opened or updated.
pub fn save_dark_mode(database_path: String, enabled: bool) -> Result<(), String> {
    open_profile_store(Path::new(&database_path))?
        .save_dark_mode_enabled(enabled)
        .map_err(|error| error.to_string())
}

/// Creates or updates a local calendar event.
///
/// # Errors
///
/// Returns a validation or storage error when the event cannot be committed.
pub fn save_local_calendar_event(
    database_path: String,
    input: LocalCalendarEventInput,
) -> Result<(), String> {
    let title = validated_text(input.title, "calendar title", 200)?;
    let event = CalendarEvent {
        id: WorkspaceItemId::parse(input.id).map_err(|error| error.to_string())?,
        title,
        starts_at_ms: input.starts_at_ms,
        ends_at_ms: input.ends_at_ms,
        location: input.location.filter(|value| !value.trim().is_empty()),
    };
    open_profile_store(database_path)?
        .save_calendar_event(&event)
        .map_err(|error| error.to_string())
}

/// Creates or updates a local task.
///
/// # Errors
///
/// Returns a validation or storage error when the task cannot be committed.
pub fn save_local_task(database_path: String, input: LocalTaskInput) -> Result<(), String> {
    let task = TaskItem {
        id: WorkspaceItemId::parse(input.id).map_err(|error| error.to_string())?,
        title: validated_text(input.title, "task title", 200)?,
        due_at_ms: input.due_at_ms,
        completed: input.completed,
    };
    open_profile_store(database_path)?
        .save_task(&task)
        .map_err(|error| error.to_string())
}

/// Creates or updates a local contact.
///
/// # Errors
///
/// Returns a validation or storage error when the contact cannot be committed.
pub fn save_local_contact(database_path: String, input: LocalContactInput) -> Result<(), String> {
    let name = validated_text(input.name, "contact name", 200)?;
    let contact = Contact {
        id: WorkspaceItemId::parse(input.id).map_err(|error| error.to_string())?,
        email: MailAddress::new(input.email, Some(name.clone()))
            .map_err(|error| error.to_string())?,
        name,
    };
    open_profile_store(database_path)?
        .save_contact(&contact)
        .map_err(|error| error.to_string())
}

/// Saves account configuration and places its password inside the encrypted
/// profile vault.
///
/// # Errors
///
/// Returns a validation or encrypted-profile error. Passwords are never
/// included in workspace snapshots.
pub fn save_mail_account(
    database_path: String,
    input: MailAccountInput,
    password: String,
) -> Result<(), String> {
    let password = Zeroizing::new(password);
    let mut account = mail_account_from_input(input)?;
    let mut store = open_profile_store(database_path)?;
    let existing = store
        .list_mail_accounts()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|stored| stored.id == account.id);
    if let Some(existing) = &existing {
        account.last_sync_at_ms = existing.last_sync_at_ms;
    }
    if password.is_empty() && existing.is_none() {
        return Err("a password or app password is required for a new account".into());
    }
    store
        .save_mail_account(&account)
        .map_err(|error| error.to_string())?;
    if !password.is_empty() {
        store
            .set(&account.id, ACCOUNT_PASSWORD_KEY, password.as_str())
            .map_err(|error| error.to_string())?;
        remove_oauth_secrets(&mut store, &account.id)?;
    }
    Ok(())
}

/// Saves an OAuth-backed IMAP/SMTP account and its refreshable token set in
/// the encrypted profile vault.
///
/// Native applications must use Authorization Code + PKCE and must not ship a
/// client secret. The refresh token never crosses back into Flutter snapshots.
pub fn save_oauth_mail_account(
    database_path: String,
    input: MailAccountInput,
    tokens: OAuthTokenInput,
) -> Result<(), String> {
    let access_token = Zeroizing::new(tokens.access_token);
    let refresh_token = Zeroizing::new(tokens.refresh_token);
    validate_oauth_token_input(
        &tokens.provider,
        &tokens.client_id,
        access_token.as_str(),
        refresh_token.as_str(),
        tokens.expires_at_ms,
        &tokens.token_endpoint,
        &tokens.scopes,
    )?;
    let mut account = mail_account_from_input(input)?;
    let mut store = open_profile_store(database_path)?;
    if let Some(existing) = store
        .list_mail_accounts()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|stored| stored.id == account.id)
    {
        account.last_sync_at_ms = existing.last_sync_at_ms;
    }
    store
        .save_mail_account(&account)
        .map_err(|error| error.to_string())?;
    for (key, value) in [
        (OAUTH_PROVIDER_KEY, tokens.provider.as_str()),
        (OAUTH_CLIENT_ID_KEY, tokens.client_id.as_str()),
        (OAUTH_ACCESS_TOKEN_KEY, access_token.as_str()),
        (OAUTH_REFRESH_TOKEN_KEY, refresh_token.as_str()),
        (OAUTH_TOKEN_ENDPOINT_KEY, tokens.token_endpoint.as_str()),
        (OAUTH_SCOPES_KEY, tokens.scopes.as_str()),
    ] {
        store
            .set(&account.id, key, value)
            .map_err(|error| error.to_string())?;
    }
    store
        .set(
            &account.id,
            OAUTH_EXPIRES_AT_KEY,
            &tokens.expires_at_ms.to_string(),
        )
        .map_err(|error| error.to_string())?;
    store
        .remove(&account.id, ACCOUNT_PASSWORD_KEY)
        .map_err(|error| error.to_string())
}

fn validate_oauth_token_input(
    provider: &str,
    client_id: &str,
    access_token: &str,
    refresh_token: &str,
    expires_at_ms: i64,
    token_endpoint: &str,
    scopes: &str,
) -> Result<(), String> {
    if !matches!(provider, "google" | "microsoft365") {
        return Err("OAuth provider must be google or microsoft365".into());
    }
    if client_id.trim().is_empty()
        || access_token.is_empty()
        || refresh_token.is_empty()
        || scopes.trim().is_empty()
    {
        return Err("OAuth client ID, tokens, and scopes must not be empty".into());
    }
    validate_oauth_token_endpoint(token_endpoint)?;
    if expires_at_ms <= current_timestamp_ms()? {
        return Err("OAuth access token is already expired".into());
    }
    Ok(())
}

fn remove_oauth_secrets(store: &mut SqliteMailStore, account_id: &AccountId) -> Result<(), String> {
    for key in [
        OAUTH_PROVIDER_KEY,
        OAUTH_CLIENT_ID_KEY,
        OAUTH_ACCESS_TOKEN_KEY,
        OAUTH_REFRESH_TOKEN_KEY,
        OAUTH_EXPIRES_AT_KEY,
        OAUTH_TOKEN_ENDPOINT_KEY,
        OAUTH_SCOPES_KEY,
    ] {
        store
            .remove(account_id, key)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Deletes an account, its cached mail, and its vault credential.
///
/// Calendar entries, tasks, contacts, and other accounts are not affected.
///
/// # Errors
///
/// Returns an account lookup or encrypted-profile storage error.
pub fn delete_mail_account(
    database_path: String,
    account_id: String,
) -> Result<WorkspaceSnapshot, String> {
    let account_id = AccountId::parse(account_id).map_err(|error| error.to_string())?;
    let mut store = open_profile_store(&database_path)?;
    let account = store
        .list_mail_accounts()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|account| account.id == account_id)
        .ok_or_else(|| "mail account not found".to_owned())?;
    let mut cached_attachments = Vec::new();
    let all_messages_limit = usize::try_from(i64::MAX).unwrap_or(usize::MAX);
    for mailbox in store
        .list_mailboxes(&account.id)
        .map_err(|error| error.to_string())?
    {
        for message in store
            .list_messages(&mailbox.id, all_messages_limit)
            .map_err(|error| error.to_string())?
        {
            cached_attachments.extend(
                store
                    .list_attachments(&message.id)
                    .map_err(|error| error.to_string())?,
            );
        }
    }
    store
        .delete_mail_account(&account.id)
        .map_err(|error| error.to_string())?;
    remove_attachment_objects(Path::new(&database_path), &cached_attachments);
    load_snapshot(&store)
}

/// Tests IMAP login and SMTP submission connectivity without persisting the
/// supplied configuration or sending a message.
///
/// # Errors
///
/// Returns a validation, TLS, authentication, or protocol error.
pub async fn test_mail_account_connection(
    input: MailAccountInput,
    password: String,
) -> Result<(), String> {
    let password = Zeroizing::new(password);
    let account = mail_account_from_input(input)?;
    test_account(&account, MailCredential::Password(password.as_str()))
        .await
        .map_err(|error| error.to_string())
}

/// Tests IMAP and SMTP with a short-lived OAuth access token without storing
/// either the configuration or token.
pub async fn test_oauth_mail_account_connection(
    input: MailAccountInput,
    access_token: String,
) -> Result<(), String> {
    let access_token = Zeroizing::new(access_token);
    if access_token.is_empty() {
        return Err("an OAuth access token is required".into());
    }
    let account = mail_account_from_input(input)?;
    test_account(
        &account,
        MailCredential::OAuth2AccessToken(access_token.as_str()),
    )
    .await
    .map_err(|error| error.to_string())
}

/// Pushes queued draft creates, edits, and removals for one account without
/// running a full mailbox catalogue pass.
///
/// # Errors
///
/// Returns an account, credential, connection, protocol, or storage error.
pub async fn synchronize_mail_account_drafts(
    database_path: String,
    account_id: String,
) -> Result<DraftSyncDto, String> {
    let account_id = AccountId::parse(account_id).map_err(|error| error.to_string())?;
    let store = open_profile_store(&database_path)?;
    let account = store
        .list_mail_accounts()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|account| account.id == account_id)
        .ok_or_else(|| "mail account not found".to_owned())?;
    drop(store);
    let credential = load_account_credential(Path::new(&database_path), &account.id).await?;
    let (synchronized, warnings) =
        synchronize_pending_drafts(&database_path, &account, credential.connector_credential())
            .await?;
    let pending = open_profile_store(&database_path)?
        .pending_mail_mutation_count()
        .map_err(|error| error.to_string())?;
    Ok(DraftSyncDto {
        synchronized,
        pending,
        warnings,
    })
}

/// Progressively catalogues compact message metadata, synchronizes a bounded
/// set of recent bodies for every configured account, and returns a refreshed
/// local snapshot.
///
/// # Errors
///
/// Returns a credential, network, MIME, or storage error. Existing cached data
/// remains available when synchronization fails.
pub async fn synchronize_mail_accounts(database_path: String) -> Result<WorkspaceSnapshot, String> {
    let accounts = open_profile_store(&database_path)?
        .list_mail_accounts()
        .map_err(|error| error.to_string())?;
    let mut sync_warnings = Vec::new();
    let mut catalog_messages_remaining = 0_u32;
    let mut delta_mailboxes_synchronized = 0_u32;
    let mut full_mailboxes_reconciled = 0_u32;
    let mut qresync_mailboxes_synchronized = 0_u32;
    for account in accounts {
        match synchronize_account(&database_path, &account).await {
            Ok(report) => {
                catalog_messages_remaining =
                    catalog_messages_remaining.saturating_add(report.catalog_messages_remaining);
                delta_mailboxes_synchronized = delta_mailboxes_synchronized
                    .saturating_add(report.delta_mailboxes_synchronized);
                full_mailboxes_reconciled =
                    full_mailboxes_reconciled.saturating_add(report.full_mailboxes_reconciled);
                qresync_mailboxes_synchronized = qresync_mailboxes_synchronized
                    .saturating_add(report.qresync_mailboxes_synchronized);
                sync_warnings.extend(
                    report
                        .warnings
                        .into_iter()
                        .map(|warning| format!("{}: {warning}", account.display_name)),
                );
            }
            Err(error) => sync_warnings.push(format!("{}: {error}", account.display_name)),
        }
    }
    let store = open_profile_store(database_path)?;
    let mut snapshot = load_snapshot(&store)?;
    snapshot.sync_warnings = sync_warnings;
    snapshot.catalog_messages_remaining = catalog_messages_remaining;
    snapshot.delta_mailboxes_synchronized = delta_mailboxes_synchronized;
    snapshot.full_mailboxes_reconciled = full_mailboxes_reconciled;
    snapshot.qresync_mailboxes_synchronized = qresync_mailboxes_synchronized;
    Ok(snapshot)
}

/// Sends a multipart HTML message with a plain-text alternative and validated
/// user-selected attachments through one configured SMTP account.
///
/// # Errors
///
/// Returns an account lookup, credential, address, connection, or SMTP error.
pub async fn send_account_message(
    database_path: String,
    input: OutgoingMessageInput,
) -> Result<String, String> {
    if input.plain_text.len() > 1_000_000 || input.html_text.len() > 2_000_000 {
        return Err("message body exceeds the supported size".into());
    }
    let sanitized_html = sanitize_composed_html(&input.html_text);
    let database_path = PathBuf::from(database_path);
    let store = open_profile_store(&database_path)?;
    let mut attachments =
        load_stored_outgoing_attachments(&store, &database_path, &input.stored_attachment_ids)?;
    attachments.extend(load_outgoing_attachments(&input.attachment_paths)?);
    validate_outgoing_attachment_set(&attachments)?;
    let account_id = AccountId::parse(input.account_id).map_err(|error| error.to_string())?;
    let account = store
        .list_mail_accounts()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|account| account.id == account_id)
        .ok_or_else(|| "mail account not found".to_owned())?;
    drop(store);
    let credential = load_account_credential(&database_path, &account.id).await?;
    let outgoing = OutgoingMessage {
        to: input.to,
        cc: input.cc,
        bcc: input.bcc,
        subject: input.subject,
        plain_text: input.plain_text,
        sanitized_html,
        attachments,
        high_importance: input.high_importance,
    };
    send_smtp_message(&account, credential.connector_credential(), &outgoing)
        .await
        .map(|response| {
            format!(
                "{} {}",
                response.code(),
                response.first_line().unwrap_or("message accepted")
            )
        })
        .map_err(|error| error.to_string())
}

fn load_outgoing_attachments(paths: &[String]) -> Result<Vec<OutgoingAttachment>, String> {
    if paths.len() > MAX_OUTGOING_ATTACHMENTS {
        return Err(format!(
            "at most {MAX_OUTGOING_ATTACHMENTS} attachments are supported"
        ));
    }
    let mut total_bytes = 0_u64;
    let mut attachments = Vec::with_capacity(paths.len());
    for raw_path in paths {
        let path = Path::new(raw_path);
        let metadata = fs::metadata(path)
            .map_err(|error| format!("attachment {} cannot be opened: {error}", path.display()))?;
        if !metadata.is_file() {
            return Err(format!("attachment {} is not a file", path.display()));
        }
        let remaining = MAX_OUTGOING_ATTACHMENT_BYTES.saturating_sub(total_bytes);
        if metadata.len() > remaining {
            return Err("attachments exceed the combined 18 MiB limit".into());
        }
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty() && !name.chars().any(char::is_control))
            .ok_or_else(|| format!("attachment {} has an invalid file name", path.display()))?
            .to_owned();
        let mut body = Vec::new();
        fs::File::open(path)
            .map_err(|error| format!("attachment {} cannot be opened: {error}", path.display()))?
            .take(remaining.saturating_add(1))
            .read_to_end(&mut body)
            .map_err(|error| format!("attachment {} cannot be read: {error}", path.display()))?;
        let body_len = u64::try_from(body.len()).map_err(|error| error.to_string())?;
        if body_len > remaining {
            return Err("attachments exceed the combined 18 MiB limit".into());
        }
        total_bytes += body_len;
        attachments.push(OutgoingAttachment {
            content_type: attachment_content_type(path).into(),
            file_name,
            body,
        });
    }
    Ok(attachments)
}

fn load_stored_outgoing_attachments(
    store: &SqliteMailStore,
    database_path: &Path,
    attachment_ids: &[String],
) -> Result<Vec<OutgoingAttachment>, String> {
    if attachment_ids.len() > MAX_OUTGOING_ATTACHMENTS {
        return Err(format!(
            "at most {MAX_OUTGOING_ATTACHMENTS} attachments are supported"
        ));
    }
    let unique_ids = attachment_ids.iter().collect::<HashSet<_>>();
    if unique_ids.len() != attachment_ids.len() {
        return Err("stored attachment identifiers contain duplicates".into());
    }

    attachment_ids
        .iter()
        .map(|raw_id| {
            let id = AttachmentId::parse(raw_id).map_err(|error| error.to_string())?;
            let attachment = store.attachment(&id).map_err(|error| error.to_string())?;
            let object_key = attachment
                .object_key
                .as_deref()
                .ok_or_else(|| "stored draft attachment is not available locally".to_owned())?;
            let path = attachment_object_path(database_path, object_key)?;
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                format!(
                    "stored attachment {} cannot be inspected: {error}",
                    path.display()
                )
            })?;
            if !metadata.file_type().is_file() {
                return Err("stored draft attachment does not match its metadata".into());
            }
            let body = profile_vault(database_path)?
                .read_object(&path, object_key, attachment.size_bytes)
                .map_err(|error| error.to_string())?;
            if u64::try_from(body.len()).map_err(|error| error.to_string())?
                != attachment.size_bytes
            {
                return Err("stored draft attachment size changed while reading".into());
            }
            Ok(OutgoingAttachment {
                file_name: attachment.file_name,
                content_type: attachment.content_type,
                body,
            })
        })
        .collect()
}

fn validate_outgoing_attachment_set(attachments: &[OutgoingAttachment]) -> Result<(), String> {
    if attachments.len() > MAX_OUTGOING_ATTACHMENTS {
        return Err(format!(
            "at most {MAX_OUTGOING_ATTACHMENTS} attachments are supported"
        ));
    }
    let total = attachments.iter().try_fold(0_u64, |total, attachment| {
        let size = u64::try_from(attachment.body.len()).map_err(|error| error.to_string())?;
        total
            .checked_add(size)
            .ok_or_else(|| "attachment size overflow".to_owned())
    })?;
    if total > MAX_OUTGOING_ATTACHMENT_BYTES {
        return Err("attachments exceed the combined 18 MiB limit".into());
    }
    Ok(())
}

fn validate_combined_outgoing_attachments(
    retained: &[MessageAttachment],
    selected: &[OutgoingAttachment],
) -> Result<(), String> {
    if retained.len() + selected.len() > MAX_OUTGOING_ATTACHMENTS {
        return Err(format!(
            "at most {MAX_OUTGOING_ATTACHMENTS} attachments are supported"
        ));
    }
    let retained_bytes = retained.iter().try_fold(0_u64, |total, attachment| {
        total
            .checked_add(attachment.size_bytes)
            .ok_or_else(|| "attachment size overflow".to_owned())
    })?;
    let selected_bytes = selected.iter().try_fold(0_u64, |total, attachment| {
        let size = u64::try_from(attachment.body.len()).map_err(|error| error.to_string())?;
        total
            .checked_add(size)
            .ok_or_else(|| "attachment size overflow".to_owned())
    })?;
    if retained_bytes.saturating_add(selected_bytes) > MAX_OUTGOING_ATTACHMENT_BYTES {
        return Err("attachments exceed the combined 18 MiB limit".into());
    }
    Ok(())
}

fn validate_draft_metadata(draft: &LocalDraftMetadata) -> Result<(), String> {
    if draft.to.len() > 20_000 || draft.cc.len() > 20_000 || draft.bcc.len() > 20_000 {
        return Err("draft recipient fields exceed the supported size".into());
    }
    if draft.editor_delta_json.len() > 2_000_000 {
        return Err("editable draft document exceeds the supported size".into());
    }
    Ok(())
}

fn attachment_content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("pdf") => "application/pdf",
        Some("txt") | Some("log") => "text/plain",
        Some("csv") => "text/csv",
        Some("html") | Some("htm") => "text/html",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("zip") => "application/zip",
        Some("ics") => "text/calendar",
        Some("vcf") => "text/vcard",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("pptx") => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    }
}

struct AccountSyncReport {
    warnings: Vec<String>,
    catalog_messages_remaining: u32,
    delta_mailboxes_synchronized: u32,
    full_mailboxes_reconciled: u32,
    qresync_mailboxes_synchronized: u32,
}

async fn synchronize_pending_drafts(
    database_path: &str,
    account: &MailAccount,
    credential: MailCredential<'_>,
) -> Result<(u32, Vec<String>), String> {
    let store = open_profile_store(database_path)?;
    let pending = store
        .pending_draft_operations(&account.id)
        .map_err(|error| error.to_string())?;
    if pending.is_empty() {
        return Ok((0, Vec::new()));
    }
    let mut warnings = Vec::new();
    let mut operations = Vec::with_capacity(pending.len());
    for pending_operation in pending {
        let previous_remote = pending_operation
            .previous_remote
            .map(|remote| RemoteDraftIdentity {
                remote_mailbox: remote.remote_mailbox,
                uid_validity: remote.uid_validity,
                remote_uid: remote.remote_uid,
            });
        let message = match pending_operation.action {
            PendingDraftAction::Delete => None,
            PendingDraftAction::Upsert => {
                let result = (|| -> Result<OutgoingMessage, String> {
                    let summary = store
                        .message_summary(&pending_operation.message_id)
                        .map_err(|error| error.to_string())?;
                    let body = store
                        .message_body(&pending_operation.message_id)
                        .map_err(|error| error.to_string())?;
                    let draft = store
                        .local_draft_metadata(&pending_operation.message_id)
                        .map_err(|error| error.to_string())?
                        .ok_or_else(|| "queued draft metadata is missing".to_owned())?;
                    let attachment_ids = store
                        .list_attachments(&pending_operation.message_id)
                        .map_err(|error| error.to_string())?
                        .into_iter()
                        .map(|attachment| attachment.id.to_string())
                        .collect::<Vec<_>>();
                    let attachments = load_stored_outgoing_attachments(
                        &store,
                        Path::new(database_path),
                        &attachment_ids,
                    )?;
                    validate_outgoing_attachment_set(&attachments)?;
                    let plain_text = body.plain_text.unwrap_or_default();
                    let sanitized_html = body
                        .sanitized_html
                        .unwrap_or_else(|| plain_text_as_html(&plain_text));
                    Ok(OutgoingMessage {
                        to: draft_recipient_list(&draft.to),
                        cc: draft_recipient_list(&draft.cc),
                        bcc: draft_recipient_list(&draft.bcc),
                        subject: summary.subject,
                        plain_text,
                        sanitized_html,
                        attachments,
                        high_importance: summary.flags.contains(&MessageFlag::Flagged),
                    })
                })();
                match result {
                    Ok(message) => Some(message),
                    Err(error) => {
                        warnings.push(format!("Entwurf {}: {error}", pending_operation.message_id));
                        continue;
                    }
                }
            }
        };
        operations.push(RemoteDraftOperation {
            local_key: pending_operation.message_id.to_string(),
            target_mailbox: pending_operation.target_mailbox,
            previous_remote,
            message,
        });
    }
    drop(store);
    if operations.is_empty() {
        return Ok((0, warnings));
    }
    let report = apply_draft_operations(account, credential, &operations)
        .await
        .map_err(|error| error.to_string())?;
    let synchronized = u32::try_from(report.applied.len()).unwrap_or(u32::MAX);
    let mut store = open_profile_store(database_path)?;
    for applied in report.applied {
        let message_id = MessageId::parse(&applied.local_key).map_err(|error| error.to_string())?;
        let uploaded_remote = applied.uploaded_remote.map(|remote| RemoteMessageMetadata {
            message_id: message_id.clone(),
            account_id: account.id.clone(),
            remote_mailbox: remote.remote_mailbox,
            uid_validity: remote.uid_validity,
            remote_uid: remote.remote_uid,
            catalog_complete: true,
            body_requested: true,
            body_complete: true,
        });
        store
            .complete_draft_operation(&message_id, uploaded_remote.as_ref())
            .map_err(|error| error.to_string())?;
    }
    warnings.extend(
        report
            .failed
            .into_iter()
            .map(|failure| format!("Entwurf {}: {}", failure.local_key, failure.error)),
    );
    Ok((synchronized, warnings))
}

fn draft_recipient_list(value: &str) -> Vec<String> {
    value
        .split([',', ';'])
        .map(str::trim)
        .filter(|recipient| !recipient.is_empty())
        .map(str::to_owned)
        .collect()
}

async fn synchronize_account(
    database_path: &str,
    account: &MailAccount,
) -> Result<AccountSyncReport, String> {
    let credential = load_account_credential(Path::new(database_path), &account.id).await?;
    let (_, mut warnings) =
        synchronize_pending_drafts(database_path, account, credential.connector_credential())
            .await?;
    let pending = open_profile_store(database_path)?
        .pending_mail_mutations(&account.id)
        .map_err(|error| error.to_string())?;
    if !pending.is_empty() {
        let remote_mutations = pending
            .iter()
            .map(|mutation| RemoteMutation {
                local_key: mutation.message_id.to_string(),
                source_mailbox: mutation.source_mailbox.clone(),
                target_mailbox: mutation.target_mailbox.clone(),
                uid_validity: mutation.uid_validity,
                remote_uid: mutation.remote_uid,
                seen: mutation.seen,
                flagged: mutation.flagged,
            })
            .collect::<Vec<_>>();
        let report = apply_mailbox_mutations(
            account,
            credential.connector_credential(),
            &remote_mutations,
        )
        .await
        .map_err(|error| error.to_string())?;
        let mut store = open_profile_store(database_path)?;
        for applied in report.applied {
            let message_id =
                MessageId::parse(applied.local_key).map_err(|error| error.to_string())?;
            store
                .complete_mail_mutation(&message_id, applied.moved)
                .map_err(|error| error.to_string())?;
        }
        warnings.extend(
            report
                .failed
                .into_iter()
                .map(|failure| format!("Änderung {}: {}", failure.local_key, failure.error)),
        );
    }

    let pending_message_ids = open_profile_store(database_path)?
        .pending_mail_mutations(&account.id)
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|mutation| mutation.message_id)
        .collect::<HashSet<_>>();
    let now = current_timestamp_ms()?;
    let stored_mailbox_states = open_profile_store(database_path)?
        .remote_mailbox_sync_states(&account.id)
        .map_err(|error| error.to_string())?;
    let mailbox_checkpoints = stored_mailbox_states
        .iter()
        .map(|state| RemoteMailboxCheckpoint {
            remote_mailbox: state.remote_mailbox.clone(),
            uid_validity: state.uid_validity,
            uid_next: state.uid_next,
            highest_modseq: state.highest_modseq,
            catalog_complete: state.catalog_complete,
            force_full_reconcile: !state.catalog_complete
                || now.saturating_sub(state.last_full_reconcile_at_ms)
                    >= FULL_MAILBOX_RECONCILE_INTERVAL_MS,
        })
        .collect::<Vec<_>>();
    let known_messages = open_profile_store(database_path)?
        .remote_messages_for_account(&account.id)
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|metadata| KnownRemoteMessage {
            local_key: metadata.message_id.to_string(),
            remote_mailbox: metadata.remote_mailbox,
            uid_validity: metadata.uid_validity,
            uid: metadata.remote_uid,
            needs_catalog_refresh: !metadata.catalog_complete,
            needs_body_refresh: metadata.body_requested && !metadata.body_complete,
        })
        .collect::<Vec<_>>();
    let synchronized = synchronize_mailboxes(
        account,
        credential.connector_credential(),
        &known_messages,
        &mailbox_checkpoints,
        25,
    )
    .await
    .map_err(|error| error.to_string())?;
    let mut store = open_profile_store(database_path)?;
    let existing_mailboxes = store
        .list_mailboxes(&account.id)
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|mailbox| (mailbox.id.to_string(), mailbox))
        .collect::<HashMap<_, _>>();
    let mailboxes = synchronized
        .mailboxes
        .iter()
        .map(|remote| {
            let id = remote_mailbox_id(account, &remote.remote_name)?;
            let (unread_count, total_count) = existing_mailboxes
                .get(id.as_str())
                .map_or((0, 0), |mailbox| {
                    (mailbox.unread_count, mailbox.total_count)
                });
            Ok(Mailbox {
                id,
                account_id: account.id.clone(),
                display_name: remote.remote_name.clone(),
                role: remote.role,
                unread_count,
                total_count,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    store
        .save_mailboxes(&mailboxes)
        .map_err(|error| error.to_string())?;
    let mut catalog_messages_remaining = 0_u32;
    for state in &synchronized.mailbox_states {
        if state.full_reconcile {
            let removed_attachments = store
                .reconcile_remote_mailbox(
                    &account.id,
                    &state.remote_mailbox,
                    state.uid_validity,
                    &state.active_uids,
                )
                .map_err(|error| error.to_string())?;
            remove_attachment_objects(Path::new(database_path), &removed_attachments);
        } else if !state.vanished_uids.is_empty() {
            let removed_attachments = store
                .remove_vanished_remote_messages(
                    &account.id,
                    &state.remote_mailbox,
                    state.uid_validity,
                    &state.vanished_uids,
                )
                .map_err(|error| error.to_string())?;
            remove_attachment_objects(Path::new(database_path), &removed_attachments);
        }
        catalog_messages_remaining = catalog_messages_remaining
            .saturating_add(u32::try_from(state.catalog_remaining).unwrap_or(u32::MAX));
    }
    for update in synchronized.flag_updates {
        let message_id = MessageId::parse(update.local_key).map_err(|error| error.to_string())?;
        if pending_message_ids.contains(&message_id) {
            continue;
        }
        store
            .update_remote_message_flags(&message_id, &update.flags)
            .map_err(|error| error.to_string())?;
    }
    for remote in synchronized.messages {
        let message_id = remote_message_id(account, &remote)?;
        if pending_message_ids.contains(&message_id) {
            continue;
        }
        cache_remote_message(
            database_path,
            account,
            remote,
            message_id,
            now,
            &mut store,
            &mut warnings,
        )?;
    }
    let previous_states = stored_mailbox_states
        .into_iter()
        .map(|state| (state.remote_mailbox.clone(), state))
        .collect::<HashMap<_, _>>();
    for state in &synchronized.mailbox_states {
        let last_full_reconcile_at_ms = if state.full_reconcile {
            now
        } else {
            previous_states
                .get(&state.remote_mailbox)
                .map_or(now, |previous| previous.last_full_reconcile_at_ms)
        };
        store
            .save_remote_mailbox_sync_state(&RemoteMailboxSyncState {
                account_id: account.id.clone(),
                remote_mailbox: state.remote_mailbox.clone(),
                uid_validity: state.uid_validity,
                uid_next: state.uid_next,
                highest_modseq: state.highest_modseq,
                catalog_complete: state.catalog_remaining == 0,
                last_full_reconcile_at_ms,
            })
            .map_err(|error| error.to_string())?;
    }
    store
        .update_account_last_sync(&account.id, now)
        .map_err(|error| error.to_string())?;
    let full_mailboxes_reconciled = synchronized
        .mailbox_states
        .iter()
        .filter(|state| state.full_reconcile)
        .count();
    let delta_mailboxes_synchronized = synchronized
        .mailbox_states
        .len()
        .saturating_sub(full_mailboxes_reconciled);
    let qresync_mailboxes_synchronized = synchronized
        .mailbox_states
        .iter()
        .filter(|state| state.qresync_used)
        .count();
    Ok(AccountSyncReport {
        warnings,
        catalog_messages_remaining,
        delta_mailboxes_synchronized: u32::try_from(delta_mailboxes_synchronized)
            .unwrap_or(u32::MAX),
        full_mailboxes_reconciled: u32::try_from(full_mailboxes_reconciled).unwrap_or(u32::MAX),
        qresync_mailboxes_synchronized: u32::try_from(qresync_mailboxes_synchronized)
            .unwrap_or(u32::MAX),
    })
}

fn cache_remote_message(
    database_path: &str,
    account: &MailAccount,
    remote: maicenta_mail_connector::RemoteMessage,
    message_id: MessageId,
    now: i64,
    store: &mut SqliteMailStore,
    warnings: &mut Vec<String>,
) -> Result<MessageDto, String> {
    let remote_attachment_parts = remote.attachments.clone();
    let remote_body_requested = remote.body_requested;
    let remote_body_complete = remote.body_complete;
    let rendered = MessageRenderer
        .render(&remote.renderable_message, RenderPolicy::default())
        .unwrap_or_else(|_| RenderedMessage {
            subject: Some("(Unreadable message)".into()),
            from_address: None,
            from_display_name: None,
            to_recipients: String::new(),
            cc_recipients: String::new(),
            bcc_recipients: String::new(),
            date_ms: None,
            plain_text: Some(
                "This message could not be parsed safely. Its server copy was not changed.".into(),
            ),
            sanitized_html: None,
            blocked_remote_images: 0,
            attachment_count: 0,
            attachments: Vec::new(),
            attachments_complete: true,
        });
    let from_address = rendered
        .from_address
        .unwrap_or_else(|| "unknown@maicenta.invalid".into());
    let sender = rendered.from_display_name;
    let mut plain_text = remote_body_requested
        .then(|| rendered.plain_text.clone())
        .flatten();
    let sanitized_html = remote_body_requested
        .then(|| rendered.sanitized_html.clone())
        .flatten();
    if remote_body_requested && plain_text.is_none() && sanitized_html.is_none() {
        plain_text = Some("Message without a displayable text body.".into());
    }
    let preview = if remote_body_requested {
        compact_preview(plain_text.as_deref().unwrap_or("HTML message"), 160)
    } else {
        String::new()
    };
    let from = MailAddress::new(from_address, sender.clone())
        .or_else(|_| MailAddress::new("unknown@maicenta.invalid", sender))
        .map_err(|error| error.to_string())?;
    let summary = MessageSummary {
        id: message_id.clone(),
        account_id: account.id.clone(),
        mailbox_id: remote_mailbox_id(account, &remote.remote_mailbox)?,
        from,
        subject: rendered.subject.unwrap_or_else(|| "(No subject)".into()),
        preview,
        received_at_ms: rendered.date_ms.unwrap_or(now),
        flags: remote.flags,
        has_attachments: rendered.attachment_count > 0 || !remote_attachment_parts.is_empty(),
    };
    let body = MessageBody {
        message_id: message_id.clone(),
        plain_text,
        sanitized_html,
    };
    let metadata = RemoteMessageMetadata {
        message_id: message_id.clone(),
        account_id: account.id.clone(),
        remote_mailbox: remote.remote_mailbox,
        uid_validity: remote.uid_validity,
        remote_uid: remote.uid,
        catalog_complete: remote.catalog_complete,
        body_requested: remote_body_requested,
        body_complete: remote_body_complete,
    };
    let recipients = MessageRecipients {
        message_id: message_id.clone(),
        to: rendered.to_recipients,
        cc: rendered.cc_recipients,
        bcc: rendered.bcc_recipients,
    };
    if remote_body_requested && !remote_body_complete {
        warnings.push(format!(
            "Nachricht „{}“: Der darstellbare Inhalt konnte nicht vollständig selektiv geladen werden",
            summary.subject
        ));
    }
    if rendered.attachment_count > 0 && !rendered.attachments_complete {
        warnings.push(format!(
            "Nachricht „{}“: Anhänge überschreiten das lokale Limit von 20 Dateien oder 25 MiB und bleiben auf dem Server",
            summary.subject
        ));
    }
    let decoded_attachments = rendered
        .attachments
        .into_iter()
        .map(|attachment| OutgoingAttachment {
            file_name: attachment.file_name,
            content_type: attachment.content_type,
            body: attachment.body,
        })
        .collect::<Vec<_>>();
    let previous_attachments = store
        .list_attachments(&message_id)
        .map_err(|error| error.to_string())?;
    let reuse_previous_attachments = !remote_body_requested && !previous_attachments.is_empty();
    let mut attachments =
        persist_attachment_objects(Path::new(database_path), &message_id, &decoded_attachments)?;
    if attachments.len() == remote_attachment_parts.len() {
        for (attachment, remote_part) in attachments.iter_mut().zip(&remote_attachment_parts) {
            attachment.remote_section = Some(remote_part.section.clone());
            attachment.transfer_encoding = Some(remote_part.transfer_encoding.clone());
        }
    } else if attachments.is_empty() && !remote_attachment_parts.is_empty() {
        attachments = remote_attachment_parts
            .iter()
            .enumerate()
            .map(|(index, remote_part)| {
                let digest =
                    stable_remote_key(&format!("{}:{}", message_id.as_str(), remote_part.section));
                Ok(MessageAttachment {
                    id: AttachmentId::parse(format!("attachment.remote.{digest}"))
                        .map_err(|error| error.to_string())?,
                    message_id: message_id.clone(),
                    file_name: safe_attachment_file_name(
                        Some(&remote_part.file_name),
                        index + 1,
                        &remote_part.content_type,
                    ),
                    content_type: remote_part.content_type.clone(),
                    size_bytes: remote_part.decoded_size_hint,
                    object_key: None,
                    remote_section: Some(remote_part.section.clone()),
                    transfer_encoding: Some(remote_part.transfer_encoding.clone()),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
    }
    if reuse_previous_attachments {
        remove_attachment_objects(Path::new(database_path), &attachments);
        attachments.clone_from(&previous_attachments);
    }
    if let Err(error) =
        store.save_remote_message(&summary, &body, &recipients, &metadata, &attachments)
    {
        if !reuse_previous_attachments {
            remove_attachment_objects(Path::new(database_path), &attachments);
        }
        return Err(error.to_string());
    }
    let draft_metadata = (summary.flags.contains(&MessageFlag::Draft)
        && metadata.body_requested
        && metadata.body_complete
        && attachments.is_empty())
    .then(|| LocalDraftMetadata {
        message_id: summary.id.clone(),
        to: recipients.to.clone(),
        cc: recipients.cc.clone(),
        bcc: recipients.bcc.clone(),
        editor_delta_json: String::new(),
    });
    if let Some(draft) = &draft_metadata {
        store
            .save_synchronized_draft_metadata(draft)
            .map_err(|error| error.to_string())?;
    }
    let draft_synchronized = draft_metadata.is_some();
    if !reuse_previous_attachments {
        remove_attachment_objects(Path::new(database_path), &previous_attachments);
    }
    Ok(message_dto(
        summary,
        body,
        recipients,
        attachments,
        draft_metadata,
        draft_synchronized,
    ))
}

fn remote_message_id(
    account: &MailAccount,
    remote: &maicenta_mail_connector::RemoteMessage,
) -> Result<MessageId, String> {
    let value = if remote.mailbox_role == MailboxRole::Inbox {
        format!("{}.{}.{}", account.id, remote.uid_validity, remote.uid)
    } else {
        format!(
            "{}.{}.{}.{}",
            account.id,
            stable_remote_key(&remote.remote_mailbox),
            remote.uid_validity,
            remote.uid
        )
    };
    MessageId::parse(value).map_err(|error| error.to_string())
}

fn mail_account_from_input(input: MailAccountInput) -> Result<MailAccount, String> {
    let id = AccountId::parse(input.id).map_err(|error| error.to_string())?;
    if id.as_str() == DEMO_ACCOUNT_ID {
        return Err("the reserved local profile identifier cannot be used".into());
    }
    let display_name = validated_text(input.display_name, "account display name", 100)?;
    let imap_host = validated_server_name(input.imap_host, "IMAP server")?;
    let smtp_host = validated_server_name(input.smtp_host, "SMTP server")?;
    let imap_username = validated_text(input.imap_username, "IMAP username", 320)?;
    let smtp_username = validated_text(input.smtp_username, "SMTP username", 320)?;
    if input.imap_port == 0 || input.smtp_port == 0 {
        return Err("mail server ports must be between 1 and 65535".into());
    }
    Ok(MailAccount {
        id,
        email: MailAddress::new(input.email, Some(display_name.clone()))
            .map_err(|error| error.to_string())?,
        display_name,
        imap_host,
        imap_port: input.imap_port,
        imap_security: parse_transport_security(&input.imap_security)?,
        imap_username,
        smtp_host,
        smtp_port: input.smtp_port,
        smtp_security: parse_transport_security(&input.smtp_security)?,
        smtp_username,
        last_sync_at_ms: None,
    })
}

fn parse_transport_security(value: &str) -> Result<TransportSecurity, String> {
    match value {
        "tls" => Ok(TransportSecurity::Tls),
        "starttls" => Ok(TransportSecurity::StartTls),
        _ => Err("transport security must be tls or starttls".into()),
    }
}

fn transport_security_name(value: TransportSecurity) -> &'static str {
    match value {
        TransportSecurity::Tls => "tls",
        TransportSecurity::StartTls => "starttls",
    }
}

fn validated_server_name(value: String, field: &str) -> Result<String, String> {
    let value = validated_text(value, field, 253)?;
    if value.contains(char::is_whitespace) || value.contains('/') || value.contains(':') {
        Err(format!("{field} must be a hostname without scheme or port"))
    } else {
        Ok(value)
    }
}

fn validated_text(value: String, field: &str, maximum: usize) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > maximum {
        Err(format!("{field} must contain 1 to {maximum} characters"))
    } else {
        Ok(trimmed.to_owned())
    }
}

fn remote_mailbox_id(account: &MailAccount, remote_name: &str) -> Result<MailboxId, String> {
    MailboxId::parse(format!(
        "{}.mailbox.{}",
        account.id,
        stable_remote_key(remote_name)
    ))
    .map_err(|error| error.to_string())
}

fn compact_preview(value: &str, maximum: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= maximum {
        normalized
    } else {
        format!("{}…", normalized.chars().take(maximum).collect::<String>())
    }
}

fn current_timestamp_ms() -> Result<i64, String> {
    let milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_millis();
    i64::try_from(milliseconds).map_err(|error| error.to_string())
}

fn validated_display_name(value: String) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 100 {
        Err("mailbox display name must contain 1 to 100 characters".into())
    } else {
        Ok(trimmed.to_owned())
    }
}

fn upgrade_prototype_content(store: &mut SqliteMailStore) -> Result<(), String> {
    let message_id =
        MessageId::parse("demo.open-source-weekly").map_err(|error| error.to_string())?;
    let Ok(existing) = store.message_body(&message_id) else {
        return Ok(());
    };
    if existing.sanitized_html.is_some() {
        return Ok(());
    }

    let body = prototype_body(
        "open-source-weekly",
        message_id,
        existing.plain_text.as_deref().unwrap_or_default(),
    )?;
    store.save_body(&body).map_err(|error| error.to_string())
}

fn load_snapshot(store: &SqliteMailStore) -> Result<WorkspaceSnapshot, String> {
    let mail_accounts = store
        .list_mail_accounts()
        .map_err(|error| error.to_string())?;
    let mail_account_dtos = mail_accounts
        .iter()
        .cloned()
        .map(|account| {
            let provider = store
                .get(&account.id, OAUTH_PROVIDER_KEY)
                .map_err(|error| error.to_string())?;
            Ok(mail_account_dto(account, provider))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut account_ids =
        vec![AccountId::parse(DEMO_ACCOUNT_ID).map_err(|error| error.to_string())?];
    account_ids.extend(mail_accounts.iter().map(|account| account.id.clone()));
    let mut mailboxes = Vec::new();
    for account_id in account_ids {
        mailboxes.extend(
            store
                .list_mailboxes(&account_id)
                .map_err(|error| error.to_string())?,
        );
    }
    let known_mailbox_ids = mailboxes
        .iter()
        .map(|mailbox| mailbox.id.as_str())
        .collect::<HashSet<_>>();
    let favorite_mailbox_ids = match store
        .favorite_mailbox_ids()
        .map_err(|error| error.to_string())?
    {
        Some(ids) => ids
            .into_iter()
            .filter(|id| known_mailbox_ids.contains(id.as_str()))
            .map(|id| id.to_string())
            .collect(),
        None => mailboxes
            .iter()
            .filter(|mailbox| {
                matches!(
                    mailbox.role,
                    MailboxRole::Inbox | MailboxRole::Drafts | MailboxRole::Sent
                )
            })
            .take(3)
            .map(|mailbox| mailbox.id.to_string())
            .collect(),
    };
    let mut messages = Vec::new();

    for mailbox in &mailboxes {
        let summaries = store
            .list_message_page(&mailbox.id, 0, INITIAL_MESSAGES_PER_MAILBOX)
            .map_err(|error| error.to_string())?;
        messages.extend(message_dtos(store, summaries)?);
    }

    Ok(WorkspaceSnapshot {
        dark_mode_enabled: store
            .dark_mode_enabled()
            .map_err(|error| error.to_string())?,
        favorite_mailbox_ids,
        mailboxes: mailboxes.into_iter().map(mailbox_dto).collect(),
        messages,
        calendar_events: store
            .list_calendar_events()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(calendar_event_dto)
            .collect(),
        tasks: store
            .list_tasks()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(task_dto)
            .collect(),
        contacts: store
            .list_contacts()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(contact_dto)
            .collect(),
        mail_accounts: mail_account_dtos,
        sync_warnings: Vec::new(),
        catalog_messages_remaining: 0,
        delta_mailboxes_synchronized: 0,
        full_mailboxes_reconciled: 0,
        qresync_mailboxes_synchronized: 0,
        pending_mail_operations: store
            .pending_mail_mutation_count()
            .map_err(|error| error.to_string())?,
    })
}

fn message_dtos(
    store: &SqliteMailStore,
    summaries: Vec<MessageSummary>,
) -> Result<Vec<MessageDto>, String> {
    let synchronized_drafts = synchronized_draft_ids(store, &summaries)?;
    summaries
        .into_iter()
        .map(|summary| {
            let body = store
                .message_body(&summary.id)
                .map_err(|error| error.to_string())?;
            let attachments = store
                .list_attachments(&summary.id)
                .map_err(|error| error.to_string())?;
            let recipients = store
                .message_recipients(&summary.id)
                .map_err(|error| error.to_string())?;
            let draft = store
                .local_draft_metadata(&summary.id)
                .map_err(|error| error.to_string())?;
            let draft_synchronized = synchronized_drafts.contains(summary.id.as_str());
            Ok(message_dto(
                summary,
                body,
                recipients,
                attachments,
                draft,
                draft_synchronized,
            ))
        })
        .collect()
}

fn synchronized_draft_ids(
    store: &SqliteMailStore,
    summaries: &[MessageSummary],
) -> Result<HashSet<String>, String> {
    let account_ids = summaries
        .iter()
        .filter(|summary| summary.flags.contains(&MessageFlag::Draft))
        .map(|summary| summary.account_id.clone())
        .collect::<HashSet<_>>();
    let mut remote_ids = HashSet::new();
    let mut pending_ids = HashSet::new();
    for account_id in account_ids {
        remote_ids.extend(
            store
                .remote_messages_for_account(&account_id)
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|remote| remote.message_id.to_string()),
        );
        pending_ids.extend(
            store
                .pending_draft_operations(&account_id)
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|pending| pending.message_id.to_string()),
        );
    }
    remote_ids.retain(|message_id| !pending_ids.contains(message_id));
    Ok(remote_ids)
}

fn mailbox_dto(mailbox: Mailbox) -> MailboxDto {
    MailboxDto {
        id: mailbox.id.to_string(),
        account_id: mailbox.account_id.to_string(),
        display_name: mailbox.display_name,
        role: mailbox_role_name(mailbox.role).into(),
        unread_count: mailbox.unread_count,
        total_count: mailbox.total_count,
    }
}

fn message_dto(
    summary: MessageSummary,
    body: MessageBody,
    recipients: MessageRecipients,
    attachments: Vec<MessageAttachment>,
    draft_metadata: Option<LocalDraftMetadata>,
    draft_synchronized: bool,
) -> MessageDto {
    let unread = summary.is_unread();
    let flagged = summary.flags.contains(&MessageFlag::Flagged);
    let draft = summary.flags.contains(&MessageFlag::Draft);
    let plain_text = body.plain_text.clone().unwrap_or_default();
    let sender = summary
        .from
        .display_name()
        .unwrap_or(summary.from.address())
        .to_owned();
    let body = body
        .sanitized_html
        .or_else(|| body.plain_text.map(|text| plain_text_as_html(&text)))
        .unwrap_or_default();
    let editable_draft = draft_metadata.is_some();
    let (draft_to, draft_cc, draft_bcc, editor_delta_json) = draft_metadata.map_or_else(
        || (String::new(), String::new(), String::new(), String::new()),
        |metadata| {
            (
                metadata.to,
                metadata.cc,
                metadata.bcc,
                metadata.editor_delta_json,
            )
        },
    );

    MessageDto {
        id: summary.id.to_string(),
        account_id: summary.account_id.to_string(),
        mailbox_id: summary.mailbox_id.to_string(),
        sender,
        email: summary.from.address().to_owned(),
        subject: summary.subject,
        preview: summary.preview,
        body,
        plain_text,
        received_at_ms: summary.received_at_ms,
        unread,
        flagged,
        draft,
        editable_draft,
        draft_synchronized,
        draft_to,
        draft_cc,
        draft_bcc,
        to_recipients: recipients.to,
        cc_recipients: recipients.cc,
        bcc_recipients: recipients.bcc,
        editor_delta_json,
        has_attachment: summary.has_attachments,
        attachments: attachments
            .into_iter()
            .map(|attachment| {
                let available_locally = attachment.is_available_locally();
                MessageAttachmentDto {
                    id: attachment.id.to_string(),
                    file_name: attachment.file_name,
                    content_type: attachment.content_type,
                    size_bytes: u32::try_from(attachment.size_bytes).unwrap_or(u32::MAX),
                    available_locally,
                }
            })
            .collect(),
    }
}

fn calendar_event_dto(event: CalendarEvent) -> CalendarEventDto {
    CalendarEventDto {
        id: event.id.to_string(),
        title: event.title,
        starts_at_ms: event.starts_at_ms,
        ends_at_ms: event.ends_at_ms,
        location: event.location,
    }
}

fn task_dto(task: TaskItem) -> TaskDto {
    TaskDto {
        id: task.id.to_string(),
        title: task.title,
        due_at_ms: task.due_at_ms,
        completed: task.completed,
    }
}

fn contact_dto(contact: Contact) -> ContactDto {
    ContactDto {
        id: contact.id.to_string(),
        name: contact.name,
        email: contact.email.address().to_owned(),
    }
}

fn mail_account_dto(account: MailAccount, oauth_provider: Option<String>) -> MailAccountDto {
    let authentication = if oauth_provider.is_some() {
        "oauth2"
    } else {
        "password"
    };
    MailAccountDto {
        id: account.id.to_string(),
        display_name: account.display_name,
        email: account.email.address().to_owned(),
        imap_host: account.imap_host,
        imap_port: account.imap_port,
        imap_security: transport_security_name(account.imap_security).into(),
        imap_username: account.imap_username,
        smtp_host: account.smtp_host,
        smtp_port: account.smtp_port,
        smtp_security: transport_security_name(account.smtp_security).into(),
        smtp_username: account.smtp_username,
        authentication: authentication.into(),
        oauth_provider,
        last_sync_at_ms: account.last_sync_at_ms,
    }
}

fn seed_prototype(store: &mut SqliteMailStore, account_id: &AccountId) -> Result<(), String> {
    let inbox_id = MailboxId::parse(DEMO_INBOX_ID).map_err(|error| error.to_string())?;
    let mailboxes = [
        prototype_mailbox(
            account_id,
            &inbox_id,
            "Posteingang",
            MailboxRole::Inbox,
            2,
            5,
        ),
        prototype_mailbox(
            account_id,
            &MailboxId::parse("personal.drafts").map_err(|error| error.to_string())?,
            "Entwürfe",
            MailboxRole::Drafts,
            0,
            1,
        ),
        prototype_mailbox(
            account_id,
            &MailboxId::parse("personal.sent").map_err(|error| error.to_string())?,
            "Gesendet",
            MailboxRole::Sent,
            0,
            0,
        ),
        prototype_mailbox(
            account_id,
            &MailboxId::parse("personal.archive").map_err(|error| error.to_string())?,
            "Archiv",
            MailboxRole::Archive,
            0,
            0,
        ),
        prototype_mailbox(
            account_id,
            &MailboxId::parse("personal.trash").map_err(|error| error.to_string())?,
            "Papierkorb",
            MailboxRole::Trash,
            0,
            0,
        ),
    ];
    store
        .save_mailboxes(&mailboxes)
        .map_err(|error| error.to_string())?;

    let messages = prototype_messages(account_id, &inbox_id)?;
    store
        .save_summaries(
            &messages
                .iter()
                .map(|item| item.0.clone())
                .collect::<Vec<_>>(),
        )
        .map_err(|error| error.to_string())?;
    for (_, body) in messages {
        store.save_body(&body).map_err(|error| error.to_string())?;
    }

    Ok(())
}

fn prototype_mailbox(
    account_id: &AccountId,
    id: &MailboxId,
    display_name: &str,
    role: MailboxRole,
    unread_count: u32,
    total_count: u32,
) -> Mailbox {
    Mailbox {
        id: id.clone(),
        account_id: account_id.clone(),
        display_name: display_name.into(),
        role,
        unread_count,
        total_count,
    }
}

fn prototype_messages(
    account_id: &AccountId,
    mailbox_id: &MailboxId,
) -> Result<Vec<(MessageSummary, MessageBody)>, String> {
    let values = [
        (
            "welcome",
            "MAICENTA Team",
            "hello@maicenta.local",
            "Willkommen bei MAICENTA",
            "Dein lokaler Workspace ist bereit für den ersten Rundgang.",
            "Hallo,\n\nwillkommen beim ersten MAICENTA-Prototypen. Diese Oberfläche zeigt die geplante Arbeitsweise für E-Mail, Kalender, Aufgaben und Kontakte.\n\nDie Beispieldaten werden jetzt aus der lokalen SQLite-Datenbank geladen.\n\nViele Grüße\nDas MAICENTA Team",
            1_785_746_520_000_i64,
            vec![],
            false,
        ),
        (
            "planning",
            "Anna Schneider",
            "anna@example.org",
            "Projektplanung für diese Woche",
            "Ich habe die offenen Punkte für unseren Termin zusammengefasst.",
            "Hallo,\n\nich habe die offenen Punkte für unseren Termin am Donnerstag zusammengefasst. Im Anhang findest du die aktuelle Übersicht.\n\nViele Grüße\nAnna",
            1_785_741_480_000,
            vec![MessageFlag::Flagged],
            true,
        ),
        (
            "calendar-reminder",
            "Kalender",
            "calendar@maicenta.local",
            "Erinnerung: Team-Stand-up",
            "Der Termin beginnt morgen um 09:30 Uhr.",
            "Erinnerung\n\nTeam-Stand-up\nMorgen, 09:30–10:00\nBesprechungsraum Nord",
            1_785_661_200_000,
            vec![MessageFlag::Seen],
            false,
        ),
        (
            "design",
            "Jonas Weber",
            "jonas@example.org",
            "Re: Design-Entwurf",
            "Die klare Navigation gefällt mir. Zwei Anmerkungen habe ich noch.",
            "Hallo,\n\ndie klare Navigation gefällt mir. Zwei kleine Anmerkungen habe ich noch direkt im Dokument ergänzt.\n\nBeste Grüße\nJonas",
            1_785_657_600_000,
            vec![MessageFlag::Seen],
            true,
        ),
        (
            "open-source-weekly",
            "Open Source Weekly",
            "newsletter@example.org",
            "Local-first software in practice",
            "This week: resilient sync, portable data and open protocols.",
            "Local-first software in practice\n\nThis week we look at resilient synchronization, portable user data, and open protocols.",
            1_785_398_400_000,
            vec![MessageFlag::Seen],
            false,
        ),
    ];

    values
        .into_iter()
        .map(
            |(id, sender, email, subject, preview, body, received_at_ms, flags, attachment)| {
                let message_id =
                    MessageId::parse(format!("demo.{id}")).map_err(|error| error.to_string())?;
                Ok((
                    MessageSummary {
                        id: message_id.clone(),
                        account_id: account_id.clone(),
                        mailbox_id: mailbox_id.clone(),
                        from: MailAddress::new(email, Some(sender.into()))
                            .map_err(|error| error.to_string())?,
                        subject: subject.into(),
                        preview: preview.into(),
                        received_at_ms,
                        flags,
                        has_attachments: attachment,
                    },
                    prototype_body(id, message_id, body)?,
                ))
            },
        )
        .collect()
}

fn prototype_body(
    id: &str,
    message_id: MessageId,
    plain_text: &str,
) -> Result<MessageBody, String> {
    if id != "open-source-weekly" {
        return Ok(MessageBody {
            message_id,
            plain_text: Some(plain_text.into()),
            sanitized_html: None,
        });
    }

    let raw_message = format!(
        "From: Open Source Weekly <newsletter@example.org>\r\n\
         To: demo@maicenta.local\r\n\
         Subject: Local-first software in practice\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: multipart/alternative; boundary=maicenta-demo\r\n\
         \r\n\
         --maicenta-demo\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         \r\n\
         {plain_text}\r\n\
         --maicenta-demo\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         \r\n\
         <div style=\"max-width:640px;font-family:Arial;color:#242424\">\
           <div style=\"background-color:#0f5fae;color:#ffffff;padding:18px\">\
             <strong style=\"font-size:20px\">Open Source Weekly</strong>\
           </div>\
           <div style=\"padding:20px;border:1px solid #d5d9de\">\
             <h2 style=\"color:#0f5fae\">Local-first software in practice</h2>\
             <p>This week we look at <strong>resilient synchronization</strong>, \
             portable user data, and open protocols.</p>\
             <table width=\"100%\" cellpadding=\"8\" style=\"border-collapse:collapse\">\
               <tr><td style=\"background-color:#eef5fb\">Offline-first</td>\
               <td style=\"background-color:#f7f7f7\">Open standards</td></tr>\
             </table>\
             <p><a href=\"https://example.org/article\">Read the article</a></p>\
             <img src=\"https://tracker.example/pixel.gif\" width=\"1\" height=\"1\">\
             <script>tracking()</script>\
           </div>\
         </div>\r\n\
         --maicenta-demo--\r\n"
    );
    let rendered = MessageRenderer
        .render(raw_message.as_bytes(), RenderPolicy::default())
        .map_err(|error| error.to_string())?;

    Ok(MessageBody {
        message_id,
        plain_text: rendered.plain_text,
        sanitized_html: rendered.sanitized_html,
    })
}

fn plain_text_as_html(text: &str) -> String {
    let escaped = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "<br>");
    format!("<div style=\"font-family:Arial;font-size:14px;line-height:1.55\">{escaped}</div>")
}

const fn mailbox_role_name(role: MailboxRole) -> &'static str {
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

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::SystemTime,
    };

    use super::{
        ACCOUNT_PASSWORD_KEY, LocalCalendarEventInput, LocalContactInput, LocalMessageInput,
        LocalTaskInput, MICROSOFT_TOKEN_ENDPOINT, MailAccountInput, OAUTH_REFRESH_TOKEN_KEY,
        OAuthTokenInput, attachment_content_type, attachment_object_path, cache_remote_message,
        create_local_mailbox, delete_local_mailbox, export_local_attachment, export_profile,
        import_profile, load_mailbox_messages, load_outgoing_attachments, mail_account_from_input,
        open_profile_store, open_workspace, remote_mailbox_id, remote_message_id,
        rename_local_mailbox, save_dark_mode, save_favorite_mailboxes, save_local_calendar_event,
        save_local_contact, save_local_message, save_local_task, save_mail_account,
        save_oauth_mail_account, search_profile_messages, update_local_message,
    };
    use maicenta_application::{MailAccountStore, MailStore, SecretStore};
    use maicenta_domain::{AccountId, Mailbox, MailboxRole, MessageBody, MessageId};
    use maicenta_mail_connector::{RemoteAttachmentPart, RemoteMessage};

    static NEXT_DATABASE_ID: AtomicU64 = AtomicU64::new(0);

    fn local_draft_input(
        id: &str,
        subject: &str,
        attachment_paths: Vec<String>,
        retained_attachment_ids: Vec<String>,
    ) -> LocalMessageInput {
        LocalMessageInput {
            id: id.into(),
            account_id: "personal".into(),
            mailbox_id: "personal.drafts".into(),
            sender: "Entwurf".into(),
            email: "demo@maicenta.local".into(),
            subject: subject.into(),
            preview: "Bearbeitbarer Entwurf".into(),
            plain_text: "Bearbeitbarer Entwurf".into(),
            html_text: "<p><strong>Bearbeitbarer Entwurf</strong></p>".into(),
            attachment_paths,
            retained_attachment_ids,
            draft_to: "anna@example.org".into(),
            draft_cc: "copy@example.org".into(),
            draft_bcc: String::new(),
            editor_delta_json:
                "[{\"insert\":\"Bearbeitbarer Entwurf\",\"attributes\":{\"bold\":true}},{\"insert\":\"\\n\"}]"
                    .into(),
            received_at_ms: 1_785_830_400_000,
            unread: false,
            flagged: false,
            draft: true,
            has_attachment: true,
        }
    }

    #[test]
    fn seeds_then_reloads_the_same_workspace() {
        let path = temporary_database_path();
        let first = open_workspace(path.to_string_lossy().into_owned()).expect("first open");
        let newsletter_id = MessageId::parse("demo.open-source-weekly").expect("message id");
        let mut store = open_profile_store(&path).expect("reopen storage");
        store
            .save_body(&MessageBody {
                message_id: newsletter_id,
                plain_text: Some("Existing plain-text prototype body".into()),
                sanitized_html: None,
            })
            .expect("replace body with legacy prototype content");
        drop(store);

        let second = open_workspace(path.to_string_lossy().into_owned()).expect("second open");

        assert_eq!(first.mailboxes.len(), 5);
        assert_eq!(first.messages.len(), 5);
        assert_eq!(first.calendar_events.len(), 2);
        assert_eq!(first.tasks.len(), 3);
        assert_eq!(first.contacts.len(), 3);
        assert!(first.mail_accounts.is_empty());
        assert_eq!(
            first.favorite_mailbox_ids,
            ["personal.inbox", "personal.drafts", "personal.sent"]
        );
        assert!(!first.dark_mode_enabled);
        assert_eq!(second.mailboxes.len(), 5);
        assert_eq!(second.messages.len(), 5);
        assert!(second.messages.iter().any(|message| message.unread));
        let newsletter = second
            .messages
            .iter()
            .find(|message| message.id == "demo.open-source-weekly")
            .expect("newsletter");
        assert!(newsletter.body.contains("<table"));
        assert!(!newsletter.body.contains("<script"));
        assert!(!newsletter.body.contains("tracker.example"));

        save_favorite_mailboxes(
            path.to_string_lossy().into_owned(),
            vec!["personal.archive".into(), "personal.inbox".into()],
        )
        .expect("save favorite order");
        let reordered = open_workspace(path.to_string_lossy().into_owned()).expect("reopen");
        assert_eq!(
            reordered.favorite_mailbox_ids,
            ["personal.archive", "personal.inbox"]
        );
        save_dark_mode(path.to_string_lossy().into_owned(), true).expect("save dark mode");
        assert!(
            open_workspace(path.to_string_lossy().into_owned())
                .expect("reopen dark mode")
                .dark_mode_enabled
        );

        for suffix in ["", "-shm", "-wal"] {
            let _ = fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }

    #[test]
    fn escapes_plain_text_before_exposing_it_as_html() {
        assert_eq!(
            super::plain_text_as_html("Hello <script>\n& goodbye"),
            "<div style=\"font-family:Arial;font-size:14px;line-height:1.55\">Hello &lt;script&gt;<br>&amp; goodbye</div>"
        );
    }

    #[test]
    fn loads_bounded_local_mailbox_pages() {
        let path = temporary_database_path();
        open_workspace(path.to_string_lossy().into_owned()).expect("seed workspace");

        let first = load_mailbox_messages(
            path.to_string_lossy().into_owned(),
            "personal.inbox".into(),
            0,
            1,
        )
        .expect("first page");
        let second = load_mailbox_messages(
            path.to_string_lossy().into_owned(),
            "personal.inbox".into(),
            1,
            1,
        )
        .expect("second page");

        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_ne!(first[0].id, second[0].id);

        for suffix in ["", "-shm", "-wal"] {
            let _ = fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }

    #[test]
    fn rejects_attachment_object_paths_outside_the_profile() {
        let database_path = temporary_database_path();

        assert!(attachment_object_path(&database_path, "../secret.txt").is_err());
        assert!(attachment_object_path(&database_path, "/tmp/secret.txt").is_err());
        assert!(attachment_object_path(&database_path, "other/file.bin").is_err());
        assert!(attachment_object_path(&database_path, "attachments/file.bin").is_ok());
    }

    #[test]
    fn persists_user_mail_actions_across_workspace_restarts() {
        let path = temporary_database_path();
        let database_path = path.to_string_lossy().into_owned();
        let attachment_path = path.with_extension("attachment.pdf");
        fs::write(&attachment_path, b"persistent attachment").expect("write source attachment");
        open_workspace(database_path.clone()).expect("seed workspace");

        create_local_mailbox(
            database_path.clone(),
            "local.folder.follow-up".into(),
            "Später".into(),
        )
        .expect("create custom mailbox");
        save_local_message(
            database_path.clone(),
            LocalMessageInput {
                id: "local.composed-message".into(),
                account_id: "personal".into(),
                mailbox_id: "personal.drafts".into(),
                sender: "Entwurf".into(),
                email: "demo@maicenta.local".into(),
                subject: "Persistenter Entwurf".into(),
                preview: "Bleibt nach dem Neustart erhalten.".into(),
                plain_text: "Bleibt nach dem Neustart erhalten.".into(),
                html_text: "<p>Bleibt nach dem <strong>Neustart</strong> erhalten.</p>".into(),
                attachment_paths: vec![attachment_path.to_string_lossy().into_owned()],
                retained_attachment_ids: Vec::new(),
                draft_to: "anna@example.org".into(),
                draft_cc: String::new(),
                draft_bcc: String::new(),
                editor_delta_json: "[{\"insert\":\"Bleibt erhalten.\\n\"}]".into(),
                received_at_ms: 1_785_830_400_000,
                unread: true,
                flagged: true,
                draft: true,
                has_attachment: true,
            },
        )
        .expect("save composed message");
        update_local_message(
            database_path.clone(),
            "local.composed-message".into(),
            "local.folder.follow-up".into(),
            false,
            false,
        )
        .expect("move and update message");
        rename_local_mailbox(
            database_path.clone(),
            "local.folder.follow-up".into(),
            "Nachfassen".into(),
        )
        .expect("rename mailbox");

        let restarted = open_workspace(database_path.clone()).expect("restart workspace");
        let custom = restarted
            .mailboxes
            .iter()
            .find(|mailbox| mailbox.id == "local.folder.follow-up")
            .expect("persisted mailbox");
        assert_eq!(custom.display_name, "Nachfassen");
        assert_eq!((custom.total_count, custom.unread_count), (1, 0));
        let message = restarted
            .messages
            .iter()
            .find(|message| message.id == "local.composed-message")
            .expect("persisted message");
        assert_eq!(message.mailbox_id, "local.folder.follow-up");
        assert!(!message.unread);
        assert!(!message.flagged);
        assert!(message.body.contains("Bleibt nach dem"));
        assert!(message.body.contains("erhalten."));
        assert!(message.body.contains("<strong>Neustart</strong>"));
        assert!(message.draft);
        assert!(message.editable_draft);
        assert_eq!(message.draft_to, "anna@example.org");
        assert_eq!(
            message.editor_delta_json,
            "[{\"insert\":\"Bleibt erhalten.\\n\"}]"
        );
        assert_eq!(message.attachments.len(), 1);
        assert_eq!(message.attachments[0].content_type, "application/pdf");
        assert_eq!(message.attachments[0].size_bytes, 21);
        let export_path = path.with_extension("exported.pdf");
        export_local_attachment(
            database_path.clone(),
            message.attachments[0].id.clone(),
            export_path.to_string_lossy().into_owned(),
        )
        .expect("export attachment");
        assert_eq!(
            fs::read(&export_path).expect("read export"),
            b"persistent attachment"
        );

        delete_local_mailbox(
            database_path.clone(),
            "local.folder.follow-up".into(),
            "personal.inbox".into(),
        )
        .expect("delete custom mailbox");
        let after_delete = open_workspace(database_path).expect("restart after delete");
        assert!(
            after_delete
                .mailboxes
                .iter()
                .all(|mailbox| mailbox.id != "local.folder.follow-up")
        );
        assert!(after_delete.messages.iter().any(|message| {
            message.id == "local.composed-message" && message.mailbox_id == "personal.inbox"
        }));

        for suffix in ["", "-shm", "-wal"] {
            let _ = fs::remove_file(format!("{}{suffix}", path.display()));
        }
        let _ = fs::remove_file(attachment_path);
        let _ = fs::remove_file(export_path);
        let _ = fs::remove_dir_all(path.with_extension("objects"));
    }

    #[test]
    fn searches_full_message_bodies_through_the_bridge() {
        let path = temporary_database_path();
        let database_path = path.to_string_lossy().into_owned();
        open_workspace(database_path.clone()).expect("seed workspace");
        save_local_message(
            database_path.clone(),
            LocalMessageInput {
                id: "local.search.test".into(),
                account_id: "personal".into(),
                mailbox_id: "personal.sent".into(),
                sender: "Suchtest".into(),
                email: "search@example.org".into(),
                subject: "Nicht im Suchbegriff".into(),
                preview: "Kurze Vorschau".into(),
                plain_text: "Die einzigartige Quantennotiz steht nur im Nachrichtentext.".into(),
                html_text: "<p>Die einzigartige Quantennotiz steht nur im Nachrichtentext.</p>"
                    .into(),
                attachment_paths: Vec::new(),
                retained_attachment_ids: Vec::new(),
                draft_to: String::new(),
                draft_cc: String::new(),
                draft_bcc: String::new(),
                editor_delta_json: String::new(),
                received_at_ms: 1_785_830_400_000,
                unread: false,
                flagged: false,
                draft: false,
                has_attachment: false,
            },
        )
        .expect("save searchable message");

        let results = search_profile_messages(database_path, "quantennot".into(), true, 20)
            .expect("profile search");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "local.search.test");

        for suffix in ["", "-shm", "-wal"] {
            let _ = fs::remove_file(format!("{}{suffix}", path.display()));
        }
        let _ = fs::remove_dir_all(path.with_extension("objects"));
    }

    #[test]
    fn editing_a_draft_retains_its_attachment_and_composer_state() {
        let path = temporary_database_path();
        let database_path = path.to_string_lossy().into_owned();
        let attachment_path = path.with_extension("draft.txt");
        fs::write(&attachment_path, b"retained draft attachment").expect("write attachment");
        open_workspace(database_path.clone()).expect("seed workspace");

        let first = save_local_message(
            database_path.clone(),
            local_draft_input(
                "local.editable-draft",
                "Erste Fassung",
                vec![attachment_path.to_string_lossy().into_owned()],
                Vec::new(),
            ),
        )
        .expect("save initial draft");
        let attachment_id = first.attachments[0].id.clone();
        let edited = save_local_message(
            database_path.clone(),
            local_draft_input(
                "local.editable-draft",
                "Überarbeitete Fassung",
                Vec::new(),
                vec![attachment_id.clone()],
            ),
        )
        .expect("edit draft");

        assert_eq!(edited.attachments[0].id, attachment_id);
        assert_eq!(edited.draft_to, "anna@example.org");
        assert!(edited.editor_delta_json.contains("bold"));
        let reopened = open_workspace(database_path).expect("reopen workspace");
        let persisted = reopened
            .messages
            .iter()
            .find(|message| message.id == "local.editable-draft")
            .expect("persisted draft");
        assert_eq!(persisted.subject, "Überarbeitete Fassung");
        assert_eq!(persisted.attachments[0].id, attachment_id);
        assert!(persisted.editable_draft);

        for suffix in ["", "-shm", "-wal"] {
            let _ = fs::remove_file(format!("{}{suffix}", path.display()));
        }
        let _ = fs::remove_file(attachment_path);
        let _ = fs::remove_dir_all(path.with_extension("objects"));
    }

    #[test]
    fn persists_personal_workspace_actions_across_restarts() {
        let path = temporary_database_path();
        let database_path = path.to_string_lossy().into_owned();
        open_workspace(database_path.clone()).expect("seed workspace");

        save_local_calendar_event(
            database_path.clone(),
            LocalCalendarEventInput {
                id: "local.event.bridge".into(),
                title: "Bridge-Termin".into(),
                starts_at_ms: 1_785_830_400_000,
                ends_at_ms: 1_785_834_000_000,
                location: Some("Lokal".into()),
            },
        )
        .expect("save event");
        save_local_task(
            database_path.clone(),
            LocalTaskInput {
                id: "local.task.bridge".into(),
                title: "Bridge-Aufgabe".into(),
                due_at_ms: None,
                completed: true,
            },
        )
        .expect("save task");
        save_local_contact(
            database_path.clone(),
            LocalContactInput {
                id: "local.contact.bridge".into(),
                name: "Bridge Kontakt".into(),
                email: "bridge@example.org".into(),
            },
        )
        .expect("save contact");

        let restarted = open_workspace(database_path).expect("restart workspace");
        assert!(
            restarted
                .calendar_events
                .iter()
                .any(|event| event.id == "local.event.bridge")
        );
        assert!(
            restarted
                .tasks
                .iter()
                .any(|task| task.id == "local.task.bridge" && task.completed)
        );
        assert!(
            restarted
                .contacts
                .iter()
                .any(|contact| contact.email == "bridge@example.org")
        );

        for suffix in ["", "-shm", "-wal"] {
            let _ = fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }

    #[test]
    fn validates_and_loads_user_selected_attachments() {
        let path = temporary_database_path().with_extension("pdf");
        fs::write(&path, b"attachment bytes").expect("write attachment");

        let attachments = load_outgoing_attachments(&[path.to_string_lossy().into_owned()])
            .expect("load attachment");

        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].content_type, "application/pdf");
        assert_eq!(attachments[0].body, b"attachment bytes");
        assert_eq!(
            attachment_content_type(PathBuf::from("unknown.bin").as_path()),
            "application/octet-stream"
        );
        fs::remove_file(path).expect("remove attachment");
    }

    #[test]
    fn remote_message_ids_are_unique_across_mailboxes() {
        let account = mail_account_from_input(MailAccountInput {
            id: "work".into(),
            display_name: "Work".into(),
            email: "user@example.org".into(),
            imap_host: "imap.example.org".into(),
            imap_port: 993,
            imap_security: "tls".into(),
            imap_username: "user@example.org".into(),
            smtp_host: "smtp.example.org".into(),
            smtp_port: 587,
            smtp_security: "starttls".into(),
            smtp_username: "user@example.org".into(),
        })
        .expect("account");
        let inbox = RemoteMessage {
            remote_mailbox: "INBOX".into(),
            mailbox_role: MailboxRole::Inbox,
            uid_validity: 42,
            uid: 7,
            flags: Vec::new(),
            renderable_message: Vec::new(),
            attachments: Vec::new(),
            catalog_complete: true,
            body_requested: true,
            body_complete: true,
        };
        let sent = RemoteMessage {
            remote_mailbox: "Sent".into(),
            mailbox_role: MailboxRole::Sent,
            ..inbox.clone()
        };

        assert_eq!(
            remote_message_id(&account, &inbox)
                .expect("inbox id")
                .as_str(),
            "work.42.7"
        );
        assert_ne!(
            remote_message_id(&account, &inbox).expect("inbox id"),
            remote_message_id(&account, &sent).expect("sent id")
        );
    }

    #[test]
    fn caches_decoded_incoming_attachment_and_exports_it() {
        let path = temporary_database_path();
        let database_path = path.to_string_lossy().into_owned();
        open_workspace(database_path.clone()).expect("seed workspace");
        let account = mail_account_from_input(MailAccountInput {
            id: "incoming-test".into(),
            display_name: "Incoming Test".into(),
            email: "user@example.org".into(),
            imap_host: "imap.example.org".into(),
            imap_port: 993,
            imap_security: "tls".into(),
            imap_username: "user@example.org".into(),
            smtp_host: "smtp.example.org".into(),
            smtp_port: 587,
            smtp_security: "starttls".into(),
            smtp_username: "user@example.org".into(),
        })
        .expect("account");
        let remote = RemoteMessage {
            remote_mailbox: "INBOX".into(),
            mailbox_role: MailboxRole::Inbox,
            uid_validity: 42,
            uid: 9,
            flags: Vec::new(),
            renderable_message: br#"From: Anna <anna@example.org>
Subject: Incoming attachment
MIME-Version: 1.0
Content-Type: multipart/mixed; boundary=incoming

--incoming
Content-Type: text/plain; charset=utf-8

See attachment
--incoming
Content-Type: application/pdf
Content-Disposition: attachment; filename="report.pdf"
Content-Transfer-Encoding: base64

aW5jb21pbmcgYnl0ZXM=
--incoming--
"#
            .to_vec(),
            attachments: Vec::new(),
            catalog_complete: true,
            body_requested: true,
            body_complete: true,
        };
        let message_id = remote_message_id(&account, &remote).expect("message id");
        let mut store = open_profile_store(&path).expect("storage");
        store.save_mail_account(&account).expect("save account");
        store
            .save_mailboxes(&[Mailbox {
                id: remote_mailbox_id(&account, "INBOX").expect("mailbox id"),
                account_id: account.id.clone(),
                display_name: "INBOX".into(),
                role: MailboxRole::Inbox,
                unread_count: 0,
                total_count: 0,
            }])
            .expect("save mailbox");
        let mut warnings = Vec::new();
        cache_remote_message(
            &database_path,
            &account,
            remote,
            message_id.clone(),
            1_785_830_400_000,
            &mut store,
            &mut warnings,
        )
        .expect("cache remote message");
        drop(store);

        assert!(warnings.is_empty());
        let restarted = open_workspace(database_path.clone()).expect("reload workspace");
        let message = restarted
            .messages
            .iter()
            .find(|message| message.id == message_id.as_str())
            .expect("cached message");
        assert_eq!(message.attachments.len(), 1);
        assert_eq!(message.attachments[0].file_name, "report.pdf");
        assert_eq!(message.attachments[0].content_type, "application/pdf");
        let export_path = path.with_extension("incoming-export.pdf");
        export_local_attachment(
            database_path,
            message.attachments[0].id.clone(),
            export_path.to_string_lossy().into_owned(),
        )
        .expect("export attachment");
        assert_eq!(
            fs::read(&export_path).expect("read export"),
            b"incoming bytes"
        );

        for suffix in ["", "-shm", "-wal"] {
            let _ = fs::remove_file(format!("{}{suffix}", path.display()));
        }
        let _ = fs::remove_file(export_path);
        let _ = fs::remove_dir_all(path.with_extension("objects"));
    }

    #[test]
    fn exposes_selectively_synchronized_attachment_as_server_download() {
        let path = temporary_database_path();
        let database_path = path.to_string_lossy().into_owned();
        open_workspace(database_path.clone()).expect("seed workspace");
        let account = mail_account_from_input(MailAccountInput {
            id: "server-attachment-test".into(),
            display_name: "Server Attachment Test".into(),
            email: "user@example.org".into(),
            imap_host: "imap.example.org".into(),
            imap_port: 993,
            imap_security: "tls".into(),
            imap_username: "user@example.org".into(),
            smtp_host: "smtp.example.org".into(),
            smtp_port: 587,
            smtp_security: "starttls".into(),
            smtp_username: "user@example.org".into(),
        })
        .expect("account");
        let remote = RemoteMessage {
            remote_mailbox: "INBOX".into(),
            mailbox_role: MailboxRole::Inbox,
            uid_validity: 42,
            uid: 10,
            flags: Vec::new(),
            renderable_message: b"From: Anna <anna@example.org>\r\nSubject: Selective attachment\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nBody only\r\n".to_vec(),
            attachments: vec![RemoteAttachmentPart {
                section: "2".into(),
                file_name: "../../server.pdf".into(),
                content_type: "application/pdf".into(),
                decoded_size_hint: 321,
                transfer_encoding: "base64".into(),
            }],
            catalog_complete: true,
            body_requested: true,
            body_complete: true,
        };
        let message_id = remote_message_id(&account, &remote).expect("message id");
        let mut store = open_profile_store(&path).expect("storage");
        store.save_mail_account(&account).expect("save account");
        store
            .save_mailboxes(&[Mailbox {
                id: remote_mailbox_id(&account, "INBOX").expect("mailbox id"),
                account_id: account.id.clone(),
                display_name: "INBOX".into(),
                role: MailboxRole::Inbox,
                unread_count: 0,
                total_count: 0,
            }])
            .expect("save mailbox");
        let mut warnings = Vec::new();
        cache_remote_message(
            &database_path,
            &account,
            remote,
            message_id.clone(),
            1_785_830_400_000,
            &mut store,
            &mut warnings,
        )
        .expect("cache remote metadata");
        drop(store);

        assert!(warnings.is_empty());
        let snapshot = open_workspace(database_path).expect("reload workspace");
        let attachment = &snapshot
            .messages
            .iter()
            .find(|message| message.id == message_id.as_str())
            .expect("cached message")
            .attachments[0];
        assert_eq!(attachment.file_name, "server.pdf");
        assert_eq!(attachment.size_bytes, 321);
        assert!(!attachment.available_locally);

        for suffix in ["", "-shm", "-wal"] {
            let _ = fs::remove_file(format!("{}{suffix}", path.display()));
        }
        let _ = fs::remove_dir_all(path.with_extension("objects"));
    }

    #[test]
    fn stores_oauth_tokens_without_exposing_them_in_snapshots() {
        let path = temporary_database_path();
        let database_path = path.to_string_lossy().into_owned();
        open_workspace(database_path.clone()).expect("workspace");
        save_oauth_mail_account(
            database_path.clone(),
            MailAccountInput {
                id: "oauth-account".into(),
                display_name: "Exchange Online".into(),
                email: "alex@example.org".into(),
                imap_host: "outlook.office365.com".into(),
                imap_port: 993,
                imap_security: "tls".into(),
                imap_username: "alex@example.org".into(),
                smtp_host: "smtp.office365.com".into(),
                smtp_port: 587,
                smtp_security: "starttls".into(),
                smtp_username: "alex@example.org".into(),
            },
            OAuthTokenInput {
                provider: "microsoft365".into(),
                client_id: "public-client-id".into(),
                access_token: "access-token-marker".into(),
                refresh_token: "refresh-token-marker".into(),
                expires_at_ms: 1_893_456_000_000,
                token_endpoint: MICROSOFT_TOKEN_ENDPOINT.into(),
                scopes: "offline_access https://outlook.office.com/SMTP.Send".into(),
            },
        )
        .expect("save OAuth account");

        let snapshot = open_workspace(database_path).expect("snapshot");
        let account = snapshot
            .mail_accounts
            .iter()
            .find(|account| account.id == "oauth-account")
            .expect("OAuth account");
        assert_eq!(account.authentication, "oauth2");
        assert_eq!(account.oauth_provider.as_deref(), Some("microsoft365"));
        let store = open_profile_store(&path).expect("profile store");
        let account_id = AccountId::parse("oauth-account").expect("account id");
        assert_eq!(
            store
                .get(&account_id, OAUTH_REFRESH_TOKEN_KEY)
                .expect("refresh token"),
            Some("refresh-token-marker".into())
        );
        assert_eq!(
            store
                .get(&account_id, ACCOUNT_PASSWORD_KEY)
                .expect("password"),
            None
        );

        for suffix in ["", "-shm", "-wal"] {
            let _ = fs::remove_file(format!("{}{suffix}", path.display()));
        }
        let _ = fs::remove_dir_all(path.with_extension("objects"));
    }

    #[test]
    fn exports_and_restores_a_complete_profile_with_vault_credentials() {
        let source = temporary_database_path();
        let target = temporary_database_path();
        let archive = source.with_extension("maicenta-profile");
        let source_path = source.to_string_lossy().into_owned();
        let target_path = target.to_string_lossy().into_owned();
        open_workspace(source_path.clone()).expect("source workspace");
        save_mail_account(
            source_path.clone(),
            MailAccountInput {
                id: "portable-account".into(),
                display_name: "Portable Account".into(),
                email: "portable@example.org".into(),
                imap_host: "imap.example.org".into(),
                imap_port: 993,
                imap_security: "tls".into(),
                imap_username: "portable@example.org".into(),
                smtp_host: "smtp.example.org".into(),
                smtp_port: 587,
                smtp_security: "starttls".into(),
                smtp_username: "portable@example.org".into(),
            },
            "vault-secret-marker".into(),
        )
        .expect("save portable account");
        export_profile(
            source_path,
            archive.to_string_lossy().into_owned(),
            "correct horse battery staple".into(),
        )
        .expect("export profile");
        assert!(
            !fs::read(&archive)
                .expect("archive")
                .windows("vault-secret-marker".len())
                .any(|window| window == b"vault-secret-marker")
        );

        open_workspace(target_path.clone()).expect("target workspace");
        let restored = import_profile(
            target_path,
            archive.to_string_lossy().into_owned(),
            "correct horse battery staple".into(),
        )
        .expect("import profile");
        assert!(
            restored
                .mail_accounts
                .iter()
                .any(|account| account.id == "portable-account")
        );
        let store = open_profile_store(&target).expect("restored storage");
        let account = mail_account_from_input(MailAccountInput {
            id: "portable-account".into(),
            display_name: "Portable Account".into(),
            email: "portable@example.org".into(),
            imap_host: "imap.example.org".into(),
            imap_port: 993,
            imap_security: "tls".into(),
            imap_username: "portable@example.org".into(),
            smtp_host: "smtp.example.org".into(),
            smtp_port: 587,
            smtp_security: "starttls".into(),
            smtp_username: "portable@example.org".into(),
        })
        .expect("restored account id");
        assert_eq!(
            store.get(&account.id, "password").expect("restored secret"),
            Some("vault-secret-marker".into())
        );

        for path in [&source, &target] {
            for suffix in ["", "-shm", "-wal"] {
                let _ = fs::remove_file(format!("{}{suffix}", path.display()));
            }
            let _ = fs::remove_dir_all(path.with_extension("objects"));
        }
        let _ = fs::remove_file(archive);
    }

    fn temporary_database_path() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let serial = NEXT_DATABASE_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "maicenta-bridge-{}-{unique}-{serial}.sqlite",
            std::process::id()
        ))
    }
}
