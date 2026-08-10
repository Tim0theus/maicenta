//! Real IMAP/SMTP connectivity and legacy credential migration support.

use std::{
    collections::{HashMap, HashSet},
    fmt::{Debug, Write as _},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_imap::{
    Client, Session,
    imap_proto::types::{
        BodyContentCommon, BodyContentSinglePart, BodyStructure, ContentEncoding,
        Response as ImapResponse, SectionPath,
    },
    types::{NameAttribute, UnsolicitedResponse},
};
use async_native_tls::TlsConnector;
use futures::TryStreamExt;
use keyring::Entry;
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    address::{Address as LettreAddress, Envelope},
    message::{
        Attachment, Mailbox as LettreMailbox, MultiPart, SinglePart,
        header::{ContentType, HeaderName, HeaderValue},
    },
    transport::smtp::{authentication::Credentials, response::Response},
};
use maicenta_domain::{MailAccount, MailboxRole, MessageFlag, TransportSecurity};
use tokio::{io::AsyncRead, io::AsyncWrite, net::TcpStream};

const KEYRING_SERVICE: &str = "org.maicenta.desktop.mail";
const NETWORK_TIMEOUT: Duration = Duration::from_secs(30);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(120);
const SYNC_OPERATION_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MAX_CATALOG_MESSAGES_PER_MAILBOX: usize = 250;
const MAX_FLAG_REFRESH_PER_MAILBOX: usize = 500;
const MAX_SELECTIVE_ATTACHMENT_BYTES: usize = 128 * 1024 * 1024;
const MAX_SYNC_HEADER_BYTES: usize = 256 * 1024;
const MAX_SYNC_TEXT_PARTS: usize = 4;
const MAX_SYNC_TEXT_PART_BYTES: usize = 5 * 1024 * 1024;
const MAX_SYNC_TEXT_TOTAL_BYTES: usize = 10 * 1024 * 1024;
const MAX_SYNC_INLINE_PARTS: usize = 20;
const MAX_SYNC_INLINE_PART_BYTES: usize = 3 * 1024 * 1024;
const MAX_SYNC_INLINE_TOTAL_BYTES: usize = 7 * 1024 * 1024;

/// Error returned by a mail server or the legacy native credential store.
#[derive(Debug, thiserror::Error)]
pub enum ConnectorError {
    #[error("invalid mail account configuration: {0}")]
    InvalidConfiguration(String),
    #[error("mail server connection failed: {0}")]
    Connection(String),
    #[error("mail server authentication failed: {0}")]
    Authentication(String),
    #[error("mail protocol operation failed: {0}")]
    Protocol(String),
    #[error("message could not be created: {0}")]
    Message(String),
    #[error("legacy operating-system credential store failed: {0}")]
    CredentialStore(String),
}

/// One provider mailbox discovered through IMAP.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteMailbox {
    pub remote_name: String,
    pub role: MailboxRole,
}

/// One message whose displayable MIME parts were selectively downloaded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteMessage {
    pub remote_mailbox: String,
    pub mailbox_role: MailboxRole,
    pub uid_validity: u32,
    pub uid: u32,
    pub flags: Vec<MessageFlag>,
    /// Complete or selectively reconstructed MIME input for the safe renderer.
    pub renderable_message: Vec<u8>,
    pub attachments: Vec<RemoteAttachmentPart>,
    pub catalog_complete: bool,
    /// Whether this synchronization pass attempted to fetch displayable body
    /// parts instead of cataloguing headers only.
    pub body_requested: bool,
    /// False when a declared display part was omitted or not returned by the
    /// server. Attachments are intentionally excluded and do not affect this.
    pub body_complete: bool,
}

/// Metadata needed to fetch one non-inline MIME part without transferring the
/// complete message again.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteAttachmentPart {
    pub section: String,
    pub file_name: String,
    pub content_type: String,
    pub decoded_size_hint: u64,
    pub transfer_encoding: String,
}

/// Cached server identity used to avoid downloading an unchanged message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KnownRemoteMessage {
    pub local_key: String,
    pub remote_mailbox: String,
    pub uid_validity: u32,
    pub uid: u32,
    pub needs_catalog_refresh: bool,
    pub needs_body_refresh: bool,
}

/// Persisted mailbox state used to select a cheap incremental IMAP path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteMailboxCheckpoint {
    pub remote_mailbox: String,
    pub uid_validity: u32,
    pub uid_next: Option<u32>,
    pub highest_modseq: Option<u64>,
    pub catalog_complete: bool,
    pub force_full_reconcile: bool,
}

/// Server flag snapshot for a message whose body is already cached.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteFlagUpdate {
    pub local_key: String,
    pub flags: Vec<MessageFlag>,
}

/// Complete UID snapshot for one mailbox selected during this pass.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteMailboxState {
    pub remote_mailbox: String,
    pub uid_validity: u32,
    pub uid_next: Option<u32>,
    pub highest_modseq: Option<u64>,
    pub active_uids: Vec<u32>,
    /// True only when `active_uids` is a complete mailbox snapshot that may be
    /// used to remove stale local rows.
    pub full_reconcile: bool,
    /// UIDs confirmed as deleted by a successful QRESYNC VANISHED response.
    pub vanished_uids: Vec<u32>,
    pub qresync_used: bool,
    /// Messages whose compact headers still need to be added to the local
    /// search catalogue during a later synchronization pass.
    pub catalog_remaining: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RenderablePartRole {
    Text,
    InlineImage,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RenderablePart {
    section: String,
    path: Vec<u32>,
    mime_headers: Vec<u8>,
    role: RenderablePartRole,
    maximum_bytes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ValidatedSectionPath {
    Text,
    Part(Vec<u32>),
}

#[derive(Debug)]
struct PendingRemoteMessage {
    uid: u32,
    flags: Vec<MessageFlag>,
    header: Vec<u8>,
    attachments: Vec<RemoteAttachmentPart>,
    renderable_parts: Vec<RenderablePart>,
    body_complete: bool,
}

/// Result of one bounded incoming-mail synchronization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImapSyncResult {
    pub mailboxes: Vec<RemoteMailbox>,
    pub messages: Vec<RemoteMessage>,
    pub flag_updates: Vec<RemoteFlagUpdate>,
    pub mailbox_states: Vec<RemoteMailboxState>,
}

#[derive(Debug)]
struct MailboxFetchResult {
    messages: Vec<RemoteMessage>,
    flag_updates: Vec<RemoteFlagUpdate>,
    state: RemoteMailboxState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SyncCapabilities {
    condstore: bool,
    qresync: bool,
}

/// One compacted local change that should be applied to an IMAP message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteMutation {
    pub local_key: String,
    pub source_mailbox: String,
    pub target_mailbox: Option<String>,
    pub uid_validity: u32,
    pub remote_uid: u32,
    pub seen: bool,
    pub flagged: bool,
}

/// One successfully applied mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppliedMutation {
    pub local_key: String,
    pub moved: bool,
}

/// One mutation retained for a later retry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailedMutation {
    pub local_key: String,
    pub error: String,
}

/// Per-message outcome of applying a persistent mutation queue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MutationReport {
    pub applied: Vec<AppliedMutation>,
    pub failed: Vec<FailedMutation>,
}

/// Stable IMAP identity of one uploaded draft.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteDraftIdentity {
    pub remote_mailbox: String,
    pub uid_validity: u32,
    pub remote_uid: u32,
}

/// One durable local draft action to apply through IMAP.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteDraftOperation {
    pub local_key: String,
    pub target_mailbox: String,
    pub previous_remote: Option<RemoteDraftIdentity>,
    /// `None` removes the previous server draft without uploading a successor.
    pub message: Option<OutgoingMessage>,
}

/// One successfully applied draft action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppliedDraftOperation {
    pub local_key: String,
    pub uploaded_remote: Option<RemoteDraftIdentity>,
}

/// Per-draft outcome of applying the persistent draft queue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DraftOperationReport {
    pub applied: Vec<AppliedDraftOperation>,
    pub failed: Vec<FailedMutation>,
}

/// One validated attachment ready for MIME encoding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutgoingAttachment {
    pub file_name: String,
    pub content_type: String,
    pub body: Vec<u8>,
}

/// Complete provider-independent message passed to SMTP submission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutgoingMessage {
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub plain_text: String,
    pub sanitized_html: String,
    pub attachments: Vec<OutgoingAttachment>,
    pub high_importance: bool,
}

/// Loads an account password from the legacy per-account keychain layout.
///
/// New and migrated credentials live inside the encrypted profile vault. This
/// adapter remains temporarily so existing alpha profiles can be upgraded.
///
/// # Errors
///
/// Returns [`ConnectorError::CredentialStore`] when no credential is present
/// or the native store cannot be accessed.
pub fn load_legacy_password(account: &MailAccount) -> Result<String, ConnectorError> {
    password_entry(account)?
        .get_password()
        .map_err(|error| ConnectorError::CredentialStore(error.to_string()))
}

/// Removes an account password from the legacy per-account keychain layout.
///
/// A missing credential is treated as already deleted.
///
/// # Errors
///
/// Returns [`ConnectorError::CredentialStore`] when the native store cannot
/// be accessed or rejects the deletion.
pub fn delete_legacy_password(account: &MailAccount) -> Result<(), ConnectorError> {
    match password_entry(account)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(ConnectorError::CredentialStore(error.to_string())),
    }
}

/// Verifies both IMAP authentication and the SMTP connection without sending.
///
/// # Errors
///
/// Returns a categorized connection, authentication, or protocol error.
pub async fn test_account(account: &MailAccount, password: &str) -> Result<(), ConnectorError> {
    tokio::time::timeout(OPERATION_TIMEOUT, test_account_inner(account, password))
        .await
        .map_err(|_| ConnectorError::Connection("account test timed out".into()))?
}

async fn test_account_inner(account: &MailAccount, password: &str) -> Result<(), ConnectorError> {
    validate_account(account)?;
    match account.imap_security {
        TransportSecurity::Tls => {
            let mut session = login_imap_tls(account, password).await?;
            session
                .logout()
                .await
                .map_err(|error| ConnectorError::Protocol(error.to_string()))?;
        }
        TransportSecurity::StartTls => {
            let mut session = login_imap_starttls(account, password).await?;
            session
                .logout()
                .await
                .map_err(|error| ConnectorError::Protocol(error.to_string()))?;
        }
    }

    let smtp = smtp_transport(account, password)?;
    let connected = smtp
        .test_connection()
        .await
        .map_err(|error| ConnectorError::Connection(error.to_string()))?;
    if connected {
        Ok(())
    } else {
        Err(ConnectorError::Connection(
            "SMTP server rejected the connection test".into(),
        ))
    }
}

/// Lists subscribed provider folders, progressively catalogues compact
/// metadata from every selectable mailbox, and downloads recent message
/// bodies while prioritizing the standard folders.
///
/// `message_limit_per_mailbox` is bounded to 50 to prevent an accidental body
/// download of an entire account. Independently, up to 250 unknown headers and
/// attachment metadata records per mailbox are catalogued during each pass;
/// the caller can immediately continue while `catalog_remaining` is non-zero.
///
/// # Errors
///
/// Returns a categorized connection, authentication, or protocol error.
pub async fn synchronize_mailboxes(
    account: &MailAccount,
    password: &str,
    known_messages: &[KnownRemoteMessage],
    mailbox_checkpoints: &[RemoteMailboxCheckpoint],
    message_limit_per_mailbox: usize,
) -> Result<ImapSyncResult, ConnectorError> {
    tokio::time::timeout(
        SYNC_OPERATION_TIMEOUT,
        synchronize_mailboxes_inner(
            account,
            password,
            known_messages,
            mailbox_checkpoints,
            message_limit_per_mailbox,
        ),
    )
    .await
    .map_err(|_| ConnectorError::Connection("IMAP synchronization timed out".into()))?
}

/// Downloads one validated MIME section through `BODY.PEEK` without changing
/// the message's read state.
///
/// The caller must verify and decode the recorded transfer encoding. The
/// connector verifies mailbox UIDVALIDITY and the exact UID before returning
/// no more than 128 MiB of encoded section data.
///
/// # Errors
///
/// Returns a categorized validation, authentication, connection, or protocol
/// error when the recorded remote identity is stale or the server response is
/// incomplete.
pub async fn download_attachment_part(
    account: &MailAccount,
    password: &str,
    remote_mailbox: &str,
    uid_validity: u32,
    uid: u32,
    section: &str,
) -> Result<Vec<u8>, ConnectorError> {
    let section_path = validated_section_path(section)?;
    tokio::time::timeout(
        OPERATION_TIMEOUT,
        download_attachment_part_inner(
            account,
            password,
            remote_mailbox,
            uid_validity,
            uid,
            section,
            section_path,
        ),
    )
    .await
    .map_err(|_| ConnectorError::Connection("attachment download timed out".into()))?
}

/// Downloads the displayable parts of one previously catalogued message.
///
/// The exact UID and UIDVALIDITY are verified before bounded `BODY.PEEK`
/// requests are issued, so opening an old header-only search result does not
/// require downloading neighbouring messages or attachment bodies.
///
/// # Errors
///
/// Returns a categorized validation, authentication, connection, or protocol
/// error when the remote identity is stale or the message cannot be fetched.
pub async fn download_message_content(
    account: &MailAccount,
    password: &str,
    remote_mailbox: &str,
    uid_validity: u32,
    uid: u32,
) -> Result<RemoteMessage, ConnectorError> {
    validate_account(account)?;
    if remote_mailbox.is_empty() || uid == 0 {
        return Err(ConnectorError::InvalidConfiguration(
            "remote message identity is incomplete".into(),
        ));
    }
    tokio::time::timeout(OPERATION_TIMEOUT, async {
        match account.imap_security {
            TransportSecurity::Tls => {
                let mut session = login_imap_tls(account, password).await?;
                let result = download_message_content_in_session(
                    &mut session,
                    remote_mailbox,
                    uid_validity,
                    uid,
                )
                .await;
                let _ = session.logout().await;
                result
            }
            TransportSecurity::StartTls => {
                let mut session = login_imap_starttls(account, password).await?;
                let result = download_message_content_in_session(
                    &mut session,
                    remote_mailbox,
                    uid_validity,
                    uid,
                )
                .await;
                let _ = session.logout().await;
                result
            }
        }
    })
    .await
    .map_err(|_| ConnectorError::Connection("message download timed out".into()))?
}

async fn download_message_content_in_session<T>(
    session: &mut Session<T>,
    remote_mailbox: &str,
    uid_validity: u32,
    uid: u32,
) -> Result<RemoteMessage, ConnectorError>
where
    T: AsyncRead + AsyncWrite + Unpin + Debug + Send,
{
    let selected = session
        .select(remote_mailbox)
        .await
        .map_err(|error| ConnectorError::Protocol(error.to_string()))?;
    if selected.uid_validity.unwrap_or_default() != uid_validity {
        return Err(ConnectorError::Protocol(
            "UIDVALIDITY changed; the catalogued message is stale".into(),
        ));
    }
    let mut pending = fetch_pending_message_metadata(session, &[uid]).await?;
    let pending = pending
        .pop()
        .filter(|message| message.uid == uid)
        .ok_or_else(|| ConnectorError::Protocol("message UID no longer exists".into()))?;
    let mailbox = RemoteMailbox {
        remote_name: remote_mailbox.to_owned(),
        role: MailboxRole::Custom,
    };
    Ok(fetch_selective_message_parts(session, &mailbox, uid_validity, pending).await)
}

async fn download_attachment_part_inner(
    account: &MailAccount,
    password: &str,
    remote_mailbox: &str,
    uid_validity: u32,
    uid: u32,
    section: &str,
    section_path: ValidatedSectionPath,
) -> Result<Vec<u8>, ConnectorError> {
    validate_account(account)?;
    match account.imap_security {
        TransportSecurity::Tls => {
            let mut session = login_imap_tls(account, password).await?;
            let result = download_attachment_in_session(
                &mut session,
                remote_mailbox,
                uid_validity,
                uid,
                section,
                section_path,
            )
            .await;
            let _ = session.logout().await;
            result
        }
        TransportSecurity::StartTls => {
            let mut session = login_imap_starttls(account, password).await?;
            let result = download_attachment_in_session(
                &mut session,
                remote_mailbox,
                uid_validity,
                uid,
                section,
                section_path,
            )
            .await;
            let _ = session.logout().await;
            result
        }
    }
}

async fn download_attachment_in_session<T>(
    session: &mut Session<T>,
    remote_mailbox: &str,
    uid_validity: u32,
    uid: u32,
    section: &str,
    section_path: ValidatedSectionPath,
) -> Result<Vec<u8>, ConnectorError>
where
    T: AsyncRead + AsyncWrite + Unpin + Debug + Send,
{
    let selected = session
        .select(remote_mailbox)
        .await
        .map_err(|error| ConnectorError::Protocol(error.to_string()))?;
    if selected.uid_validity.unwrap_or_default() != uid_validity {
        return Err(ConnectorError::Protocol(
            "mailbox UIDVALIDITY changed; synchronize before downloading the attachment".into(),
        ));
    }
    let stream = session
        .uid_fetch(
            uid.to_string(),
            format!(
                "(UID BODY.PEEK[{section}]<0.{}>)",
                MAX_SELECTIVE_ATTACHMENT_BYTES + 1
            ),
        )
        .await
        .map_err(|error| ConnectorError::Protocol(error.to_string()))?;
    let fetched = stream
        .try_collect::<Vec<_>>()
        .await
        .map_err(|error| ConnectorError::Protocol(error.to_string()))?;
    let bytes = fetched
        .iter()
        .find(|message| message.uid == Some(uid))
        .and_then(|message| match &section_path {
            ValidatedSectionPath::Text => message.text(),
            ValidatedSectionPath::Part(path) => {
                message.section(&SectionPath::Part(path.clone(), None))
            }
        })
        .ok_or_else(|| ConnectorError::Protocol("attachment section was not returned".into()))?;
    if bytes.len() > MAX_SELECTIVE_ATTACHMENT_BYTES {
        return Err(ConnectorError::Protocol(
            "attachment section exceeds the 128 MiB download limit".into(),
        ));
    }
    Ok(bytes.to_vec())
}

fn validated_section_path(section: &str) -> Result<ValidatedSectionPath, ConnectorError> {
    if section == "TEXT" {
        return Ok(ValidatedSectionPath::Text);
    }
    if section.is_empty() || section.len() > 128 {
        return Err(ConnectorError::InvalidConfiguration(
            "attachment section is invalid".into(),
        ));
    }
    section
        .split('.')
        .map(|component| {
            component
                .parse::<u32>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or_else(|| {
                    ConnectorError::InvalidConfiguration("attachment section is invalid".into())
                })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(ValidatedSectionPath::Part)
}

/// Applies queued flags and moves without ever issuing an unsafe global
/// `EXPUNGE`. Failed items are reported individually and can be retried.
///
/// # Errors
///
/// Returns a connection or authentication error when an IMAP session cannot
/// be established. Protocol failures for individual messages are returned in
/// [`MutationReport::failed`].
pub async fn apply_mailbox_mutations(
    account: &MailAccount,
    password: &str,
    mutations: &[RemoteMutation],
) -> Result<MutationReport, ConnectorError> {
    tokio::time::timeout(
        OPERATION_TIMEOUT,
        apply_mailbox_mutations_inner(account, password, mutations),
    )
    .await
    .map_err(|_| ConnectorError::Connection("IMAP mutation synchronization timed out".into()))?
}

/// Uploads, replaces, or removes locally editable drafts through IMAP. A
/// stable Message-ID makes retries idempotent if APPEND succeeded but the
/// connection ended before the local UID could be persisted.
///
/// Existing drafts are removed only with UID EXPUNGE. This deliberately
/// refuses a global EXPUNGE on servers without UIDPLUS.
///
/// # Errors
///
/// Returns a connection or authentication error when an IMAP session cannot
/// be established. Per-draft protocol failures remain in the returned report
/// for a later retry.
pub async fn apply_draft_operations(
    account: &MailAccount,
    password: &str,
    operations: &[RemoteDraftOperation],
) -> Result<DraftOperationReport, ConnectorError> {
    tokio::time::timeout(
        OPERATION_TIMEOUT,
        apply_draft_operations_inner(account, password, operations),
    )
    .await
    .map_err(|_| ConnectorError::Connection("IMAP draft synchronization timed out".into()))?
}

async fn apply_draft_operations_inner(
    account: &MailAccount,
    password: &str,
    operations: &[RemoteDraftOperation],
) -> Result<DraftOperationReport, ConnectorError> {
    validate_account(account)?;
    match account.imap_security {
        TransportSecurity::Tls => {
            let mut session = login_imap_tls(account, password).await?;
            let result = apply_drafts_in_session(&mut session, account, operations).await;
            let _ = session.logout().await;
            result
        }
        TransportSecurity::StartTls => {
            let mut session = login_imap_starttls(account, password).await?;
            let result = apply_drafts_in_session(&mut session, account, operations).await;
            let _ = session.logout().await;
            result
        }
    }
}

async fn apply_mailbox_mutations_inner(
    account: &MailAccount,
    password: &str,
    mutations: &[RemoteMutation],
) -> Result<MutationReport, ConnectorError> {
    validate_account(account)?;
    match account.imap_security {
        TransportSecurity::Tls => {
            let mut session = login_imap_tls(account, password).await?;
            let result = apply_mutations_in_session(&mut session, mutations).await;
            let _ = session.logout().await;
            result
        }
        TransportSecurity::StartTls => {
            let mut session = login_imap_starttls(account, password).await?;
            let result = apply_mutations_in_session(&mut session, mutations).await;
            let _ = session.logout().await;
            result
        }
    }
}

async fn synchronize_mailboxes_inner(
    account: &MailAccount,
    password: &str,
    known_messages: &[KnownRemoteMessage],
    mailbox_checkpoints: &[RemoteMailboxCheckpoint],
    message_limit_per_mailbox: usize,
) -> Result<ImapSyncResult, ConnectorError> {
    validate_account(account)?;
    let message_limit_per_mailbox = message_limit_per_mailbox.clamp(1, 50);
    match account.imap_security {
        TransportSecurity::Tls => {
            let mut session = login_imap_tls(account, password).await?;
            let result = synchronize_session(
                &mut session,
                known_messages,
                mailbox_checkpoints,
                message_limit_per_mailbox,
            )
            .await;
            let _ = session.logout().await;
            result
        }
        TransportSecurity::StartTls => {
            let mut session = login_imap_starttls(account, password).await?;
            let result = synchronize_session(
                &mut session,
                known_messages,
                mailbox_checkpoints,
                message_limit_per_mailbox,
            )
            .await;
            let _ = session.logout().await;
            result
        }
    }
}

/// Sends one standards-oriented HTML message with a plain-text alternative and
/// optional attachments through the configured SMTP submission server.
///
/// # Errors
///
/// Returns an error when addresses are invalid or the SMTP server rejects the
/// message.
pub async fn send_message(
    account: &MailAccount,
    password: &str,
    outgoing: &OutgoingMessage,
) -> Result<Response, ConnectorError> {
    validate_account(account)?;
    let message = build_message(account, outgoing)?;

    tokio::time::timeout(
        OPERATION_TIMEOUT,
        smtp_transport(account, password)?.send(message),
    )
    .await
    .map_err(|_| ConnectorError::Connection("SMTP delivery timed out".into()))?
    .map_err(|error| ConnectorError::Protocol(error.to_string()))
}

fn build_message(
    account: &MailAccount,
    outgoing: &OutgoingMessage,
) -> Result<Message, ConnectorError> {
    build_content_message(account, outgoing, None, true)
}

fn build_draft_message(
    account: &MailAccount,
    outgoing: &OutgoingMessage,
    message_id: &str,
) -> Result<Message, ConnectorError> {
    build_content_message(account, outgoing, Some(message_id), false)
}

fn build_content_message(
    account: &MailAccount,
    outgoing: &OutgoingMessage,
    message_id: Option<&str>,
    require_to_recipient: bool,
) -> Result<Message, ConnectorError> {
    if require_to_recipient && outgoing.to.is_empty() {
        return Err(ConnectorError::InvalidConfiguration(
            "at least one To recipient is required".into(),
        ));
    }
    let from_address: LettreAddress =
        account
            .email
            .address()
            .parse()
            .map_err(|error: lettre::address::AddressError| {
                ConnectorError::Message(error.to_string())
            })?;
    let envelope_address = from_address.clone();
    let from = LettreMailbox::new(Some(account.display_name.clone()), from_address);
    let mut builder = Message::builder().from(from).subject(&outgoing.subject);
    if let Some(message_id) = message_id {
        builder = builder.message_id(Some(message_id.to_owned())).keep_bcc();
    }
    for recipient in parse_mailboxes(&outgoing.to)? {
        builder = builder.to(recipient);
    }
    for recipient in parse_mailboxes(&outgoing.cc)? {
        builder = builder.cc(recipient);
    }
    for recipient in parse_mailboxes(&outgoing.bcc)? {
        builder = builder.bcc(recipient);
    }
    if !require_to_recipient
        && outgoing.to.is_empty()
        && outgoing.cc.is_empty()
        && outgoing.bcc.is_empty()
    {
        // Lettre requires a non-empty transport envelope even though RFC 5322
        // permits a draft without destination headers. IMAP APPEND serializes
        // only the message, so use a harmless internal self-envelope while
        // keeping To/Cc/Bcc absent from the stored draft.
        let envelope = Envelope::new(Some(envelope_address.clone()), vec![envelope_address])
            .map_err(|error| ConnectorError::Message(error.to_string()))?;
        builder = builder.envelope(envelope);
    }
    let alternative = MultiPart::alternative()
        .singlepart(SinglePart::plain(outgoing.plain_text.clone()))
        .singlepart(SinglePart::html(outgoing.sanitized_html.clone()));
    let content = if outgoing.attachments.is_empty() {
        alternative
    } else {
        let mut mixed = MultiPart::mixed().multipart(alternative);
        for attachment in &outgoing.attachments {
            let content_type = ContentType::parse(&attachment.content_type)
                .map_err(|error| ConnectorError::Message(error.to_string()))?;
            mixed = mixed.singlepart(
                Attachment::new(attachment.file_name.clone())
                    .body(attachment.body.clone(), content_type),
            );
        }
        mixed
    };
    let mut message = builder
        .multipart(content)
        .map_err(|error| ConnectorError::Message(error.to_string()))?;
    if outgoing.high_importance {
        message.headers_mut().insert_raw(HeaderValue::new(
            HeaderName::new_from_ascii_str("Importance"),
            "high".into(),
        ));
        message.headers_mut().insert_raw(HeaderValue::new(
            HeaderName::new_from_ascii_str("X-Priority"),
            "1".into(),
        ));
    }

    Ok(message)
}

fn parse_mailboxes(recipients: &[String]) -> Result<Vec<LettreMailbox>, ConnectorError> {
    recipients
        .iter()
        .map(|recipient| {
            recipient
                .parse::<LettreMailbox>()
                .map_err(|error| ConnectorError::Message(error.to_string()))
        })
        .collect()
}

/// Produces a stable provider-independent suffix for a remote identifier.
#[must_use]
pub fn stable_remote_key(value: &str) -> String {
    let hash = value
        .as_bytes()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
        });
    format!("{hash:016x}")
}

fn password_entry(account: &MailAccount) -> Result<Entry, ConnectorError> {
    Entry::new(KEYRING_SERVICE, account.id.as_str())
        .map_err(|error| ConnectorError::CredentialStore(error.to_string()))
}

fn validate_account(account: &MailAccount) -> Result<(), ConnectorError> {
    if account.imap_host.trim().is_empty()
        || account.smtp_host.trim().is_empty()
        || account.imap_username.trim().is_empty()
        || account.smtp_username.trim().is_empty()
    {
        Err(ConnectorError::InvalidConfiguration(
            "server names and usernames must not be empty".into(),
        ))
    } else {
        Ok(())
    }
}

async fn login_imap_tls(
    account: &MailAccount,
    password: &str,
) -> Result<Session<async_native_tls::TlsStream<TcpStream>>, ConnectorError> {
    let stream = connect_tcp(&account.imap_host, account.imap_port).await?;
    let tls = TlsConnector::new()
        .connect(&account.imap_host, stream)
        .await
        .map_err(|error| ConnectorError::Connection(error.to_string()))?;
    let mut client = Client::new(tls);
    client
        .read_response()
        .await
        .map_err(|error| ConnectorError::Connection(error.to_string()))?
        .ok_or_else(|| ConnectorError::Connection("IMAP greeting was missing".into()))?;
    client
        .login(&account.imap_username, password)
        .await
        .map_err(|(error, _)| ConnectorError::Authentication(error.to_string()))
}

async fn login_imap_starttls(
    account: &MailAccount,
    password: &str,
) -> Result<Session<async_native_tls::TlsStream<TcpStream>>, ConnectorError> {
    let stream = connect_tcp(&account.imap_host, account.imap_port).await?;
    let mut client = Client::new(stream);
    client
        .read_response()
        .await
        .map_err(|error| ConnectorError::Connection(error.to_string()))?
        .ok_or_else(|| ConnectorError::Connection("IMAP greeting was missing".into()))?;
    client
        .run_command_and_check_ok("STARTTLS", None)
        .await
        .map_err(|error| ConnectorError::Protocol(error.to_string()))?;
    let stream = client.into_inner();
    let tls = TlsConnector::new()
        .connect(&account.imap_host, stream)
        .await
        .map_err(|error| ConnectorError::Connection(error.to_string()))?;
    Client::new(tls)
        .login(&account.imap_username, password)
        .await
        .map_err(|(error, _)| ConnectorError::Authentication(error.to_string()))
}

async fn connect_tcp(host: &str, port: u16) -> Result<TcpStream, ConnectorError> {
    tokio::time::timeout(NETWORK_TIMEOUT, TcpStream::connect((host, port)))
        .await
        .map_err(|_| ConnectorError::Connection("connection timed out".into()))?
        .map_err(|error| ConnectorError::Connection(error.to_string()))
}

async fn synchronize_session<T>(
    session: &mut Session<T>,
    known_messages: &[KnownRemoteMessage],
    mailbox_checkpoints: &[RemoteMailboxCheckpoint],
    message_limit_per_mailbox: usize,
) -> Result<ImapSyncResult, ConnectorError>
where
    T: AsyncRead + AsyncWrite + Unpin + Debug + Send,
{
    let advertised = session.capabilities().await.ok();
    let condstore = advertised
        .as_ref()
        .is_some_and(|capabilities| capabilities.has_str("CONDSTORE"));
    let qresync_advertised = advertised
        .as_ref()
        .is_some_and(|capabilities| capabilities.has_str("QRESYNC"));
    let qresync = qresync_advertised
        && session
            .run_command_and_check_ok("ENABLE QRESYNC")
            .await
            .is_ok();
    let capabilities = SyncCapabilities {
        condstore: condstore || qresync,
        qresync,
    };
    let mailbox_stream = session
        .list(None, Some("*"))
        .await
        .map_err(|error| ConnectorError::Protocol(error.to_string()))?;
    let names = mailbox_stream
        .try_collect::<Vec<_>>()
        .await
        .map_err(|error| ConnectorError::Protocol(error.to_string()))?;
    let subscribed_names = match session.lsub(None, Some("*")).await {
        Ok(stream) => match stream.try_collect::<Vec<_>>().await {
            Ok(names) => names
                .into_iter()
                .filter(|name| {
                    !name
                        .attributes()
                        .iter()
                        .any(|attribute| matches!(attribute, NameAttribute::NoSelect))
                })
                .map(|name| name.name().to_owned())
                .collect::<HashSet<_>>(),
            Err(_) => HashSet::new(),
        },
        Err(_) => HashSet::new(),
    };
    let filter_subscriptions = !subscribed_names.is_empty();
    let mailboxes = names
        .iter()
        .filter(|name| {
            !name
                .attributes()
                .iter()
                .any(|attribute| matches!(attribute, NameAttribute::NoSelect))
                && (!filter_subscriptions
                    || name.name().eq_ignore_ascii_case("INBOX")
                    || subscribed_names.contains(name.name()))
        })
        .map(|name| RemoteMailbox {
            remote_name: name.name().to_owned(),
            role: mailbox_role(name.name(), name.attributes()),
        })
        .collect::<Vec<_>>();
    let selected_mailboxes = ordered_mailboxes_for_sync(mailboxes.clone());

    let mut messages = Vec::new();
    let mut flag_updates = Vec::new();
    let mut mailbox_states = Vec::new();
    for mailbox in selected_mailboxes {
        let checkpoint = mailbox_checkpoints
            .iter()
            .find(|checkpoint| checkpoint.remote_mailbox == mailbox.remote_name);
        let fetched = fetch_mailbox_messages(
            session,
            &mailbox,
            known_messages,
            checkpoint,
            message_limit_per_mailbox,
            capabilities,
        )
        .await?;
        messages.extend(fetched.messages);
        flag_updates.extend(fetched.flag_updates);
        mailbox_states.push(fetched.state);
    }

    Ok(ImapSyncResult {
        mailboxes,
        messages,
        flag_updates,
        mailbox_states,
    })
}

fn ordered_mailboxes_for_sync(mut mailboxes: Vec<RemoteMailbox>) -> Vec<RemoteMailbox> {
    mailboxes.sort_by(|left, right| {
        mailbox_sync_priority(left.role)
            .cmp(&mailbox_sync_priority(right.role))
            .then_with(|| left.remote_name.cmp(&right.remote_name))
    });
    mailboxes
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MoveStrategy {
    Move,
    CopyUidExpunge,
    Unavailable,
}

const fn safe_move_strategy(has_move: bool, has_uid_plus: bool) -> MoveStrategy {
    if has_move {
        MoveStrategy::Move
    } else if has_uid_plus {
        MoveStrategy::CopyUidExpunge
    } else {
        MoveStrategy::Unavailable
    }
}

async fn apply_drafts_in_session<T>(
    session: &mut Session<T>,
    account: &MailAccount,
    operations: &[RemoteDraftOperation],
) -> Result<DraftOperationReport, ConnectorError>
where
    T: AsyncRead + AsyncWrite + Unpin + Debug + Send,
{
    let capabilities = session
        .capabilities()
        .await
        .map_err(|error| ConnectorError::Protocol(error.to_string()))?;
    let has_uid_plus = capabilities.has_str("UIDPLUS");
    let mut report = DraftOperationReport {
        applied: Vec::new(),
        failed: Vec::new(),
    };
    for operation in operations {
        match apply_one_draft_operation(session, account, operation, has_uid_plus).await {
            Ok(uploaded_remote) => report.applied.push(AppliedDraftOperation {
                local_key: operation.local_key.clone(),
                uploaded_remote,
            }),
            Err(error) => report.failed.push(FailedMutation {
                local_key: operation.local_key.clone(),
                error,
            }),
        }
    }
    Ok(report)
}

async fn apply_one_draft_operation<T>(
    session: &mut Session<T>,
    account: &MailAccount,
    operation: &RemoteDraftOperation,
    has_uid_plus: bool,
) -> Result<Option<RemoteDraftIdentity>, String>
where
    T: AsyncRead + AsyncWrite + Unpin + Debug + Send,
{
    if operation.target_mailbox.is_empty() {
        return Err("draft mailbox name is empty".into());
    }
    if let Some(previous) = &operation.previous_remote {
        delete_previous_draft(session, previous, has_uid_plus).await?;
    }
    let Some(outgoing) = &operation.message else {
        return Ok(None);
    };
    let message_id = draft_message_id(&operation.local_key);
    if let Some(existing) =
        find_draft_by_message_id(session, &operation.target_mailbox, &message_id).await?
    {
        return Ok(Some(existing));
    }
    let message =
        build_draft_message(account, outgoing, &message_id).map_err(|error| error.to_string())?;
    session
        .append(
            &operation.target_mailbox,
            Some(r"(\Draft)"),
            None,
            message.formatted(),
        )
        .await
        .map_err(|error| error.to_string())?;
    find_draft_by_message_id(session, &operation.target_mailbox, &message_id)
        .await?
        .ok_or_else(|| "uploaded draft UID could not be resolved".into())
        .map(Some)
}

async fn delete_previous_draft<T>(
    session: &mut Session<T>,
    previous: &RemoteDraftIdentity,
    has_uid_plus: bool,
) -> Result<(), String>
where
    T: AsyncRead + AsyncWrite + Unpin + Debug + Send,
{
    let selected = session
        .select(&previous.remote_mailbox)
        .await
        .map_err(|error| error.to_string())?;
    if selected.uid_validity.unwrap_or_default() != previous.uid_validity {
        return Err("UIDVALIDITY changed; the previous server draft was not removed".into());
    }
    let uid = previous.remote_uid.to_string();
    let exists = session
        .uid_fetch(&uid, "UID")
        .await
        .map_err(|error| error.to_string())?
        .try_collect::<Vec<_>>()
        .await
        .map_err(|error| error.to_string())?
        .iter()
        .any(|message| message.uid == Some(previous.remote_uid));
    if !exists {
        return Ok(());
    }
    if !has_uid_plus {
        return Err("server does not support UIDPLUS; draft replacement was kept local".into());
    }
    set_flag(session, &uid, r"\Deleted", true).await?;
    session
        .uid_expunge(&uid)
        .await
        .map_err(|error| error.to_string())?
        .try_collect::<Vec<_>>()
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

async fn find_draft_by_message_id<T>(
    session: &mut Session<T>,
    mailbox: &str,
    message_id: &str,
) -> Result<Option<RemoteDraftIdentity>, String>
where
    T: AsyncRead + AsyncWrite + Unpin + Debug + Send,
{
    let selected = session
        .select(mailbox)
        .await
        .map_err(|error| error.to_string())?;
    let uid_validity = selected.uid_validity.unwrap_or_default();
    let query = format!("HEADER Message-ID \"{message_id}\"");
    let remote_uid = session
        .uid_search(query)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .max();
    Ok(remote_uid.map(|remote_uid| RemoteDraftIdentity {
        remote_mailbox: mailbox.to_owned(),
        uid_validity,
        remote_uid,
    }))
}

fn draft_message_id(local_key: &str) -> String {
    format!("<{}@draft.maicenta.local>", stable_remote_key(local_key))
}

async fn apply_mutations_in_session<T>(
    session: &mut Session<T>,
    mutations: &[RemoteMutation],
) -> Result<MutationReport, ConnectorError>
where
    T: AsyncRead + AsyncWrite + Unpin + Debug + Send,
{
    let capabilities = session
        .capabilities()
        .await
        .map_err(|error| ConnectorError::Protocol(error.to_string()))?;
    let move_strategy = safe_move_strategy(
        capabilities.has_str("MOVE"),
        capabilities.has_str("UIDPLUS"),
    );
    let mut report = MutationReport {
        applied: Vec::new(),
        failed: Vec::new(),
    };

    for mutation in mutations {
        match apply_one_mutation(session, mutation, move_strategy).await {
            Ok(()) => report.applied.push(AppliedMutation {
                local_key: mutation.local_key.clone(),
                moved: mutation.target_mailbox.is_some(),
            }),
            Err(error) => report.failed.push(FailedMutation {
                local_key: mutation.local_key.clone(),
                error,
            }),
        }
    }

    Ok(report)
}

async fn apply_one_mutation<T>(
    session: &mut Session<T>,
    mutation: &RemoteMutation,
    move_strategy: MoveStrategy,
) -> Result<(), String>
where
    T: AsyncRead + AsyncWrite + Unpin + Debug + Send,
{
    let selected = session
        .select(&mutation.source_mailbox)
        .await
        .map_err(|error| error.to_string())?;
    if selected.uid_validity.unwrap_or_default() != mutation.uid_validity {
        return Err("UIDVALIDITY changed; local operation was not applied".into());
    }

    let uid = mutation.remote_uid.to_string();
    let fetched = session
        .uid_fetch(&uid, "UID")
        .await
        .map_err(|error| error.to_string())?
        .try_collect::<Vec<_>>()
        .await
        .map_err(|error| error.to_string())?;
    if !fetched
        .iter()
        .any(|message| message.uid == Some(mutation.remote_uid))
    {
        return Err("message UID no longer exists in the source mailbox".into());
    }

    set_flag(session, &uid, "\\Seen", mutation.seen).await?;
    set_flag(session, &uid, "\\Flagged", mutation.flagged).await?;

    if let Some(target_mailbox) = &mutation.target_mailbox {
        match move_strategy {
            MoveStrategy::Move => session
                .uid_mv(&uid, target_mailbox)
                .await
                .map_err(|error| error.to_string())?,
            MoveStrategy::CopyUidExpunge => {
                session
                    .uid_copy(&uid, target_mailbox)
                    .await
                    .map_err(|error| error.to_string())?;
                set_flag(session, &uid, "\\Deleted", true).await?;
                session
                    .uid_expunge(&uid)
                    .await
                    .map_err(|error| error.to_string())?
                    .try_collect::<Vec<_>>()
                    .await
                    .map_err(|error| error.to_string())?;
            }
            MoveStrategy::Unavailable => {
                return Err("server supports neither MOVE nor the safe UIDPLUS fallback".into());
            }
        }
    }

    Ok(())
}

async fn set_flag<T>(
    session: &mut Session<T>,
    uid: &str,
    flag: &str,
    enabled: bool,
) -> Result<(), String>
where
    T: AsyncRead + AsyncWrite + Unpin + Debug + Send,
{
    let operation = if enabled {
        format!("+FLAGS.SILENT ({flag})")
    } else {
        format!("-FLAGS.SILENT ({flag})")
    };
    session
        .uid_store(uid, operation)
        .await
        .map_err(|error| error.to_string())?
        .try_collect::<Vec<_>>()
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

async fn fetch_known_flag_updates<T>(
    session: &mut Session<T>,
    known_messages: &[&KnownRemoteMessage],
) -> Result<Vec<RemoteFlagUpdate>, ConnectorError>
where
    T: AsyncRead + AsyncWrite + Unpin + Debug + Send,
{
    let mut updates = Vec::new();
    for chunk in known_messages.chunks(200) {
        let sequence = chunk
            .iter()
            .map(|known| known.uid.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let fetched = session
            .uid_fetch(sequence, "(UID FLAGS)")
            .await
            .map_err(|error| ConnectorError::Protocol(error.to_string()))?
            .try_collect::<Vec<_>>()
            .await
            .map_err(|error| ConnectorError::Protocol(error.to_string()))?;
        for message in fetched {
            let Some(uid) = message.uid else {
                continue;
            };
            let Some(known) = chunk.iter().find(|known| known.uid == uid) else {
                continue;
            };
            updates.push(RemoteFlagUpdate {
                local_key: known.local_key.clone(),
                flags: message_flags(&message),
            });
        }
    }
    Ok(updates)
}

struct IncrementalMailboxWork<'a> {
    known_active: Vec<&'a KnownRemoteMessage>,
    body_uids: Vec<u32>,
    catalog_uids: Vec<u32>,
    catalog_remaining: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MailboxSyncPlan {
    Full,
    Delta { first_uid: u32, last_uid: u32 },
}

fn mailbox_sync_plan(
    checkpoint: Option<&RemoteMailboxCheckpoint>,
    uid_validity: u32,
    current_uid_next: Option<u32>,
) -> MailboxSyncPlan {
    let Some(checkpoint) = checkpoint else {
        return MailboxSyncPlan::Full;
    };
    if checkpoint.uid_validity != uid_validity
        || !checkpoint.catalog_complete
        || checkpoint.force_full_reconcile
    {
        return MailboxSyncPlan::Full;
    }
    let (Some(previous_uid_next), Some(current_uid_next)) = (checkpoint.uid_next, current_uid_next)
    else {
        return MailboxSyncPlan::Full;
    };
    if current_uid_next < previous_uid_next {
        return MailboxSyncPlan::Full;
    }
    MailboxSyncPlan::Delta {
        first_uid: previous_uid_next,
        last_uid: current_uid_next.saturating_sub(1),
    }
}

fn known_messages_for_mailbox<'a>(
    mailbox_name: &str,
    uid_validity: u32,
    known_messages: &'a [KnownRemoteMessage],
) -> Vec<&'a KnownRemoteMessage> {
    let mut known = known_messages
        .iter()
        .filter(|message| {
            message.remote_mailbox == mailbox_name && message.uid_validity == uid_validity
        })
        .collect::<Vec<_>>();
    known.sort_unstable_by_key(|message| std::cmp::Reverse(message.uid));
    known
}

struct ChangedFlagResult {
    updates: Vec<RemoteFlagUpdate>,
    vanished_uids: Vec<u32>,
    qresync_used: bool,
}

fn filter_known_vanished_uids(
    known_uids: &HashSet<u32>,
    vanished_ranges: &[std::ops::RangeInclusive<u32>],
) -> Vec<u32> {
    let mut vanished = known_uids
        .iter()
        .copied()
        .filter(|known_uid| {
            vanished_ranges
                .iter()
                .any(|range| range.contains(known_uid))
        })
        .collect::<Vec<_>>();
    vanished.sort_unstable();
    vanished
}

fn drain_vanished_uids<T>(session: &Session<T>, known_uids: &HashSet<u32>) -> Vec<u32>
where
    T: AsyncRead + AsyncWrite + Unpin + Debug,
{
    let mut vanished = HashSet::new();
    while let Ok(response) = session.unsolicited_responses.try_recv() {
        let UnsolicitedResponse::Other(data) = response else {
            continue;
        };
        let ImapResponse::Vanished { uids, .. } = data.parsed() else {
            continue;
        };
        vanished.extend(filter_known_vanished_uids(known_uids, uids));
    }
    let mut vanished = vanished.into_iter().collect::<Vec<_>>();
    vanished.sort_unstable();
    vanished
}

async fn fetch_changed_flag_updates<T>(
    session: &mut Session<T>,
    known_messages: &[&KnownRemoteMessage],
    changed_since: u64,
    request_vanished: bool,
) -> Result<ChangedFlagResult, ConnectorError>
where
    T: AsyncRead + AsyncWrite + Unpin + Debug + Send,
{
    if known_messages.is_empty() {
        return Ok(ChangedFlagResult {
            updates: Vec::new(),
            vanished_uids: Vec::new(),
            qresync_used: request_vanished,
        });
    }
    let known_by_uid = known_messages
        .iter()
        .map(|known| (known.uid, *known))
        .collect::<HashMap<_, _>>();
    let modifier = if request_vanished {
        format!("(CHANGEDSINCE {changed_since} VANISHED)")
    } else {
        format!("(CHANGEDSINCE {changed_since})")
    };
    let fetched = session
        .uid_fetch("1:*", format!("(UID FLAGS MODSEQ) {modifier}"))
        .await
        .map_err(|error| ConnectorError::Protocol(error.to_string()))?
        .try_collect::<Vec<_>>()
        .await
        .map_err(|error| ConnectorError::Protocol(error.to_string()))?;
    let updates = fetched
        .into_iter()
        .filter_map(|message| {
            let known = known_by_uid.get(&message.uid?)?;
            Some(RemoteFlagUpdate {
                local_key: known.local_key.clone(),
                flags: message_flags(&message),
            })
        })
        .collect();
    let known_uids = known_by_uid.keys().copied().collect::<HashSet<_>>();
    let vanished_uids = if request_vanished {
        drain_vanished_uids(session, &known_uids)
    } else {
        Vec::new()
    };
    Ok(ChangedFlagResult {
        updates,
        vanished_uids,
        qresync_used: request_vanished,
    })
}

fn plan_incremental_mailbox_work<'a>(
    mailbox_name: &str,
    uid_validity: u32,
    sorted_uids: &[u32],
    known_messages: &'a [KnownRemoteMessage],
    message_limit: usize,
) -> IncrementalMailboxWork<'a> {
    let active_uids = sorted_uids.iter().copied().collect::<HashSet<_>>();
    let matching_known = known_messages
        .iter()
        .filter(|known| known.remote_mailbox == mailbox_name && known.uid_validity == uid_validity)
        .collect::<Vec<_>>();
    let known_catalog_uids = matching_known
        .iter()
        .map(|known| known.uid)
        .collect::<HashSet<_>>();
    let highest_known_uid = matching_known.iter().map(|known| known.uid).max();
    let retry_uids = matching_known
        .iter()
        .copied()
        .filter(|known| known.needs_body_refresh && active_uids.contains(&known.uid))
        .map(|known| known.uid)
        .collect::<Vec<_>>();
    let mut known_active = matching_known
        .iter()
        .copied()
        .filter(|known| active_uids.contains(&known.uid))
        .collect::<Vec<_>>();
    known_active.sort_unstable_by_key(|known| std::cmp::Reverse(known.uid));
    known_active.truncate(MAX_FLAG_REFRESH_PER_MAILBOX);
    let body_uids =
        select_incremental_uids(sorted_uids, highest_known_uid, &retry_uids, message_limit);
    let mut missing_catalog_uids = sorted_uids
        .iter()
        .rev()
        .copied()
        .filter(|uid| !known_catalog_uids.contains(uid))
        .collect::<Vec<_>>();
    let mut catalog_retry_uids = matching_known
        .iter()
        .filter(|known| known.needs_catalog_refresh && active_uids.contains(&known.uid))
        .map(|known| known.uid)
        .collect::<Vec<_>>();
    catalog_retry_uids.sort_unstable_by(|left, right| right.cmp(left));
    // A malformed legacy header must not starve genuinely unknown older mail.
    // Refreshes are therefore attempted after the newest unknown UIDs.
    missing_catalog_uids.extend(catalog_retry_uids);
    let catalog_remaining = missing_catalog_uids
        .len()
        .saturating_sub(MAX_CATALOG_MESSAGES_PER_MAILBOX);
    let mut catalog_uids = missing_catalog_uids
        .into_iter()
        .take(MAX_CATALOG_MESSAGES_PER_MAILBOX)
        .collect::<Vec<_>>();
    catalog_uids.sort_unstable();
    IncrementalMailboxWork {
        known_active,
        body_uids,
        catalog_uids,
        catalog_remaining,
    }
}

async fn select_mailbox_for_sync<T>(
    session: &mut Session<T>,
    mailbox_name: &str,
    supports_condstore: bool,
) -> Result<async_imap::types::Mailbox, ConnectorError>
where
    T: AsyncRead + AsyncWrite + Unpin + Debug + Send,
{
    if supports_condstore {
        if let Ok(selected) = session.select_condstore(mailbox_name).await {
            return Ok(selected);
        }
    }
    session
        .select(mailbox_name)
        .await
        .map_err(|error| ConnectorError::Protocol(error.to_string()))
}

async fn fetch_sync_uids<T>(
    session: &mut Session<T>,
    plan: MailboxSyncPlan,
) -> Result<(Vec<u32>, bool), ConnectorError>
where
    T: AsyncRead + AsyncWrite + Unpin + Debug + Send,
{
    let (searched, full_reconcile) = match plan {
        MailboxSyncPlan::Full => (
            session
                .uid_search("ALL")
                .await
                .map_err(|error| ConnectorError::Protocol(error.to_string()))?,
            true,
        ),
        MailboxSyncPlan::Delta {
            first_uid,
            last_uid,
        } if first_uid <= last_uid => (
            session
                .uid_search(format!("UID {first_uid}:{last_uid}"))
                .await
                .map_err(|error| ConnectorError::Protocol(error.to_string()))?,
            false,
        ),
        MailboxSyncPlan::Delta { .. } => return Ok((Vec::new(), false)),
    };
    let mut uids = searched.into_iter().collect::<Vec<_>>();
    uids.sort_unstable();
    Ok((uids, full_reconcile))
}

struct FlagSyncContext<'a> {
    mailbox_name: &'a str,
    uid_validity: u32,
    known_messages: &'a [KnownRemoteMessage],
    active_known: Vec<&'a KnownRemoteMessage>,
    checkpoint: Option<&'a RemoteMailboxCheckpoint>,
    highest_modseq: Option<u64>,
    capabilities: SyncCapabilities,
    full_reconcile: bool,
    catalog_will_complete: bool,
}

struct FlagSyncResult {
    updates: Vec<RemoteFlagUpdate>,
    vanished_uids: Vec<u32>,
    highest_modseq: Option<u64>,
    qresync_used: bool,
}

async fn fetch_mailbox_flag_updates<T>(
    session: &mut Session<T>,
    context: FlagSyncContext<'_>,
) -> Result<FlagSyncResult, ConnectorError>
where
    T: AsyncRead + AsyncWrite + Unpin + Debug + Send,
{
    let known_in_mailbox = known_messages_for_mailbox(
        context.mailbox_name,
        context.uid_validity,
        context.known_messages,
    );
    let full_known_refresh = context.full_reconcile
        && (context.catalog_will_complete
            || context
                .checkpoint
                .is_some_and(|checkpoint| checkpoint.catalog_complete));
    let fallback_known = if full_known_refresh {
        known_in_mailbox.clone()
    } else if context.full_reconcile {
        context.active_known
    } else {
        known_in_mailbox
            .iter()
            .copied()
            .take(MAX_FLAG_REFRESH_PER_MAILBOX)
            .collect::<Vec<_>>()
    };
    let previous_modseq = context
        .checkpoint
        .filter(|checkpoint| checkpoint.uid_validity == context.uid_validity)
        .and_then(|checkpoint| checkpoint.highest_modseq);
    if !context.capabilities.condstore {
        return Ok(FlagSyncResult {
            updates: fetch_known_flag_updates(session, &fallback_known).await?,
            vanished_uids: Vec::new(),
            highest_modseq: previous_modseq,
            qresync_used: false,
        });
    }
    let modseq_pair = previous_modseq.zip(context.highest_modseq);
    match modseq_pair {
        Some((previous, current)) if current > previous => {
            let changed = if context.capabilities.qresync {
                match fetch_changed_flag_updates(session, &known_in_mailbox, previous, true).await {
                    Ok(changed) => Ok(changed),
                    Err(_) => {
                        fetch_changed_flag_updates(session, &known_in_mailbox, previous, false)
                            .await
                    }
                }
            } else {
                fetch_changed_flag_updates(session, &known_in_mailbox, previous, false).await
            };
            match changed {
                Ok(changed) => Ok(FlagSyncResult {
                    updates: changed.updates,
                    vanished_uids: changed.vanished_uids,
                    highest_modseq: Some(current),
                    qresync_used: changed.qresync_used,
                }),
                Err(_) => Ok(FlagSyncResult {
                    updates: fetch_known_flag_updates(session, &fallback_known).await?,
                    vanished_uids: Vec::new(),
                    highest_modseq: full_known_refresh.then_some(current).or(previous_modseq),
                    qresync_used: false,
                }),
            }
        }
        Some((previous, current)) if current == previous => Ok(FlagSyncResult {
            updates: Vec::new(),
            vanished_uids: Vec::new(),
            highest_modseq: Some(current),
            qresync_used: false,
        }),
        _ => Ok(FlagSyncResult {
            updates: fetch_known_flag_updates(session, &fallback_known).await?,
            vanished_uids: Vec::new(),
            highest_modseq: full_known_refresh
                .then_some(context.highest_modseq)
                .flatten()
                .or(previous_modseq),
            qresync_used: false,
        }),
    }
}

async fn fetch_mailbox_messages<T>(
    session: &mut Session<T>,
    mailbox: &RemoteMailbox,
    known_messages: &[KnownRemoteMessage],
    checkpoint: Option<&RemoteMailboxCheckpoint>,
    message_limit: usize,
    capabilities: SyncCapabilities,
) -> Result<MailboxFetchResult, ConnectorError>
where
    T: AsyncRead + AsyncWrite + Unpin + Debug + Send,
{
    let selected =
        select_mailbox_for_sync(session, &mailbox.remote_name, capabilities.condstore).await?;
    let uid_validity = selected.uid_validity.unwrap_or_default();
    let plan = mailbox_sync_plan(checkpoint, uid_validity, selected.uid_next);
    let (mut uids, full_reconcile) = fetch_sync_uids(session, plan).await?;
    let active_uids = uids.clone();
    if !full_reconcile {
        uids.extend(
            known_messages
                .iter()
                .filter(|known| {
                    known.remote_mailbox == mailbox.remote_name
                        && known.uid_validity == uid_validity
                        && (known.needs_catalog_refresh || known.needs_body_refresh)
                })
                .map(|known| known.uid),
        );
        uids.sort_unstable();
        uids.dedup();
    }

    let mut work = plan_incremental_mailbox_work(
        &mailbox.remote_name,
        uid_validity,
        &uids,
        known_messages,
        message_limit,
    );
    let flag_sync = fetch_mailbox_flag_updates(
        session,
        FlagSyncContext {
            mailbox_name: &mailbox.remote_name,
            uid_validity,
            known_messages,
            active_known: work.known_active.clone(),
            checkpoint,
            highest_modseq: selected.highest_modseq,
            capabilities,
            full_reconcile,
            catalog_will_complete: work.catalog_remaining == 0,
        },
    )
    .await?;
    let body_uids = work.body_uids;
    let body_uid_set = body_uids.iter().copied().collect::<HashSet<_>>();
    let mut selected_uids = work.catalog_uids;
    selected_uids.extend(body_uids);
    selected_uids.sort_unstable();
    selected_uids.dedup();
    if selected_uids.is_empty() {
        return Ok(MailboxFetchResult {
            messages: Vec::new(),
            flag_updates: flag_sync.updates,
            state: RemoteMailboxState {
                remote_mailbox: mailbox.remote_name.clone(),
                uid_validity,
                uid_next: selected.uid_next,
                highest_modseq: flag_sync.highest_modseq,
                active_uids,
                full_reconcile,
                vanished_uids: flag_sync.vanished_uids,
                qresync_used: flag_sync.qresync_used,
                catalog_remaining: work.catalog_remaining,
            },
        });
    }
    let pending = fetch_pending_message_metadata(session, &selected_uids).await?;

    let mut messages = Vec::with_capacity(pending.len());
    for pending_message in pending {
        if body_uid_set.contains(&pending_message.uid) {
            messages.push(
                fetch_selective_message_parts(session, mailbox, uid_validity, pending_message)
                    .await,
            );
        } else {
            messages.push(catalog_message(mailbox, uid_validity, pending_message));
        }
    }
    work.catalog_remaining = work.catalog_remaining.saturating_add(
        messages
            .iter()
            .filter(|message| !message.catalog_complete)
            .count(),
    );

    Ok(MailboxFetchResult {
        messages,
        flag_updates: flag_sync.updates,
        state: RemoteMailboxState {
            remote_mailbox: mailbox.remote_name.clone(),
            uid_validity,
            uid_next: selected.uid_next,
            highest_modseq: flag_sync.highest_modseq,
            active_uids,
            full_reconcile,
            vanished_uids: flag_sync.vanished_uids,
            qresync_used: flag_sync.qresync_used,
            catalog_remaining: work.catalog_remaining,
        },
    })
}

async fn fetch_pending_message_metadata<T>(
    session: &mut Session<T>,
    selected_uids: &[u32],
) -> Result<Vec<PendingRemoteMessage>, ConnectorError>
where
    T: AsyncRead + AsyncWrite + Unpin + Debug + Send,
{
    let sequence = selected_uids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let fetched = session
        .uid_fetch(
            sequence,
            format!(
                "(UID FLAGS BODY.PEEK[HEADER]<0.{}> BODYSTRUCTURE)",
                MAX_SYNC_HEADER_BYTES + 1
            ),
        )
        .await
        .map_err(|error| ConnectorError::Protocol(error.to_string()))?
        .try_collect::<Vec<_>>()
        .await
        .map_err(|error| ConnectorError::Protocol(error.to_string()))?;
    Ok(fetched
        .into_iter()
        .filter_map(|message| {
            let uid = message.uid?;
            let header = message.header().unwrap_or_default().to_vec();
            let (attachments, renderable_parts, structure_complete) =
                message.bodystructure().map_or_else(
                    || (Vec::new(), Vec::new(), false),
                    |structure| {
                        let (renderable_parts, renderable_complete) =
                            renderable_parts_from_bodystructure(structure);
                        (
                            remote_attachments_from_bodystructure(structure),
                            renderable_parts,
                            renderable_complete,
                        )
                    },
                );
            Some(PendingRemoteMessage {
                uid,
                flags: message_flags(&message),
                body_complete: structure_complete
                    && !header.is_empty()
                    && header.len() <= MAX_SYNC_HEADER_BYTES,
                header,
                attachments,
                renderable_parts,
            })
        })
        .collect())
}

fn select_incremental_uids(
    sorted_uids: &[u32],
    highest_known_uid: Option<u32>,
    retry_uids: &[u32],
    message_limit: usize,
) -> Vec<u32> {
    if let Some(highest_known_uid) = highest_known_uid {
        let mut selected = sorted_uids
            .iter()
            .copied()
            .filter(|uid| *uid > highest_known_uid)
            .collect::<Vec<_>>();
        let mut retries = retry_uids.to_vec();
        retries.sort_unstable_by(|left, right| right.cmp(left));
        retries.dedup();
        let selected_set = selected.iter().copied().collect::<HashSet<_>>();
        selected.extend(
            retries
                .into_iter()
                .filter(|uid| !selected_set.contains(uid)),
        );
        selected.truncate(message_limit);
        selected
    } else {
        sorted_uids[sorted_uids.len().saturating_sub(message_limit)..].to_vec()
    }
}

fn catalog_message(
    mailbox: &RemoteMailbox,
    uid_validity: u32,
    mut pending: PendingRemoteMessage,
) -> RemoteMessage {
    let catalog_complete =
        !pending.header.is_empty() && pending.header.len() <= MAX_SYNC_HEADER_BYTES;
    if pending.header.len() > MAX_SYNC_HEADER_BYTES {
        pending.header.clear();
    }
    RemoteMessage {
        remote_mailbox: mailbox.remote_name.clone(),
        mailbox_role: mailbox.role,
        uid_validity,
        uid: pending.uid,
        flags: pending.flags,
        renderable_message: assemble_selective_message(&pending.header, pending.uid, &[], false),
        attachments: pending.attachments,
        catalog_complete,
        body_requested: false,
        body_complete: false,
    }
}

fn message_flags(message: &async_imap::types::Fetch) -> Vec<MessageFlag> {
    message
        .flags()
        .filter_map(|flag| match flag {
            async_imap::types::Flag::Seen => Some(MessageFlag::Seen),
            async_imap::types::Flag::Answered => Some(MessageFlag::Answered),
            async_imap::types::Flag::Flagged => Some(MessageFlag::Flagged),
            async_imap::types::Flag::Draft => Some(MessageFlag::Draft),
            async_imap::types::Flag::Deleted => Some(MessageFlag::Deleted),
            _ => None,
        })
        .collect()
}

async fn fetch_selective_message_parts<T>(
    session: &mut Session<T>,
    mailbox: &RemoteMailbox,
    uid_validity: u32,
    mut pending: PendingRemoteMessage,
) -> RemoteMessage
where
    T: AsyncRead + AsyncWrite + Unpin + Debug + Send,
{
    let catalog_complete =
        !pending.header.is_empty() && pending.header.len() <= MAX_SYNC_HEADER_BYTES;
    let mut fetched_parts = Vec::new();
    if !pending.renderable_parts.is_empty() {
        let query = pending
            .renderable_parts
            .iter()
            .map(|part| {
                format!(
                    "BODY.PEEK[{}]<0.{}>",
                    part.section,
                    part.maximum_bytes.saturating_add(1)
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        let response = match session
            .uid_fetch(pending.uid.to_string(), format!("(UID {query})"))
            .await
        {
            Ok(stream) => stream.try_collect::<Vec<_>>().await.ok(),
            Err(_) => None,
        };
        if response.is_none() {
            pending.body_complete = false;
        }
        let fetched = response.as_ref().and_then(|response| {
            response
                .iter()
                .find(|message| message.uid == Some(pending.uid))
        });
        let mut text_total = 0_usize;
        let mut inline_total = 0_usize;
        for part in &pending.renderable_parts {
            let bytes = fetched.and_then(|message| {
                if part.path.is_empty() {
                    message.text()
                } else {
                    message.section(&SectionPath::Part(part.path.clone(), None))
                }
            });
            let total = match part.role {
                RenderablePartRole::Text => &mut text_total,
                RenderablePartRole::InlineImage => &mut inline_total,
            };
            let total_limit = match part.role {
                RenderablePartRole::Text => MAX_SYNC_TEXT_TOTAL_BYTES,
                RenderablePartRole::InlineImage => MAX_SYNC_INLINE_TOTAL_BYTES,
            };
            match bytes {
                Some(bytes)
                    if bytes.len() <= part.maximum_bytes
                        && bytes.len() <= total_limit.saturating_sub(*total) =>
                {
                    *total += bytes.len();
                    fetched_parts.push((part.clone(), bytes.to_vec()));
                }
                _ => pending.body_complete = false,
            }
        }
    }
    if pending.header.len() > MAX_SYNC_HEADER_BYTES {
        pending.header.clear();
    }
    let renderable_message = assemble_selective_message(
        &pending.header,
        pending.uid,
        &fetched_parts,
        pending.body_complete,
    );
    RemoteMessage {
        remote_mailbox: mailbox.remote_name.clone(),
        mailbox_role: mailbox.role,
        uid_validity,
        uid: pending.uid,
        flags: pending.flags,
        renderable_message,
        attachments: pending.attachments,
        catalog_complete,
        body_requested: true,
        body_complete: pending.body_complete,
    }
}

fn remote_attachments_from_bodystructure(root: &BodyStructure<'_>) -> Vec<RemoteAttachmentPart> {
    let mut attachments = Vec::new();
    collect_remote_attachments(root, &mut Vec::new(), &mut attachments);
    attachments
}

fn renderable_parts_from_bodystructure(root: &BodyStructure<'_>) -> (Vec<RenderablePart>, bool) {
    let mut text_parts = Vec::new();
    let mut text_total = 0_usize;
    let mut complete = true;
    collect_primary_text_parts(
        root,
        &mut Vec::new(),
        &mut text_parts,
        &mut text_total,
        &mut complete,
    );

    let mut inline_parts = Vec::new();
    let mut inline_total = 0_usize;
    collect_inline_image_parts(
        root,
        &mut Vec::new(),
        &mut inline_parts,
        &mut inline_total,
        &mut complete,
    );
    text_parts.extend(inline_parts);
    (text_parts, complete)
}

fn collect_primary_text_parts(
    node: &BodyStructure<'_>,
    section_path: &mut Vec<u32>,
    parts: &mut Vec<RenderablePart>,
    total_bytes: &mut usize,
    complete: &mut bool,
) -> bool {
    match node {
        BodyStructure::Multipart { common, bodies, .. } => {
            let alternative = common.ty.subtype.eq_ignore_ascii_case("alternative");
            let mut found = false;
            for (index, child) in bodies.iter().enumerate() {
                let Ok(section) = u32::try_from(index + 1) else {
                    *complete = false;
                    continue;
                };
                section_path.push(section);
                let child_found =
                    collect_primary_text_parts(child, section_path, parts, total_bytes, complete);
                section_path.pop();
                found |= child_found;
                if child_found && !alternative {
                    break;
                }
            }
            found
        }
        BodyStructure::Text { common, other, .. } => {
            if bodystructure_attachment_name(common).is_some()
                || !common.ty.ty.eq_ignore_ascii_case("text")
                || !(common.ty.subtype.eq_ignore_ascii_case("plain")
                    || common.ty.subtype.eq_ignore_ascii_case("html"))
            {
                return false;
            }
            let size = other.octets as usize;
            if parts.len() >= MAX_SYNC_TEXT_PARTS
                || size > MAX_SYNC_TEXT_PART_BYTES
                || size > MAX_SYNC_TEXT_TOTAL_BYTES.saturating_sub(*total_bytes)
            {
                *complete = false;
                return true;
            }
            let Some(part) = renderable_part(common, other, section_path, RenderablePartRole::Text)
            else {
                *complete = false;
                return true;
            };
            *total_bytes += size;
            parts.push(part);
            true
        }
        BodyStructure::Message { common, .. } => {
            if bodystructure_attachment_name(common).is_none() {
                *complete = false;
                true
            } else {
                false
            }
        }
        BodyStructure::Basic { .. } => false,
    }
}

fn collect_inline_image_parts(
    node: &BodyStructure<'_>,
    section_path: &mut Vec<u32>,
    parts: &mut Vec<RenderablePart>,
    total_bytes: &mut usize,
    complete: &mut bool,
) {
    if let BodyStructure::Multipart { bodies, .. } = node {
        for (index, child) in bodies.iter().enumerate() {
            let Ok(section) = u32::try_from(index + 1) else {
                *complete = false;
                continue;
            };
            section_path.push(section);
            collect_inline_image_parts(child, section_path, parts, total_bytes, complete);
            section_path.pop();
        }
        return;
    }

    let (common, other) = match node {
        BodyStructure::Basic { common, other, .. } | BodyStructure::Text { common, other, .. } => {
            (common, other)
        }
        BodyStructure::Message { .. } | BodyStructure::Multipart { .. } => return,
    };
    let is_supported_raster = common.ty.ty.eq_ignore_ascii_case("image")
        && matches!(
            common.ty.subtype.to_ascii_lowercase().as_str(),
            "png" | "jpeg" | "jpg" | "gif"
        );
    let is_inline = common
        .disposition
        .as_ref()
        .is_some_and(|value| value.ty.eq_ignore_ascii_case("inline"))
        || other.id.is_some();
    if !is_supported_raster || !is_inline {
        return;
    }
    let size = other.octets as usize;
    if parts.len() >= MAX_SYNC_INLINE_PARTS
        || size > MAX_SYNC_INLINE_PART_BYTES
        || size > MAX_SYNC_INLINE_TOTAL_BYTES.saturating_sub(*total_bytes)
    {
        *complete = false;
        return;
    }
    let Some(part) = renderable_part(common, other, section_path, RenderablePartRole::InlineImage)
    else {
        *complete = false;
        return;
    };
    *total_bytes += size;
    parts.push(part);
}

fn renderable_part(
    common: &BodyContentCommon<'_>,
    other: &BodyContentSinglePart<'_>,
    section_path: &[u32],
    role: RenderablePartRole,
) -> Option<RenderablePart> {
    let path = section_path.to_vec();
    let section = section_name(section_path);
    let content_type = format!(
        "{}/{}",
        validated_mime_token(&common.ty.ty)?,
        validated_mime_token(&common.ty.subtype)?
    );
    let mut headers = format!("Content-Type: {content_type}");
    if let Some(parameters) = common.ty.params.as_deref() {
        for (name, value) in parameters.iter().take(8) {
            let Some(name) = validated_mime_token(name) else {
                continue;
            };
            let Some(value) = safe_mime_header_value(value, 512) else {
                continue;
            };
            write!(headers, "; {name}=\"{}\"", escape_mime_parameter(value))
                .expect("writing to a String cannot fail");
        }
    }
    headers.push_str("\r\nContent-Transfer-Encoding: ");
    let transfer_encoding = transfer_encoding_name(&other.transfer_encoding);
    headers.push_str(validated_mime_token(&transfer_encoding)?);
    headers.push_str("\r\n");
    if let Some(content_id) = other
        .id
        .as_deref()
        .and_then(|value| safe_mime_header_value(value, 998))
    {
        headers.push_str("Content-ID: ");
        headers.push_str(content_id);
        headers.push_str("\r\n");
    }
    if role == RenderablePartRole::InlineImage {
        headers.push_str("Content-Disposition: inline\r\n");
    }
    Some(RenderablePart {
        section,
        path,
        mime_headers: headers.into_bytes(),
        role,
        maximum_bytes: other.octets as usize,
    })
}

fn validated_mime_token(value: &str) -> Option<&str> {
    (!value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
                )
        }))
    .then_some(value)
}

fn safe_mime_header_value(value: &str, maximum_length: usize) -> Option<&str> {
    (value.len() <= maximum_length && !value.chars().any(char::is_control)).then_some(value)
}

fn escape_mime_parameter(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn assemble_selective_message(
    original_header: &[u8],
    uid: u32,
    fetched_parts: &[(RenderablePart, Vec<u8>)],
    body_complete: bool,
) -> Vec<u8> {
    let serial = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let outer_boundary = format!("maicenta-related-{uid}-{serial:x}");
    let alternative_boundary = format!("maicenta-alternative-{uid}-{serial:x}");
    let mut message = filtered_message_headers(original_header);
    message.extend_from_slice(b"MIME-Version: 1.0\r\n");
    message.extend_from_slice(
        format!("Content-Type: multipart/related; boundary=\"{outer_boundary}\"\r\n\r\n")
            .as_bytes(),
    );

    let text_parts = fetched_parts
        .iter()
        .filter(|(part, _)| part.role == RenderablePartRole::Text)
        .collect::<Vec<_>>();
    let inline_parts = fetched_parts
        .iter()
        .filter(|(part, _)| part.role == RenderablePartRole::InlineImage)
        .collect::<Vec<_>>();

    message.extend_from_slice(format!("--{outer_boundary}\r\n").as_bytes());
    if text_parts.len() > 1 {
        message.extend_from_slice(
            format!(
                "Content-Type: multipart/alternative; boundary=\"{alternative_boundary}\"\r\n\r\n"
            )
            .as_bytes(),
        );
        for (part, body) in text_parts {
            message.extend_from_slice(format!("--{alternative_boundary}\r\n").as_bytes());
            append_mime_part(&mut message, part, body);
        }
        message.extend_from_slice(format!("--{alternative_boundary}--\r\n").as_bytes());
    } else if let Some((part, body)) = text_parts.first() {
        append_mime_part(&mut message, part, body);
    } else {
        message.extend_from_slice(b"Content-Type: text/plain; charset=utf-8\r\n\r\n");
        if body_complete {
            message.extend_from_slice(b"Message without a displayable text body.\r\n");
        } else {
            message.extend_from_slice(
                b"The displayable message body could not be downloaded completely.\r\n",
            );
        }
    }

    for (part, body) in inline_parts {
        message.extend_from_slice(format!("--{outer_boundary}\r\n").as_bytes());
        append_mime_part(&mut message, part, body);
    }
    message.extend_from_slice(format!("--{outer_boundary}--\r\n").as_bytes());
    message
}

fn append_mime_part(message: &mut Vec<u8>, part: &RenderablePart, body: &[u8]) {
    message.extend_from_slice(&part.mime_headers);
    message.extend_from_slice(b"\r\n");
    message.extend_from_slice(body);
    if !body.ends_with(b"\n") {
        message.extend_from_slice(b"\r\n");
    }
}

fn filtered_message_headers(original: &[u8]) -> Vec<u8> {
    let mut filtered = Vec::new();
    let mut skip_continuation = false;
    for raw_line in original.split(|byte| *byte == b'\n') {
        let line = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
        if line.is_empty() {
            break;
        }
        let continuation = matches!(line.first(), Some(b' ' | b'\t'));
        if continuation {
            if !skip_continuation {
                filtered.extend_from_slice(line);
                filtered.extend_from_slice(b"\r\n");
            }
            continue;
        }
        let name = line.split(|byte| *byte == b':').next().unwrap_or_default();
        skip_continuation = [
            b"content-type".as_slice(),
            b"content-transfer-encoding".as_slice(),
            b"content-disposition".as_slice(),
            b"mime-version".as_slice(),
        ]
        .iter()
        .any(|blocked| name.eq_ignore_ascii_case(blocked));
        if !skip_continuation {
            filtered.extend_from_slice(line);
            filtered.extend_from_slice(b"\r\n");
        }
    }
    filtered
}

fn collect_remote_attachments(
    node: &BodyStructure<'_>,
    section_path: &mut Vec<u32>,
    attachments: &mut Vec<RemoteAttachmentPart>,
) {
    if let BodyStructure::Multipart { bodies, .. } = node {
        for (index, child) in bodies.iter().enumerate() {
            let Ok(section) = u32::try_from(index + 1) else {
                continue;
            };
            section_path.push(section);
            collect_remote_attachments(child, section_path, attachments);
            section_path.pop();
        }
        return;
    }

    let (common, single_part) = match node {
        BodyStructure::Basic { common, other, .. }
        | BodyStructure::Text { common, other, .. }
        | BodyStructure::Message { common, other, .. } => (common, other),
        BodyStructure::Multipart { .. } => return,
    };
    let Some(file_name) = bodystructure_attachment_name(common) else {
        return;
    };
    let section = section_name(section_path);
    let content_type = format!(
        "{}/{}",
        common.ty.ty.to_ascii_lowercase(),
        common.ty.subtype.to_ascii_lowercase()
    );
    attachments.push(RemoteAttachmentPart {
        section,
        file_name,
        content_type,
        decoded_size_hint: decoded_size_hint(single_part),
        transfer_encoding: transfer_encoding_name(&single_part.transfer_encoding),
    });
}

fn section_name(section_path: &[u32]) -> String {
    if section_path.is_empty() {
        "TEXT".into()
    } else {
        section_path
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(".")
    }
}

fn bodystructure_attachment_name(common: &BodyContentCommon<'_>) -> Option<String> {
    let disposition = common.disposition.as_ref();
    if disposition.is_some_and(|value| value.ty.eq_ignore_ascii_case("inline")) {
        return None;
    }
    let disposition_name =
        disposition.and_then(|value| body_parameter(value.params.as_deref(), "filename"));
    let type_name = body_parameter(common.ty.params.as_deref(), "name");
    let is_attachment =
        disposition.is_some_and(|value| value.ty.eq_ignore_ascii_case("attachment"));
    if !is_attachment && disposition_name.is_none() && type_name.is_none() {
        return None;
    }
    disposition_name
        .or(type_name)
        .map(str::to_owned)
        .or_else(|| Some("attachment.bin".into()))
}

fn body_parameter<'a>(
    parameters: Option<&'a [(std::borrow::Cow<'a, str>, std::borrow::Cow<'a, str>)]>,
    name: &str,
) -> Option<&'a str> {
    parameters?
        .iter()
        .find_map(|(key, value)| key.eq_ignore_ascii_case(name).then_some(value.as_ref()))
}

fn decoded_size_hint(part: &BodyContentSinglePart<'_>) -> u64 {
    let octets = u64::from(part.octets);
    match part.transfer_encoding {
        ContentEncoding::Base64 => octets.saturating_mul(3).saturating_div(4),
        _ => octets,
    }
}

fn transfer_encoding_name(encoding: &ContentEncoding<'_>) -> String {
    match encoding {
        ContentEncoding::SevenBit => "7bit".into(),
        ContentEncoding::EightBit => "8bit".into(),
        ContentEncoding::Binary => "binary".into(),
        ContentEncoding::Base64 => "base64".into(),
        ContentEncoding::QuotedPrintable => "quoted-printable".into(),
        ContentEncoding::Other(value) => value.to_ascii_lowercase(),
    }
}

const fn mailbox_sync_priority(role: MailboxRole) -> u8 {
    match role {
        MailboxRole::Inbox => 0,
        MailboxRole::Drafts => 1,
        MailboxRole::Sent => 2,
        MailboxRole::Archive => 3,
        MailboxRole::Trash => 4,
        MailboxRole::Junk => 5,
        MailboxRole::Custom => 6,
    }
}

fn mailbox_role(name: &str, attributes: &[NameAttribute<'_>]) -> MailboxRole {
    for attribute in attributes {
        match attribute {
            NameAttribute::Drafts => return MailboxRole::Drafts,
            NameAttribute::Sent => return MailboxRole::Sent,
            NameAttribute::Archive => return MailboxRole::Archive,
            NameAttribute::Trash => return MailboxRole::Trash,
            NameAttribute::Junk => return MailboxRole::Junk,
            _ => {}
        }
    }
    let leaf_name = name
        .rsplit(['.', '/', '\\'])
        .next()
        .unwrap_or(name)
        .trim()
        .to_ascii_lowercase();
    match leaf_name.as_str() {
        "inbox" => MailboxRole::Inbox,
        "draft" | "drafts" | "entwurf" | "entwürfe" => MailboxRole::Drafts,
        "sent" | "sent items" | "sent messages" | "gesendet" => MailboxRole::Sent,
        "archive" | "archiv" => MailboxRole::Archive,
        "trash" | "deleted items" | "deleted messages" | "papierkorb" => MailboxRole::Trash,
        "junk" | "junk email" | "spam" | "spambucket" => MailboxRole::Junk,
        _ => MailboxRole::Custom,
    }
}

fn smtp_transport(
    account: &MailAccount,
    password: &str,
) -> Result<AsyncSmtpTransport<Tokio1Executor>, ConnectorError> {
    let builder = match account.smtp_security {
        TransportSecurity::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&account.smtp_host),
        TransportSecurity::StartTls => {
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&account.smtp_host)
        }
    }
    .map_err(|error| ConnectorError::InvalidConfiguration(error.to_string()))?;
    Ok(builder
        .port(account.smtp_port)
        .credentials(Credentials::new(
            account.smtp_username.clone(),
            password.to_owned(),
        ))
        .timeout(Some(NETWORK_TIMEOUT))
        .build())
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use async_imap::imap_proto::{
        parser::parse_response,
        types::{AttributeValue, Response},
    };
    use maicenta_domain::{AccountId, MailAccount, MailAddress, MailboxRole, TransportSecurity};
    use maicenta_rendering::{MessageRenderer, RenderPolicy};

    use super::{
        KnownRemoteMessage, MailboxSyncPlan, MoveStrategy, OutgoingAttachment, OutgoingMessage,
        RemoteMailbox, RemoteMailboxCheckpoint, RenderablePartRole, ValidatedSectionPath,
        assemble_selective_message, build_draft_message, build_message, draft_message_id,
        filter_known_vanished_uids, mailbox_role, mailbox_sync_plan, mailbox_sync_priority,
        ordered_mailboxes_for_sync, plan_incremental_mailbox_work,
        remote_attachments_from_bodystructure, renderable_parts_from_bodystructure,
        safe_move_strategy, select_incremental_uids, stable_remote_key, validated_section_path,
    };

    fn mail_account() -> MailAccount {
        MailAccount {
            id: AccountId::parse("work").expect("account id"),
            display_name: "MAICENTA User".into(),
            email: MailAddress::new("user@example.org", Some("MAICENTA User".into()))
                .expect("mail address"),
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
    fn remote_keys_are_stable_and_portable() {
        assert_eq!(stable_remote_key("INBOX"), stable_remote_key("INBOX"));
        assert_ne!(stable_remote_key("INBOX"), stable_remote_key("Archive"));
        assert!(
            stable_remote_key("Ordner/Unterordner")
                .bytes()
                .all(|value| value.is_ascii_hexdigit())
        );
    }

    #[test]
    fn incremental_selection_catches_up_without_redownloading_known_uids() {
        let uids = [1, 2, 3, 4, 5, 6];
        assert_eq!(select_incremental_uids(&uids, None, &[], 3), [4, 5, 6]);
        assert_eq!(select_incremental_uids(&uids, Some(3), &[], 2), [4, 5]);
        assert_eq!(select_incremental_uids(&uids, Some(5), &[2], 2), [6, 2]);
        assert_eq!(select_incremental_uids(&uids, Some(6), &[4], 2), [4]);
    }

    #[test]
    fn uidnext_checkpoint_selects_delta_only_when_it_is_safe() {
        let checkpoint = RemoteMailboxCheckpoint {
            remote_mailbox: "INBOX".into(),
            uid_validity: 42,
            uid_next: Some(100),
            highest_modseq: Some(900),
            catalog_complete: true,
            force_full_reconcile: false,
        };

        assert_eq!(
            mailbox_sync_plan(Some(&checkpoint), 42, Some(105)),
            MailboxSyncPlan::Delta {
                first_uid: 100,
                last_uid: 104,
            }
        );
        assert_eq!(
            mailbox_sync_plan(Some(&checkpoint), 42, Some(100)),
            MailboxSyncPlan::Delta {
                first_uid: 100,
                last_uid: 99,
            }
        );
        assert_eq!(
            mailbox_sync_plan(Some(&checkpoint), 43, Some(105)),
            MailboxSyncPlan::Full
        );
        assert_eq!(
            mailbox_sync_plan(Some(&checkpoint), 42, Some(99)),
            MailboxSyncPlan::Full
        );
        let mut incomplete = checkpoint.clone();
        incomplete.catalog_complete = false;
        assert_eq!(
            mailbox_sync_plan(Some(&incomplete), 42, Some(105)),
            MailboxSyncPlan::Full
        );
        let mut forced = checkpoint;
        forced.force_full_reconcile = true;
        assert_eq!(
            mailbox_sync_plan(Some(&forced), 42, Some(105)),
            MailboxSyncPlan::Full
        );
    }

    #[test]
    fn vanished_ranges_only_remove_uids_present_in_the_local_catalogue() {
        let known_uids = [2, 4, 9, 25].into_iter().collect::<HashSet<_>>();

        assert_eq!(
            filter_known_vanished_uids(&known_uids, &[1..=3, 8..=20]),
            [2, 9]
        );
    }

    #[test]
    fn catalog_selection_includes_older_unknown_headers_without_body_downloads() {
        let known = [KnownRemoteMessage {
            local_key: "message.6".into(),
            remote_mailbox: "INBOX".into(),
            uid_validity: 42,
            uid: 6,
            needs_catalog_refresh: true,
            needs_body_refresh: false,
        }];
        let work = plan_incremental_mailbox_work("INBOX", 42, &[1, 2, 3, 4, 5, 6], &known, 2);
        assert_eq!(work.catalog_uids, [1, 2, 3, 4, 5, 6]);
        assert!(work.body_uids.is_empty());
        assert_eq!(work.catalog_remaining, 0);
    }

    #[test]
    fn catalog_retries_cannot_starve_an_unknown_old_message() {
        let known = (2..=252)
            .map(|uid| KnownRemoteMessage {
                local_key: format!("message.{uid}"),
                remote_mailbox: "INBOX".into(),
                uid_validity: 42,
                uid,
                needs_catalog_refresh: true,
                needs_body_refresh: false,
            })
            .collect::<Vec<_>>();
        let uids = (1..=252).collect::<Vec<_>>();

        let work = plan_incremental_mailbox_work("INBOX", 42, &uids, &known, 2);

        assert_eq!(work.catalog_uids.len(), 250);
        assert!(work.catalog_uids.contains(&1));
        assert_eq!(work.catalog_remaining, 2);
    }

    #[test]
    fn extracts_downloadable_parts_and_section_paths_from_bodystructure() {
        let response = b"* 1 FETCH (BODYSTRUCTURE (((\"TEXT\" \"PLAIN\" (\"CHARSET\" \"UTF-8\") NIL NIL \"7BIT\" 20 1 NIL NIL NIL NIL)(\"IMAGE\" \"PNG\" (\"NAME\" \"logo.png\") \"<logo@example.org>\" NIL \"BASE64\" 100 NIL (\"INLINE\" (\"FILENAME\" \"logo.png\")) NIL) \"RELATED\" (\"BOUNDARY\" \"related\") NIL NIL)(\"APPLICATION\" \"PDF\" (\"NAME\" \"report.pdf\") NIL NIL \"BASE64\" 400 NIL (\"ATTACHMENT\" (\"FILENAME\" \"report.pdf\")) NIL) \"MIXED\" (\"BOUNDARY\" \"mixed\") NIL NIL))\r\n";
        let (_, parsed) = parse_response(response).expect("BODYSTRUCTURE response");
        let Response::Fetch(_, attributes) = parsed else {
            panic!("expected FETCH response");
        };
        let bodystructure = attributes
            .iter()
            .find_map(|attribute| match attribute {
                AttributeValue::BodyStructure(value) => Some(value),
                _ => None,
            })
            .expect("BODYSTRUCTURE");

        let attachments = remote_attachments_from_bodystructure(bodystructure);

        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].section, "2");
        assert_eq!(attachments[0].file_name, "report.pdf");
        assert_eq!(attachments[0].content_type, "application/pdf");
        assert_eq!(attachments[0].decoded_size_hint, 300);
        assert_eq!(attachments[0].transfer_encoding, "base64");

        let (renderable, complete) = renderable_parts_from_bodystructure(bodystructure);
        assert!(complete);
        assert_eq!(renderable.len(), 2);
        assert_eq!(renderable[0].section, "1.1");
        assert_eq!(renderable[0].role, RenderablePartRole::Text);
        assert_eq!(renderable[1].section, "1.2");
        assert_eq!(renderable[1].role, RenderablePartRole::InlineImage);

        let synthesized = assemble_selective_message(
            b"From: Anna <anna@example.org>\r\nSubject: Selective\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=sender\r\n\r\n",
            7,
            &[
                (renderable[0].clone(), b"Hello selective".to_vec()),
                (renderable[1].clone(), b"aW1hZ2U=".to_vec()),
            ],
            true,
        );
        let rendered = MessageRenderer
            .render(&synthesized, RenderPolicy::default())
            .expect("render selective MIME");
        assert_eq!(rendered.subject.as_deref(), Some("Selective"));
        assert_eq!(rendered.plain_text.as_deref(), Some("Hello selective"));
        assert_eq!(rendered.attachment_count, 0);
        let synthesized = String::from_utf8(synthesized).expect("synthetic MIME");
        assert!(synthesized.contains("Content-ID: <logo@example.org>"));
        assert!(!synthesized.contains("boundary=sender"));
        assert!(!synthesized.contains("report.pdf"));
    }

    #[test]
    fn rejects_unsafe_imap_section_values() {
        assert_eq!(
            validated_section_path("2.1").expect("section"),
            ValidatedSectionPath::Part(vec![2, 1])
        );
        assert_eq!(
            validated_section_path("TEXT").expect("root section"),
            ValidatedSectionPath::Text
        );
        for unsafe_value in ["", "text", "0", "1..2", "1] BODY.PEEK[1", "../2"] {
            assert!(validated_section_path(unsafe_value).is_err());
        }
    }

    #[test]
    fn uses_text_section_for_a_single_part_root_attachment() {
        let response = b"* 1 FETCH (BODYSTRUCTURE (\"APPLICATION\" \"PDF\" (\"NAME\" \"root.pdf\") NIL NIL \"BASE64\" 100 NIL (\"ATTACHMENT\" (\"FILENAME\" \"root.pdf\")) NIL))\r\n";
        let (_, parsed) = parse_response(response).expect("BODYSTRUCTURE response");
        let Response::Fetch(_, attributes) = parsed else {
            panic!("expected FETCH response");
        };
        let bodystructure = attributes
            .iter()
            .find_map(|attribute| match attribute {
                AttributeValue::BodyStructure(value) => Some(value),
                _ => None,
            })
            .expect("BODYSTRUCTURE");

        let attachments = remote_attachments_from_bodystructure(bodystructure);
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].section, "TEXT");
    }

    #[test]
    fn standard_mailboxes_are_synchronized_before_custom_folders() {
        assert!(
            mailbox_sync_priority(MailboxRole::Inbox) < mailbox_sync_priority(MailboxRole::Sent)
        );
        assert!(
            mailbox_sync_priority(MailboxRole::Junk) < mailbox_sync_priority(MailboxRole::Custom)
        );
    }

    #[test]
    fn recognizes_standard_roles_below_an_inbox_namespace() {
        assert_eq!(mailbox_role("INBOX.Drafts", &[]), MailboxRole::Drafts);
        assert_eq!(mailbox_role("INBOX/Sent", &[]), MailboxRole::Sent);
        assert_eq!(mailbox_role(r"INBOX\Trash", &[]), MailboxRole::Trash);
        assert_eq!(mailbox_role("INBOX.spambucket", &[]), MailboxRole::Junk);
        assert_eq!(mailbox_role("INBOX.Templates", &[]), MailboxRole::Custom);
    }

    #[test]
    fn synchronization_order_keeps_every_selectable_mailbox() {
        let mailboxes = (0..20)
            .map(|index| RemoteMailbox {
                remote_name: format!("Folder {index:02}"),
                role: MailboxRole::Custom,
            })
            .chain([RemoteMailbox {
                remote_name: "INBOX".into(),
                role: MailboxRole::Inbox,
            }])
            .collect::<Vec<_>>();

        let ordered = ordered_mailboxes_for_sync(mailboxes);

        assert_eq!(ordered.len(), 21);
        assert_eq!(ordered[0].role, MailboxRole::Inbox);
        assert_eq!(ordered[20].remote_name, "Folder 19");
    }

    #[test]
    fn moving_never_falls_back_to_global_expunge() {
        assert_eq!(safe_move_strategy(true, false), MoveStrategy::Move);
        assert_eq!(
            safe_move_strategy(false, true),
            MoveStrategy::CopyUidExpunge
        );
        assert_eq!(safe_move_strategy(false, false), MoveStrategy::Unavailable);
    }

    #[test]
    fn builds_multipart_html_mail_with_attachment_and_importance() {
        let message = build_message(
            &mail_account(),
            &OutgoingMessage {
                to: vec!["Anna <anna@example.org>".into()],
                cc: vec!["copy@example.org".into()],
                bcc: vec!["hidden@example.org".into()],
                subject: "Formatted message".into(),
                plain_text: "Hello world".into(),
                sanitized_html: "<p>Hello <strong>world</strong></p>".into(),
                attachments: vec![OutgoingAttachment {
                    file_name: "notes.txt".into(),
                    content_type: "text/plain".into(),
                    body: b"attachment body".to_vec(),
                }],
                high_importance: true,
            },
        )
        .expect("build message");
        let formatted = String::from_utf8(message.formatted()).expect("UTF-8 MIME message");

        assert!(formatted.contains("Content-Type: multipart/mixed"));
        assert!(formatted.contains("Content-Type: multipart/alternative"));
        assert!(formatted.contains("Content-Type: text/plain; charset=utf-8"));
        assert!(formatted.contains("Content-Type: text/html; charset=utf-8"));
        assert!(formatted.contains("filename=\"notes.txt\""));
        assert!(formatted.contains("Importance: high"));
        assert!(formatted.contains("X-Priority: 1"));
        assert!(!formatted.contains("Bcc:"));
    }

    #[test]
    fn builds_retry_stable_draft_mime_without_requiring_a_recipient() {
        let local_key = "local.work.draft";
        let message_id = draft_message_id(local_key);
        assert_eq!(message_id, draft_message_id(local_key));
        let message = build_draft_message(
            &mail_account(),
            &OutgoingMessage {
                to: Vec::new(),
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: "Unvollständiger Entwurf".into(),
                plain_text: "Noch ohne Empfänger".into(),
                sanitized_html: "<p>Noch ohne Empfänger</p>".into(),
                attachments: Vec::new(),
                high_importance: false,
            },
            &message_id,
        )
        .expect("draft MIME");
        let formatted = String::from_utf8(message.formatted()).expect("UTF-8 draft MIME");

        assert!(formatted.contains(&format!("Message-ID: {message_id}")));
        assert!(formatted.contains("Subject:"));
        assert!(!formatted.contains("\r\nTo:"));
        assert!(!formatted.contains("\r\nCc:"));
        assert!(!formatted.contains("\r\nBcc:"));
    }

    #[test]
    fn retains_bcc_in_an_imap_draft() {
        let message = build_draft_message(
            &mail_account(),
            &OutgoingMessage {
                to: Vec::new(),
                cc: Vec::new(),
                bcc: vec!["hidden@example.org".into()],
                subject: "Privater Entwurf".into(),
                plain_text: "Entwurfstext".into(),
                sanitized_html: "<p>Entwurfstext</p>".into(),
                attachments: Vec::new(),
                high_importance: false,
            },
            &draft_message_id("local.private.draft"),
        )
        .expect("draft MIME");
        let formatted = String::from_utf8(message.formatted()).expect("UTF-8 draft MIME");

        assert!(formatted.contains("Bcc: hidden@example.org"));
    }
}
