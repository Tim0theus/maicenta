//! Microsoft Graph mail connector.
//!
//! Exchange Online tenants increasingly disable IMAP and SMTP AUTH. This
//! connector reaches the same mailbox through the Graph REST API and produces
//! the provider-neutral values the rest of MAICENTA already understands:
//! mailboxes with roles, RFC 5322 bytes for the safe renderer, portable flags,
//! and opaque stable message identities.
//!
//! Design notes:
//!
//! - Every request carries `Prefer: IdType="ImmutableId"` so message IDs stay
//!   stable when a message is moved between folders.
//! - Incremental synchronization uses per-folder delta queries. An unfinished
//!   initial round stores its `nextLink`, a finished round its `deltaLink`;
//!   both are opaque cursors for the caller.
//! - Displayable bodies are requested as HTML and wrapped into a synthetic
//!   MIME message together with bounded inline images, so the existing
//!   sanitizer and `cid:` resolution apply unchanged. Normal attachments are
//!   catalogued by ID and downloaded on demand.
//! - Outgoing drafts and messages reuse the exact MIME structure of the
//!   IMAP/SMTP connector and are submitted as base64 MIME.

use std::{
    collections::{HashMap, HashSet},
    fmt::Write as _,
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use maicenta_domain::{MailAccount, MailboxRole, MessageFlag};
pub use maicenta_mail_connector::ConnectorError;
use maicenta_mail_connector::{
    AppliedMutation, FailedMutation, MutationReport, OutgoingMessage, RemoteFlagUpdate,
    render_outgoing_message,
};
use serde_json::{Value, json};

const GRAPH_BASE: &str = "https://graph.microsoft.com/v1.0";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(120);
const SYNC_OPERATION_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_RETRY_AFTER: Duration = Duration::from_secs(10);
const DELTA_PAGE_SIZE: usize = 100;
const MAX_DELTA_PAGES_PER_MAILBOX: usize = 3;
// One pass is deliberately short so the client can show progress between
// passes and continue automatically while work remains.
const MAX_DELTA_PAGES_PER_PASS: usize = 12;
const MAX_BODY_DOWNLOADS_PER_PASS: usize = 25;
// The very first pass of a new account returns as soon as the folder list
// and the first inbox page are known; bodies follow in the next pass.
const INITIAL_PASS_DELTA_PAGES: usize = 3;
const MAX_FOLDERS: usize = 200;
const MAX_FOLDER_DEPTH: usize = 4;
const MAX_BODY_BYTES: usize = 5 * 1024 * 1024;
const MAX_INLINE_PARTS: usize = 20;
const MAX_INLINE_PART_BYTES: u64 = 3 * 1024 * 1024;
const MAX_INLINE_TOTAL_BYTES: u64 = 7 * 1024 * 1024;
const MAX_ATTACHMENT_LIST: usize = 100;
const MAX_GRAPH_ID_LENGTH: usize = 1_024;

const MESSAGE_SELECT: &str = "id,parentFolderId,subject,from,toRecipients,ccRecipients,\
receivedDateTime,sentDateTime,isRead,isDraft,flag,hasAttachments,importance,internetMessageId";
const MESSAGE_BODY_SELECT: &str = "id,parentFolderId,subject,from,toRecipients,ccRecipients,\
bccRecipients,receivedDateTime,sentDateTime,isRead,isDraft,flag,hasAttachments,importance,\
internetMessageId,body";

/// Well-known folder names Graph resolves independently of localization.
const WELL_KNOWN_FOLDERS: [(&str, MailboxRole); 6] = [
    ("inbox", MailboxRole::Inbox),
    ("drafts", MailboxRole::Drafts),
    ("sentitems", MailboxRole::Sent),
    ("archive", MailboxRole::Archive),
    ("deleteditems", MailboxRole::Trash),
    ("junkemail", MailboxRole::Junk),
];

/// One Graph mail folder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphMailbox {
    /// Stable folder ID used as the remote mailbox identity.
    pub folder_id: String,
    pub display_name: String,
    pub role: MailboxRole,
    /// Server-side message count, used to estimate remaining catalogue work.
    pub total_item_count: Option<u64>,
}

/// Metadata for one non-inline attachment that stays on the server until the
/// user requests it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphAttachmentPart {
    pub attachment_id: String,
    pub file_name: String,
    pub content_type: String,
    pub size_bytes: u64,
}

/// One message catalogued or downloaded through Graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphMessage {
    pub folder_id: String,
    pub mailbox_role: MailboxRole,
    /// Immutable Graph message ID.
    pub id: String,
    pub flags: Vec<MessageFlag>,
    /// Synthetic RFC 5322 message for the safe renderer.
    pub renderable_message: Vec<u8>,
    pub attachments: Vec<GraphAttachmentPart>,
    pub catalog_complete: bool,
    pub body_requested: bool,
    pub body_complete: bool,
}

/// Locally cached identity used to skip unchanged messages.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KnownGraphMessage {
    pub local_key: String,
    pub folder_id: String,
    pub id: String,
    pub needs_catalog_refresh: bool,
    pub needs_body_refresh: bool,
}

/// Persisted per-folder delta state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphMailboxCheckpoint {
    pub folder_id: String,
    /// `deltaLink` of a finished round or `nextLink` of an unfinished one.
    pub delta_cursor: Option<String>,
    pub catalog_complete: bool,
}

/// Delta outcome for one folder during this pass.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphMailboxState {
    pub folder_id: String,
    pub delta_cursor: Option<String>,
    /// Message IDs the server reported as removed from this folder.
    pub removed_ids: Vec<String>,
    pub catalog_complete: bool,
    /// Lower-bound estimate of catalogue entries still to be fetched.
    pub catalog_remaining: usize,
}

/// Result of one bounded synchronization pass.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphSyncResult {
    pub mailboxes: Vec<GraphMailbox>,
    pub messages: Vec<GraphMessage>,
    pub flag_updates: Vec<RemoteFlagUpdate>,
    pub mailbox_states: Vec<GraphMailboxState>,
}

/// One compacted local change to apply to a Graph message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphMutation {
    pub local_key: String,
    pub message_id: String,
    pub target_folder_id: Option<String>,
    pub seen: bool,
    pub flagged: bool,
}

/// Stable identity of one server draft.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphDraftIdentity {
    pub folder_id: String,
    pub message_id: String,
}

/// One durable local draft action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphDraftOperation {
    pub local_key: String,
    pub target_folder_id: String,
    pub previous_remote: Option<GraphDraftIdentity>,
    /// `None` removes the previous server draft without uploading a successor.
    pub message: Option<OutgoingMessage>,
}

/// One successfully applied draft action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppliedGraphDraftOperation {
    pub local_key: String,
    pub uploaded_remote: Option<GraphDraftIdentity>,
}

/// Per-draft outcome of applying the persistent draft queue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphDraftOperationReport {
    pub applied: Vec<AppliedGraphDraftOperation>,
    pub failed: Vec<FailedMutation>,
}

/// Verifies that the access token can read the mailbox.
///
/// # Errors
///
/// Returns an authentication error for a rejected token, a connection error
/// for network failures, or a protocol error for an unexpected response.
pub async fn test_account(access_token: &str) -> Result<(), ConnectorError> {
    let client = GraphClient::new(access_token)?;
    tokio::time::timeout(OPERATION_TIMEOUT, async {
        client
            .get_json(&format!("{GRAPH_BASE}/me/mailFolders/inbox?$select=id"))
            .await
            .map(|_| ())
    })
    .await
    .map_err(|_| ConnectorError::Connection("Microsoft Graph connection timed out".into()))?
}

/// Runs one bounded incremental synchronization pass.
///
/// Folders are discovered on every pass. Each folder continues its delta
/// round from the stored cursor; up to `message_limit_per_mailbox` displayable
/// bodies per folder are downloaded, newest first on the initial round.
///
/// # Errors
///
/// Returns a categorized authentication, connection, or protocol error.
pub async fn synchronize_mailboxes(
    access_token: &str,
    known_messages: &[KnownGraphMessage],
    checkpoints: &[GraphMailboxCheckpoint],
    message_limit_per_mailbox: usize,
) -> Result<GraphSyncResult, ConnectorError> {
    let client = GraphClient::new(access_token)?;
    tokio::time::timeout(
        SYNC_OPERATION_TIMEOUT,
        synchronize_inner(
            &client,
            known_messages,
            checkpoints,
            message_limit_per_mailbox,
        ),
    )
    .await
    .map_err(|_| ConnectorError::Connection("Microsoft Graph synchronization timed out".into()))?
}

/// Downloads the displayable content of one catalogued message.
///
/// # Errors
///
/// Returns [`ConnectorError::RemoteMessageMissing`] when the message no longer
/// exists, or a categorized authentication, connection, or protocol error.
pub async fn download_message_content(
    access_token: &str,
    folder_id: &str,
    message_id: &str,
    mailbox_role: MailboxRole,
) -> Result<GraphMessage, ConnectorError> {
    validate_graph_id(message_id, "message")?;
    validate_graph_id(folder_id, "folder")?;
    let client = GraphClient::new(access_token)?;
    tokio::time::timeout(OPERATION_TIMEOUT, async {
        let item = client
            .get_json(&format!(
                "{GRAPH_BASE}/me/messages/{}?$select={MESSAGE_BODY_SELECT}",
                escape_path(message_id)
            ))
            .await?;
        build_body_message(&client, &item, folder_id, mailbox_role).await
    })
    .await
    .map_err(|_| ConnectorError::Connection("message download timed out".into()))?
}

/// Downloads the raw bytes of one non-inline attachment.
///
/// # Errors
///
/// Returns [`ConnectorError::RemoteMessageMissing`] when the message or
/// attachment no longer exists, a protocol error when the attachment exceeds
/// `maximum_bytes`, or a categorized authentication or connection error.
pub async fn download_attachment(
    access_token: &str,
    message_id: &str,
    attachment_id: &str,
    maximum_bytes: usize,
) -> Result<Vec<u8>, ConnectorError> {
    validate_graph_id(message_id, "message")?;
    validate_graph_id(attachment_id, "attachment")?;
    let client = GraphClient::new(access_token)?;
    tokio::time::timeout(OPERATION_TIMEOUT, async {
        client
            .get_bytes(
                &format!(
                    "{GRAPH_BASE}/me/messages/{}/attachments/{}/$value",
                    escape_path(message_id),
                    escape_path(attachment_id)
                ),
                maximum_bytes,
            )
            .await
    })
    .await
    .map_err(|_| ConnectorError::Connection("attachment download timed out".into()))?
}

/// Applies read/flag changes and folder moves for queued local mutations.
///
/// Each mutation is applied independently; failures are reported per message
/// so the caller can retain them for a later retry.
///
/// # Errors
///
/// Returns an error only when the client itself cannot be created.
pub async fn apply_mailbox_mutations(
    access_token: &str,
    mutations: &[GraphMutation],
) -> Result<MutationReport, ConnectorError> {
    let client = GraphClient::new(access_token)?;
    let mut report = MutationReport {
        applied: Vec::new(),
        failed: Vec::new(),
    };
    for mutation in mutations {
        let result = tokio::time::timeout(OPERATION_TIMEOUT, apply_mutation(&client, mutation))
            .await
            .map_err(|_| ConnectorError::Connection("mutation timed out".into()))
            .and_then(|result| result);
        match result {
            Ok(moved) => report.applied.push(AppliedMutation {
                local_key: mutation.local_key.clone(),
                moved,
            }),
            Err(ConnectorError::RemoteMessageMissing(_)) => {
                // The server copy is gone; a later delta removal cleans the
                // local row and its queued mutation.
                report.failed.push(FailedMutation {
                    local_key: mutation.local_key.clone(),
                    error: "message no longer exists on the server".into(),
                });
            }
            Err(error) => report.failed.push(FailedMutation {
                local_key: mutation.local_key.clone(),
                error: error.to_string(),
            }),
        }
    }
    Ok(report)
}

/// Uploads, replaces, or removes server drafts for queued local operations.
///
/// # Errors
///
/// Returns an error only when the client itself cannot be created.
pub async fn apply_draft_operations(
    access_token: &str,
    account: &MailAccount,
    operations: &[GraphDraftOperation],
) -> Result<GraphDraftOperationReport, ConnectorError> {
    let client = GraphClient::new(access_token)?;
    let mut report = GraphDraftOperationReport {
        applied: Vec::new(),
        failed: Vec::new(),
    };
    for operation in operations {
        let result = tokio::time::timeout(
            OPERATION_TIMEOUT,
            apply_draft_operation(&client, account, operation),
        )
        .await
        .map_err(|_| ConnectorError::Connection("draft operation timed out".into()))
        .and_then(|result| result);
        match result {
            Ok(uploaded_remote) => report.applied.push(AppliedGraphDraftOperation {
                local_key: operation.local_key.clone(),
                uploaded_remote,
            }),
            Err(error) => report.failed.push(FailedMutation {
                local_key: operation.local_key.clone(),
                error: error.to_string(),
            }),
        }
    }
    Ok(report)
}

/// Sends one message through Graph. Exchange stores the sent copy itself.
///
/// # Errors
///
/// Returns a validation error for an invalid message or a categorized
/// authentication, connection, or protocol error.
pub async fn send_message(
    access_token: &str,
    account: &MailAccount,
    outgoing: &OutgoingMessage,
) -> Result<(), ConnectorError> {
    let mime = render_outgoing_message(account, outgoing, None, true)?;
    let client = GraphClient::new(access_token)?;
    tokio::time::timeout(OPERATION_TIMEOUT, async {
        client
            .post_mime(&format!("{GRAPH_BASE}/me/sendMail"), &mime)
            .await
            .map(|_| ())
    })
    .await
    .map_err(|_| ConnectorError::Connection("Microsoft Graph submission timed out".into()))?
}

struct GraphClient {
    http: reqwest::Client,
    access_token: String,
}

impl GraphClient {
    fn new(access_token: &str) -> Result<Self, ConnectorError> {
        if access_token.is_empty() || access_token.chars().any(char::is_control) {
            return Err(ConnectorError::Authentication(
                "an OAuth access token is required for Microsoft Graph".into(),
            ));
        }
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .https_only(true)
            .build()
            .map_err(|error| ConnectorError::Connection(error.to_string()))?;
        Ok(Self {
            http,
            access_token: access_token.to_owned(),
        })
    }

    fn request(&self, method: reqwest::Method, url: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, url)
            .bearer_auth(&self.access_token)
            .header("Prefer", "IdType=\"ImmutableId\"")
            .header("Accept", "application/json")
    }

    async fn send(
        &self,
        build: impl Fn() -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, ConnectorError> {
        let mut attempt = 0;
        loop {
            let response = build()
                .send()
                .await
                .map_err(|error| ConnectorError::Connection(redact_error(error)))?;
            let status = response.status();
            if attempt == 0 && (status.as_u16() == 429 || status.as_u16() == 503) {
                let retry_after = response
                    .headers()
                    .get("Retry-After")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<u64>().ok())
                    .map_or(Duration::from_secs(2), Duration::from_secs)
                    .min(MAX_RETRY_AFTER);
                tokio::time::sleep(retry_after).await;
                attempt += 1;
                continue;
            }
            if status.is_success() {
                return Ok(response);
            }
            return Err(graph_error(response).await);
        }
    }

    async fn get_json(&self, url: &str) -> Result<Value, ConnectorError> {
        validate_graph_url(url)?;
        let response = self
            .send(|| self.request(reqwest::Method::GET, url))
            .await?;
        response
            .json::<Value>()
            .await
            .map_err(|error| ConnectorError::Protocol(redact_error(error)))
    }

    async fn get_json_with_prefer(&self, url: &str, prefer: &str) -> Result<Value, ConnectorError> {
        validate_graph_url(url)?;
        let response = self
            .send(|| {
                self.request(reqwest::Method::GET, url)
                    .header("Prefer", format!("IdType=\"ImmutableId\", {prefer}"))
            })
            .await?;
        response
            .json::<Value>()
            .await
            .map_err(|error| ConnectorError::Protocol(redact_error(error)))
    }

    async fn get_bytes(&self, url: &str, maximum_bytes: usize) -> Result<Vec<u8>, ConnectorError> {
        validate_graph_url(url)?;
        let response = self
            .send(|| {
                self.request(reqwest::Method::GET, url)
                    .header("Accept", "*/*")
            })
            .await?;
        if response
            .content_length()
            .is_some_and(|length| length > maximum_bytes as u64)
        {
            return Err(ConnectorError::Protocol(
                "attachment exceeds the configured download limit".into(),
            ));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| ConnectorError::Connection(redact_error(error)))?;
        if bytes.len() > maximum_bytes {
            return Err(ConnectorError::Protocol(
                "attachment exceeds the configured download limit".into(),
            ));
        }
        Ok(bytes.to_vec())
    }

    async fn post_json(&self, url: &str, body: &Value) -> Result<Value, ConnectorError> {
        validate_graph_url(url)?;
        let response = self
            .send(|| self.request(reqwest::Method::POST, url).json(body))
            .await?;
        json_or_null(response).await
    }

    async fn patch_json(&self, url: &str, body: &Value) -> Result<Value, ConnectorError> {
        validate_graph_url(url)?;
        let response = self
            .send(|| self.request(reqwest::Method::PATCH, url).json(body))
            .await?;
        json_or_null(response).await
    }

    async fn post_mime(&self, url: &str, mime: &[u8]) -> Result<Value, ConnectorError> {
        validate_graph_url(url)?;
        let encoded = BASE64.encode(mime);
        let response = self
            .send(|| {
                self.request(reqwest::Method::POST, url)
                    .header("Content-Type", "text/plain")
                    .body(encoded.clone())
            })
            .await?;
        json_or_null(response).await
    }

    /// Deletes a resource; a missing resource is treated as already deleted.
    async fn delete(&self, url: &str) -> Result<(), ConnectorError> {
        validate_graph_url(url)?;
        match self
            .send(|| self.request(reqwest::Method::DELETE, url))
            .await
        {
            Ok(_) | Err(ConnectorError::RemoteMessageMissing(_)) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

async fn json_or_null(response: reqwest::Response) -> Result<Value, ConnectorError> {
    if response.status() == reqwest::StatusCode::NO_CONTENT
        || response.status() == reqwest::StatusCode::ACCEPTED
        || response.content_length() == Some(0)
    {
        return Ok(Value::Null);
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| ConnectorError::Connection(redact_error(error)))?;
    if bytes.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&bytes).map_err(|error| ConnectorError::Protocol(error.to_string()))
}

async fn graph_error(response: reqwest::Response) -> ConnectorError {
    let status = response.status();
    let detail = response
        .json::<Value>()
        .await
        .ok()
        .and_then(|payload| {
            payload
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(|message| message.chars().take(300).collect::<String>())
        })
        .unwrap_or_else(|| format!("Microsoft Graph returned HTTP {}", status.as_u16()));
    match status.as_u16() {
        401 => ConnectorError::Authentication(format!(
            "Microsoft Graph rejected the access token: {detail}"
        )),
        403 => ConnectorError::Authentication(format!(
            "Microsoft Graph denied access; check the Mail.ReadWrite and Mail.Send permissions: {detail}"
        )),
        404 => ConnectorError::RemoteMessageMissing(detail),
        429 | 500..=599 => ConnectorError::Connection(detail),
        _ => ConnectorError::Protocol(detail),
    }
}

/// Removes URLs and query strings from transport errors so tokens or IDs in a
/// request never reach a log or the interface.
fn redact_error(error: reqwest::Error) -> String {
    let mut redacted = error.without_url().to_string();
    if let Some(index) = redacted.find('?') {
        redacted.truncate(index);
    }
    redacted
}

fn validate_graph_url(url: &str) -> Result<(), ConnectorError> {
    if url.starts_with(GRAPH_BASE) {
        Ok(())
    } else {
        Err(ConnectorError::Protocol(
            "refusing to follow a link outside Microsoft Graph".into(),
        ))
    }
}

fn validate_graph_id(value: &str, description: &str) -> Result<(), ConnectorError> {
    let valid = !value.is_empty()
        && value.len() <= MAX_GRAPH_ID_LENGTH
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'=' | b'+' | b'/')
        });
    if valid {
        Ok(())
    } else {
        Err(ConnectorError::InvalidConfiguration(format!(
            "invalid Microsoft Graph {description} identifier"
        )))
    }
}

/// Percent-encodes the few characters a Graph ID may contain that are not
/// safe inside a URL path segment.
fn escape_path(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'+' => escaped.push_str("%2B"),
            b'/' => escaped.push_str("%2F"),
            b'=' => escaped.push_str("%3D"),
            other => escaped.push(char::from(other)),
        }
    }
    escaped
}

// ---------------------------------------------------------------------------
// Synchronization
// ---------------------------------------------------------------------------

struct FolderBudget {
    delta_pages: usize,
    body_downloads: usize,
}

async fn synchronize_inner(
    client: &GraphClient,
    known_messages: &[KnownGraphMessage],
    checkpoints: &[GraphMailboxCheckpoint],
    message_limit_per_mailbox: usize,
) -> Result<GraphSyncResult, ConnectorError> {
    let mailboxes = discover_folders(client).await?;
    let known_by_id = known_messages
        .iter()
        .map(|known| (known.id.as_str(), known))
        .collect::<HashMap<_, _>>();
    let checkpoints_by_folder = checkpoints
        .iter()
        .map(|checkpoint| (checkpoint.folder_id.as_str(), checkpoint))
        .collect::<HashMap<_, _>>();
    let mut known_per_folder: HashMap<&str, usize> = HashMap::new();
    for known in known_messages {
        *known_per_folder
            .entry(known.folder_id.as_str())
            .or_default() += 1;
    }
    let initial_pass = checkpoints.is_empty();
    let mut budget = if initial_pass {
        FolderBudget {
            delta_pages: INITIAL_PASS_DELTA_PAGES,
            body_downloads: 0,
        }
    } else {
        FolderBudget {
            delta_pages: MAX_DELTA_PAGES_PER_PASS,
            body_downloads: MAX_BODY_DOWNLOADS_PER_PASS,
        }
    };
    let mut result = GraphSyncResult {
        mailboxes: mailboxes.clone(),
        messages: Vec::new(),
        flag_updates: Vec::new(),
        mailbox_states: Vec::new(),
    };
    for mailbox in &mailboxes {
        let checkpoint = checkpoints_by_folder
            .get(mailbox.folder_id.as_str())
            .copied();
        let known_in_folder = known_per_folder
            .get(mailbox.folder_id.as_str())
            .copied()
            .unwrap_or(0);
        if budget.delta_pages == 0 {
            // Leave the folder untouched; its stored cursor continues later.
            // Reporting the remaining estimate keeps the client continuing.
            let catalog_complete = checkpoint.is_some_and(|checkpoint| checkpoint.catalog_complete);
            result.mailbox_states.push(GraphMailboxState {
                folder_id: mailbox.folder_id.clone(),
                delta_cursor: checkpoint.and_then(|checkpoint| checkpoint.delta_cursor.clone()),
                removed_ids: Vec::new(),
                catalog_complete,
                catalog_remaining: remaining_estimate(
                    mailbox,
                    catalog_complete,
                    known_in_folder,
                    0,
                    0,
                ),
            });
            continue;
        }
        let folder_result = synchronize_folder(
            client,
            mailbox,
            checkpoint,
            &known_by_id,
            known_in_folder,
            message_limit_per_mailbox,
            &mut budget,
        )
        .await?;
        result.messages.extend(folder_result.messages);
        result.flag_updates.extend(folder_result.flag_updates);
        result.mailbox_states.push(folder_result.state);
    }
    Ok(result)
}

struct FolderSyncResult {
    messages: Vec<GraphMessage>,
    flag_updates: Vec<RemoteFlagUpdate>,
    state: GraphMailboxState,
}

/// Lower-bound estimate of work left in one folder: messages the server
/// reports but the catalogue does not know yet, plus body downloads that were
/// deferred by the pass budget. Never zero while the catalogue is incomplete,
/// so the client keeps continuing.
fn remaining_estimate(
    mailbox: &GraphMailbox,
    catalog_complete: bool,
    known_in_folder: usize,
    catalogued_this_pass: usize,
    deferred_bodies: usize,
) -> usize {
    let catalog = if catalog_complete {
        0
    } else {
        let server_total = usize::try_from(mailbox.total_item_count.unwrap_or(0)).unwrap_or(0);
        server_total
            .saturating_sub(known_in_folder)
            .saturating_sub(catalogued_this_pass)
            .max(1)
    };
    catalog + deferred_bodies
}

#[allow(clippy::too_many_lines)]
async fn synchronize_folder(
    client: &GraphClient,
    mailbox: &GraphMailbox,
    checkpoint: Option<&GraphMailboxCheckpoint>,
    known_by_id: &HashMap<&str, &KnownGraphMessage>,
    known_in_folder: usize,
    message_limit_per_mailbox: usize,
    budget: &mut FolderBudget,
) -> Result<FolderSyncResult, ConnectorError> {
    let mut url = checkpoint
        .and_then(|checkpoint| checkpoint.delta_cursor.clone())
        .unwrap_or_else(|| {
            format!(
                "{GRAPH_BASE}/me/mailFolders/{}/messages/delta?$select={MESSAGE_SELECT}",
                escape_path(&mailbox.folder_id)
            )
        });
    let mut messages = Vec::new();
    let mut flag_updates = Vec::new();
    let mut removed_ids = Vec::new();
    let mut body_candidates: Vec<(String, String)> = Vec::new();
    let mut seen_in_pass = HashSet::new();
    let mut catalog_complete = false;
    let mut pages = 0;
    let mut next_cursor = Some(url.clone());

    while pages < MAX_DELTA_PAGES_PER_MAILBOX && budget.delta_pages > 0 {
        let page = client
            .get_json_with_prefer(&url, &format!("odata.maxpagesize={DELTA_PAGE_SIZE}"))
            .await?;
        pages += 1;
        budget.delta_pages -= 1;
        for item in page
            .get("value")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(id) = item.get("id").and_then(Value::as_str) else {
                continue;
            };
            if validate_graph_id(id, "message").is_err() {
                continue;
            }
            if item.get("@removed").is_some() {
                removed_ids.push(id.to_owned());
                continue;
            }
            seen_in_pass.insert(id.to_owned());
            let flags = message_flags(item);
            match known_by_id.get(id) {
                Some(known)
                    if known.folder_id == mailbox.folder_id && !known.needs_catalog_refresh =>
                {
                    flag_updates.push(RemoteFlagUpdate {
                        local_key: known.local_key.clone(),
                        flags,
                    });
                    if known.needs_body_refresh {
                        body_candidates
                            .push((id.to_owned(), received_at(item).unwrap_or_default()));
                    }
                }
                _ => {
                    messages.push(GraphMessage {
                        folder_id: mailbox.folder_id.clone(),
                        mailbox_role: mailbox.role,
                        id: id.to_owned(),
                        flags,
                        renderable_message: build_renderable_mime(item, None, &[]),
                        attachments: Vec::new(),
                        catalog_complete: true,
                        body_requested: false,
                        body_complete: false,
                    });
                    body_candidates.push((id.to_owned(), received_at(item).unwrap_or_default()));
                }
            }
        }
        if let Some(delta_link) = page.get("@odata.deltaLink").and_then(Value::as_str) {
            validate_graph_url(delta_link)?;
            next_cursor = Some(delta_link.to_owned());
            catalog_complete = true;
            break;
        }
        if let Some(next_link) = page.get("@odata.nextLink").and_then(Value::as_str) {
            validate_graph_url(next_link)?;
            next_link.clone_into(&mut url);
            next_cursor = Some(url.clone());
        } else {
            // A round without continuation is treated as complete; the next
            // pass starts a fresh round for this folder.
            next_cursor = None;
            catalog_complete = true;
            break;
        }
    }
    if !catalog_complete && pages == 0 {
        // No budget was available; keep the stored cursor unchanged.
        next_cursor = checkpoint.and_then(|checkpoint| checkpoint.delta_cursor.clone());
        catalog_complete = checkpoint.is_some_and(|checkpoint| checkpoint.catalog_complete);
    }

    let catalogued_this_pass = messages.len();

    // Bodies: newest first. Delta rounds deliver messages in server order and
    // only report changed items, so ask for the newest messages explicitly on
    // every pass; those without a cached body are filled progressively.
    let mut selected = Vec::new();
    if message_limit_per_mailbox > 0 && budget.body_downloads > 0 {
        let newest = client
            .get_json(&format!(
                "{GRAPH_BASE}/me/mailFolders/{}/messages?$orderby=receivedDateTime desc&$top={}&$select={MESSAGE_SELECT}",
                escape_path(&mailbox.folder_id),
                message_limit_per_mailbox.min(50)
            ))
            .await?;
        for item in newest
            .get("value")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(id) = item.get("id").and_then(Value::as_str) {
                if validate_graph_id(id, "message").is_ok()
                    && known_by_id
                        .get(id)
                        .is_none_or(|known| known.needs_body_refresh || known.needs_catalog_refresh)
                {
                    selected.push(id.to_owned());
                }
            }
        }
    }
    body_candidates.sort_by(|left, right| right.1.cmp(&left.1));
    for (id, _) in body_candidates {
        if !selected.contains(&id) {
            selected.push(id);
        }
    }
    let wanted_bodies = selected.len();
    selected.truncate(message_limit_per_mailbox.min(budget.body_downloads));
    let deferred_bodies = wanted_bodies.saturating_sub(selected.len());
    for id in selected {
        if budget.body_downloads == 0 {
            break;
        }
        budget.body_downloads -= 1;
        let item = match client
            .get_json_with_prefer(
                &format!(
                    "{GRAPH_BASE}/me/messages/{}?$select={MESSAGE_BODY_SELECT}",
                    escape_path(&id)
                ),
                "outlook.body-content-type=\"html\"",
            )
            .await
        {
            Ok(item) => item,
            Err(ConnectorError::RemoteMessageMissing(_)) => {
                removed_ids.push(id);
                continue;
            }
            Err(error) => return Err(error),
        };
        let message = build_body_message(client, &item, &mailbox.folder_id, mailbox.role).await?;
        messages.retain(|catalogued| catalogued.id != message.id);
        messages.push(message);
    }

    Ok(FolderSyncResult {
        messages,
        flag_updates,
        state: GraphMailboxState {
            folder_id: mailbox.folder_id.clone(),
            delta_cursor: next_cursor,
            removed_ids,
            catalog_complete,
            catalog_remaining: remaining_estimate(
                mailbox,
                catalog_complete,
                known_in_folder,
                catalogued_this_pass,
                deferred_bodies,
            ),
        },
    })
}

async fn discover_folders(client: &GraphClient) -> Result<Vec<GraphMailbox>, ConnectorError> {
    let mut roles = HashMap::new();
    for (well_known, role) in WELL_KNOWN_FOLDERS {
        match client
            .get_json(&format!(
                "{GRAPH_BASE}/me/mailFolders/{well_known}?$select=id"
            ))
            .await
        {
            Ok(folder) => {
                if let Some(id) = folder.get("id").and_then(Value::as_str) {
                    roles.insert(id.to_owned(), role);
                }
            }
            Err(ConnectorError::RemoteMessageMissing(_)) => {}
            Err(error) => return Err(error),
        }
    }
    let mut folders = Vec::new();
    collect_folders(
        client,
        &format!(
            "{GRAPH_BASE}/me/mailFolders?$top=100&$select=id,displayName,childFolderCount,totalItemCount"
        ),
        &roles,
        None,
        0,
        &mut folders,
    )
    .await?;
    folders.sort_by_key(|folder| {
        (
            match folder.role {
                MailboxRole::Inbox => 0,
                MailboxRole::Drafts => 1,
                MailboxRole::Sent => 2,
                MailboxRole::Archive => 3,
                MailboxRole::Trash => 4,
                MailboxRole::Junk => 5,
                MailboxRole::Custom => 6,
            },
            folder.display_name.to_lowercase(),
        )
    });
    Ok(folders)
}

async fn collect_folders(
    client: &GraphClient,
    url: &str,
    roles: &HashMap<String, MailboxRole>,
    parent_path: Option<&str>,
    depth: usize,
    folders: &mut Vec<GraphMailbox>,
) -> Result<(), ConnectorError> {
    let mut url = url.to_owned();
    loop {
        if folders.len() >= MAX_FOLDERS {
            return Ok(());
        }
        let page = client.get_json(&url).await?;
        let mut children = Vec::new();
        for item in page
            .get("value")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(id) = item.get("id").and_then(Value::as_str) else {
                continue;
            };
            if validate_graph_id(id, "folder").is_err() {
                continue;
            }
            let name = item
                .get("displayName")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .unwrap_or("Folder");
            let display_name = match parent_path {
                Some(parent) => format!("{parent}/{name}"),
                None => name.to_owned(),
            };
            let display_name = display_name.chars().take(255).collect::<String>();
            let role = roles.get(id).copied().unwrap_or(MailboxRole::Custom);
            if item
                .get("childFolderCount")
                .and_then(Value::as_u64)
                .is_some_and(|count| count > 0)
                && depth + 1 < MAX_FOLDER_DEPTH
            {
                children.push((id.to_owned(), display_name.clone()));
            }
            folders.push(GraphMailbox {
                folder_id: id.to_owned(),
                display_name,
                role,
                total_item_count: item.get("totalItemCount").and_then(Value::as_u64),
            });
            if folders.len() >= MAX_FOLDERS {
                break;
            }
        }
        for (id, path) in children {
            Box::pin(collect_folders(
                client,
                &format!(
                    "{GRAPH_BASE}/me/mailFolders/{}/childFolders?$top=100&$select=id,displayName,childFolderCount,totalItemCount",
                    escape_path(&id)
                ),
                roles,
                Some(&path),
                depth + 1,
                folders,
            ))
            .await?;
        }
        match page.get("@odata.nextLink").and_then(Value::as_str) {
            Some(next) => {
                validate_graph_url(next)?;
                next.clone_into(&mut url);
            }
            None => return Ok(()),
        }
    }
}

// ---------------------------------------------------------------------------
// Message conversion
// ---------------------------------------------------------------------------

struct InlinePart {
    content_id: String,
    content_type: String,
    file_name: String,
    bytes: Vec<u8>,
}

#[allow(clippy::too_many_lines)]
async fn build_body_message(
    client: &GraphClient,
    item: &Value,
    fallback_folder_id: &str,
    mailbox_role: MailboxRole,
) -> Result<GraphMessage, ConnectorError> {
    let id = item
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| ConnectorError::Protocol("message response lacks an id".into()))?;
    validate_graph_id(id, "message")?;
    let folder_id = item
        .get("parentFolderId")
        .and_then(Value::as_str)
        .filter(|folder| validate_graph_id(folder, "folder").is_ok())
        .unwrap_or(fallback_folder_id)
        .to_owned();
    let mut body_complete = true;
    let body = item.get("body").and_then(|body| {
        let content = body.get("content").and_then(Value::as_str)?;
        let content_type = body
            .get("contentType")
            .and_then(Value::as_str)
            .unwrap_or("text");
        Some((content_type.eq_ignore_ascii_case("html"), content))
    });
    let body = match body {
        Some((_, content)) if content.len() > MAX_BODY_BYTES => {
            body_complete = false;
            None
        }
        other => other,
    };

    let mut inline_parts = Vec::new();
    let mut attachments = Vec::new();
    if item
        .get("hasAttachments")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || body.is_some_and(|(is_html, content)| is_html && content.contains("cid:"))
    {
        let listing = client
            .get_json(&format!(
                "{GRAPH_BASE}/me/messages/{}/attachments?$top={MAX_ATTACHMENT_LIST}&$select=id,name,contentType,size,isInline",
                escape_path(id)
            ))
            .await?;
        let mut inline_total = 0_u64;
        for entry in listing
            .get("value")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(attachment_id) = entry.get("id").and_then(Value::as_str) else {
                continue;
            };
            if validate_graph_id(attachment_id, "attachment").is_err() {
                continue;
            }
            let odata_type = entry
                .get("@odata.type")
                .and_then(Value::as_str)
                .unwrap_or("");
            let name = entry
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("attachment")
                .to_owned();
            let content_type = entry
                .get("contentType")
                .and_then(Value::as_str)
                .unwrap_or("application/octet-stream")
                .to_owned();
            let size = entry.get("size").and_then(Value::as_u64).unwrap_or(0);
            let is_inline = entry
                .get("isInline")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let is_file = odata_type.is_empty() || odata_type.ends_with("fileAttachment");
            if is_inline
                && is_file
                && is_safe_inline_image(&content_type)
                && inline_parts.len() < MAX_INLINE_PARTS
                && size <= MAX_INLINE_PART_BYTES
                && inline_total + size <= MAX_INLINE_TOTAL_BYTES
            {
                let full = client
                    .get_json(&format!(
                        "{GRAPH_BASE}/me/messages/{}/attachments/{}",
                        escape_path(id),
                        escape_path(attachment_id)
                    ))
                    .await?;
                let content_id = full
                    .get("contentId")
                    .and_then(Value::as_str)
                    .map(|value| value.trim_matches(['<', '>']).to_owned());
                let bytes = full
                    .get("contentBytes")
                    .and_then(Value::as_str)
                    .and_then(|encoded| BASE64.decode(encoded).ok());
                match (content_id, bytes) {
                    (Some(content_id), Some(bytes))
                        if !content_id.is_empty()
                            && bytes.len() as u64 <= MAX_INLINE_PART_BYTES =>
                    {
                        inline_total += bytes.len() as u64;
                        inline_parts.push(InlinePart {
                            content_id,
                            content_type,
                            file_name: name,
                            bytes,
                        });
                        continue;
                    }
                    _ => body_complete = false,
                }
            } else if is_inline && is_file && is_safe_inline_image(&content_type) {
                body_complete = false;
            }
            if !is_file {
                // Item and reference attachments have no downloadable bytes
                // through `$value`; list them so the user knows they exist.
                attachments.push(GraphAttachmentPart {
                    attachment_id: attachment_id.to_owned(),
                    file_name: name,
                    content_type: "application/octet-stream".into(),
                    size_bytes: size,
                });
                continue;
            }
            attachments.push(GraphAttachmentPart {
                attachment_id: attachment_id.to_owned(),
                file_name: name,
                content_type,
                size_bytes: size,
            });
        }
    }

    Ok(GraphMessage {
        folder_id,
        mailbox_role,
        id: id.to_owned(),
        flags: message_flags(item),
        renderable_message: build_renderable_mime(item, body, &inline_parts),
        attachments,
        catalog_complete: true,
        body_requested: true,
        body_complete,
    })
}

fn is_safe_inline_image(content_type: &str) -> bool {
    matches!(
        content_type.to_ascii_lowercase().as_str(),
        "image/png" | "image/jpeg" | "image/jpg" | "image/gif"
    )
}

fn message_flags(item: &Value) -> Vec<MessageFlag> {
    let mut flags = Vec::new();
    if item.get("isRead").and_then(Value::as_bool).unwrap_or(false) {
        flags.push(MessageFlag::Seen);
    }
    if item
        .get("flag")
        .and_then(|flag| flag.get("flagStatus"))
        .and_then(Value::as_str)
        .is_some_and(|status| status.eq_ignore_ascii_case("flagged"))
    {
        flags.push(MessageFlag::Flagged);
    }
    if item
        .get("isDraft")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        flags.push(MessageFlag::Draft);
    }
    flags
}

fn received_at(item: &Value) -> Option<String> {
    item.get("receivedDateTime")
        .or_else(|| item.get("sentDateTime"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Builds a synthetic RFC 5322 message from Graph JSON.
///
/// `body` is `Some((is_html, content))` for a downloaded body and `None` for a
/// header-only catalogue entry.
fn build_renderable_mime(
    item: &Value,
    body: Option<(bool, &str)>,
    inline: &[InlinePart],
) -> Vec<u8> {
    let mut out = String::new();
    out.push_str("MIME-Version: 1.0\r\n");
    if let Some(from) = item.get("from").and_then(format_address) {
        let _ = write!(out, "From: {from}\r\n");
    }
    for (header, key) in [
        ("To", "toRecipients"),
        ("Cc", "ccRecipients"),
        ("Bcc", "bccRecipients"),
    ] {
        let recipients = item
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(format_address)
            .collect::<Vec<_>>();
        if !recipients.is_empty() {
            let _ = write!(out, "{header}: {}\r\n", recipients.join(",\r\n "));
        }
    }
    let subject = item
        .get("subject")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let _ = write!(out, "Subject: {}\r\n", encode_header_text(subject));
    if let Some(date) = received_at(item).as_deref().and_then(rfc2822_from_iso8601) {
        let _ = write!(out, "Date: {date}\r\n");
    }
    if let Some(message_id) = item
        .get("internetMessageId")
        .and_then(Value::as_str)
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 998
                && value.bytes().all(|byte| byte.is_ascii_graphic())
        })
    {
        let _ = write!(out, "Message-ID: {message_id}\r\n");
    }
    if item
        .get("importance")
        .and_then(Value::as_str)
        .is_some_and(|importance| importance.eq_ignore_ascii_case("high"))
    {
        out.push_str("Importance: high\r\nX-Priority: 1\r\n");
    }

    let Some((is_html, content)) = body else {
        out.push_str("Content-Type: text/plain; charset=utf-8\r\n\r\n");
        return out.into_bytes();
    };
    let body_type = if is_html { "text/html" } else { "text/plain" };
    if inline.is_empty() {
        let _ = write!(
            out,
            "Content-Type: {body_type}; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n"
        );
        out.push_str(&wrap_base64(content.as_bytes()));
        return out.into_bytes();
    }
    let boundary = "=_maicenta_graph_related";
    let _ = write!(
        out,
        "Content-Type: multipart/related; boundary=\"{boundary}\"; type=\"{body_type}\"\r\n\r\n"
    );
    let _ = write!(
        out,
        "--{boundary}\r\nContent-Type: {body_type}; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n"
    );
    out.push_str(&wrap_base64(content.as_bytes()));
    for part in inline {
        let file_name = sanitize_token(&part.file_name);
        let _ = write!(
            out,
            "\r\n--{boundary}\r\nContent-Type: {}\r\nContent-Transfer-Encoding: base64\r\nContent-ID: <{}>\r\nContent-Disposition: inline; filename=\"{file_name}\"\r\n\r\n",
            sanitize_token(&part.content_type),
            sanitize_token(&part.content_id),
        );
        out.push_str(&wrap_base64(&part.bytes));
    }
    let _ = write!(out, "\r\n--{boundary}--\r\n");
    out.into_bytes()
}

fn format_address(value: &Value) -> Option<String> {
    let email = value.get("emailAddress")?;
    let address = email
        .get("address")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|address| {
            !address.is_empty()
                && address.len() <= 320
                && address.contains('@')
                && address
                    .bytes()
                    .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'<' | b'>' | b'"'))
        })?;
    let name = email
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty() && *name != address);
    Some(match name {
        Some(name) => format!("{} <{address}>", encode_header_text(name)),
        None => address.to_owned(),
    })
}

/// Encodes header text as an RFC 2047 encoded word when it is not plain
/// printable ASCII. Plain ASCII is quoted so separators stay unambiguous.
fn encode_header_text(value: &str) -> String {
    let cleaned = value
        .chars()
        .filter(|character| !character.is_control())
        .collect::<String>();
    if cleaned.is_empty() {
        return String::new();
    }
    let plain = cleaned
        .bytes()
        .all(|byte| byte.is_ascii_graphic() || byte == b' ')
        && !cleaned.contains(['"', '<', '>', ',', ':', ';', '\\', '=', '?']);
    if plain {
        return cleaned;
    }
    let encoded = BASE64.encode(cleaned.as_bytes());
    // Encoded words are limited to 75 characters; split long values.
    let chunk_bytes = 45; // 45 raw bytes -> 60 base64 characters + overhead
    if cleaned.len() <= chunk_bytes {
        return format!("=?utf-8?B?{encoded}?=");
    }
    let mut words = Vec::new();
    let mut current = String::new();
    for character in cleaned.chars() {
        if current.len() + character.len_utf8() > chunk_bytes {
            words.push(format!("=?utf-8?B?{}?=", BASE64.encode(current.as_bytes())));
            current.clear();
        }
        current.push(character);
    }
    if !current.is_empty() {
        words.push(format!("=?utf-8?B?{}?=", BASE64.encode(current.as_bytes())));
    }
    words.join(" ")
}

fn sanitize_token(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            character.is_ascii_graphic() && !matches!(character, '"' | '\\' | '<' | '>')
        })
        .take(200)
        .collect()
}

fn wrap_base64(bytes: &[u8]) -> String {
    let encoded = BASE64.encode(bytes);
    let mut wrapped = String::with_capacity(encoded.len() + encoded.len() / 76 * 2 + 2);
    for chunk in encoded.as_bytes().chunks(76) {
        wrapped.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        wrapped.push_str("\r\n");
    }
    wrapped
}

/// Converts a Graph ISO 8601 UTC timestamp such as
/// `2026-09-04T10:15:30Z` or `2026-09-04T10:15:30.1234567Z` to RFC 2822.
fn rfc2822_from_iso8601(value: &str) -> Option<String> {
    const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let value = value.trim().trim_end_matches('Z');
    let (date, time) = value.split_once('T')?;
    let mut date_parts = date.split('-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: u32 = date_parts.next()?.parse().ok()?;
    let day: u32 = date_parts.next()?.parse().ok()?;
    let time = time.split('.').next()?;
    let mut time_parts = time.split(':');
    let hour: u32 = time_parts.next()?.parse().ok()?;
    let minute: u32 = time_parts.next()?.parse().ok()?;
    let second: u32 = time_parts.next().unwrap_or("0").parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }
    let second = second.min(59);
    // Days from civil (Howard Hinnant's algorithm) to derive the weekday.
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (i64::from(month) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let weekday = ((days % 7) + 11) % 7; // 0 = Sunday
    Some(format!(
        "{}, {day:02} {} {year:04} {hour:02}:{minute:02}:{second:02} +0000",
        WEEKDAYS[usize::try_from(weekday).ok()?],
        MONTHS[(month - 1) as usize]
    ))
}

// ---------------------------------------------------------------------------
// Mutations, drafts, submission
// ---------------------------------------------------------------------------

async fn apply_mutation(
    client: &GraphClient,
    mutation: &GraphMutation,
) -> Result<bool, ConnectorError> {
    validate_graph_id(&mutation.message_id, "message")?;
    let message_url = format!(
        "{GRAPH_BASE}/me/messages/{}",
        escape_path(&mutation.message_id)
    );
    client
        .patch_json(
            &message_url,
            &json!({
                "isRead": mutation.seen,
                "flag": { "flagStatus": if mutation.flagged { "flagged" } else { "notFlagged" } },
            }),
        )
        .await?;
    let Some(target) = &mutation.target_folder_id else {
        return Ok(false);
    };
    validate_graph_id(target, "folder")?;
    client
        .post_json(
            &format!("{message_url}/move"),
            &json!({ "destinationId": target }),
        )
        .await?;
    Ok(true)
}

async fn apply_draft_operation(
    client: &GraphClient,
    account: &MailAccount,
    operation: &GraphDraftOperation,
) -> Result<Option<GraphDraftIdentity>, ConnectorError> {
    validate_graph_id(&operation.target_folder_id, "folder")?;
    // Render first so an invalid draft never removes its predecessor.
    let mime = operation
        .message
        .as_ref()
        .map(|message| {
            render_outgoing_message(
                account,
                message,
                Some(&format!(
                    "<{}@maicenta.local>",
                    sanitize_token(&operation.local_key)
                )),
                false,
            )
        })
        .transpose()?;
    if let Some(previous) = &operation.previous_remote {
        validate_graph_id(&previous.message_id, "message")?;
        client
            .delete(&format!(
                "{GRAPH_BASE}/me/messages/{}",
                escape_path(&previous.message_id)
            ))
            .await?;
    }
    let Some(mime) = mime else {
        return Ok(None);
    };
    let created = client
        .post_mime(&format!("{GRAPH_BASE}/me/messages"), &mime)
        .await?;
    let created_id = created
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| ConnectorError::Protocol("draft upload returned no message id".into()))?
        .to_owned();
    validate_graph_id(&created_id, "message")?;
    let mut folder_id = created
        .get("parentFolderId")
        .and_then(Value::as_str)
        .unwrap_or(&operation.target_folder_id)
        .to_owned();
    let mut message_id = created_id.clone();
    if folder_id != operation.target_folder_id {
        let moved = client
            .post_json(
                &format!("{GRAPH_BASE}/me/messages/{}/move", escape_path(&created_id)),
                &json!({ "destinationId": operation.target_folder_id }),
            )
            .await?;
        if let Some(id) = moved.get("id").and_then(Value::as_str) {
            validate_graph_id(id, "message")?;
            id.clone_into(&mut message_id);
        }
        folder_id.clone_from(&operation.target_folder_id);
    }
    Ok(Some(GraphDraftIdentity {
        folder_id,
        message_id,
    }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        build_renderable_mime, encode_header_text, escape_path, message_flags,
        rfc2822_from_iso8601, validate_graph_id, validate_graph_url,
    };
    use maicenta_domain::MessageFlag;

    #[test]
    fn converts_iso8601_to_rfc2822() {
        assert_eq!(
            rfc2822_from_iso8601("2026-09-04T10:15:30Z").as_deref(),
            Some("Fri, 04 Sep 2026 10:15:30 +0000")
        );
        assert_eq!(
            rfc2822_from_iso8601("2000-02-29T23:59:59.1234567Z").as_deref(),
            Some("Tue, 29 Feb 2000 23:59:59 +0000")
        );
        assert_eq!(rfc2822_from_iso8601("not a date"), None);
    }

    #[test]
    fn encodes_non_ascii_header_text() {
        assert_eq!(encode_header_text("Plain subject"), "Plain subject");
        assert_eq!(
            encode_header_text("Grüße"),
            format!(
                "=?utf-8?B?{}?=",
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "Grüße")
            )
        );
        assert!(encode_header_text("Quoted \"name\"").starts_with("=?utf-8?B?"));
    }

    #[test]
    fn maps_graph_state_to_portable_flags() {
        let item = json!({
            "isRead": true,
            "isDraft": false,
            "flag": { "flagStatus": "flagged" },
        });
        assert_eq!(
            message_flags(&item),
            vec![MessageFlag::Seen, MessageFlag::Flagged]
        );
    }

    #[test]
    fn builds_renderable_mime_from_graph_json() {
        let item = json!({
            "subject": "Statusbericht",
            "from": { "emailAddress": { "name": "Anna Müller", "address": "anna@example.org" } },
            "toRecipients": [
                { "emailAddress": { "name": "Team", "address": "team@example.org" } }
            ],
            "receivedDateTime": "2026-09-04T10:15:30Z",
            "internetMessageId": "<abc@example.org>",
            "importance": "high",
        });
        let mime = build_renderable_mime(&item, Some((true, "<p>Hallo</p>")), &[]);
        let text = String::from_utf8(mime).expect("ascii mime");
        assert!(text.contains("Subject: Statusbericht\r\n"));
        assert!(text.contains("From: =?utf-8?B?"));
        assert!(text.contains("To: Team <team@example.org>\r\n"));
        assert!(text.contains("Date: Fri, 04 Sep 2026 10:15:30 +0000\r\n"));
        assert!(text.contains("Message-ID: <abc@example.org>\r\n"));
        assert!(text.contains("Importance: high\r\n"));
        assert!(text.contains("Content-Type: text/html; charset=utf-8\r\n"));
        let rendered = maicenta_rendering::MessageRenderer
            .render(text.as_bytes(), maicenta_rendering::RenderPolicy::default())
            .expect("renderable");
        assert_eq!(rendered.subject.as_deref(), Some("Statusbericht"));
        assert_eq!(rendered.from_address.as_deref(), Some("anna@example.org"));
        assert_eq!(rendered.from_display_name.as_deref(), Some("Anna Müller"));
        assert!(rendered.sanitized_html.expect("html").contains("Hallo"));
    }

    #[test]
    fn rejects_foreign_links_and_unsafe_identifiers() {
        assert!(validate_graph_url("https://graph.microsoft.com/v1.0/me/messages/delta").is_ok());
        assert!(validate_graph_url("https://evil.example/graph.microsoft.com/v1.0").is_err());
        assert!(validate_graph_id("AAMkAGI2THVSAAA=", "message").is_ok());
        assert!(validate_graph_id("../me", "message").is_err());
        assert!(validate_graph_id("", "message").is_err());
        assert_eq!(escape_path("a+b/c="), "a%2Bb%2Fc%3D");
    }
}
