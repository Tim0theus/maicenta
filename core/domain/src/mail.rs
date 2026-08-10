use crate::{AccountId, AttachmentId, DomainError, MailboxId, MessageId};

/// Email address with an optional human-readable display name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MailAddress {
    address: String,
    display_name: Option<String>,
}

impl MailAddress {
    /// Creates a minimally validated email address.
    ///
    /// Protocol adapters remain responsible for complete RFC parsing.
    ///
    /// # Errors
    ///
    /// Returns [`DomainError::InvalidEmailAddress`] for an obviously malformed
    /// address or [`DomainError::EmptyDisplayName`] for a blank display name.
    pub fn new(
        address: impl Into<String>,
        display_name: Option<String>,
    ) -> Result<Self, DomainError> {
        let address = address.into();
        let (local, domain) = address
            .split_once('@')
            .ok_or(DomainError::InvalidEmailAddress)?;

        if local.is_empty()
            || domain.is_empty()
            || domain.starts_with('.')
            || domain.ends_with('.')
            || address.chars().any(char::is_whitespace)
        {
            return Err(DomainError::InvalidEmailAddress);
        }

        if display_name
            .as_ref()
            .is_some_and(|name| name.trim().is_empty())
        {
            return Err(DomainError::EmptyDisplayName);
        }

        Ok(Self {
            address,
            display_name,
        })
    }

    #[must_use]
    pub fn address(&self) -> &str {
        &self.address
    }

    #[must_use]
    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }
}

/// Standard semantic role of a mailbox.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MailboxRole {
    Inbox,
    Drafts,
    Sent,
    Archive,
    Trash,
    Junk,
    Custom,
}

/// A mailbox synchronized from a provider.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mailbox {
    pub id: MailboxId,
    pub account_id: AccountId,
    pub display_name: String,
    pub role: MailboxRole,
    pub unread_count: u32,
    pub total_count: u32,
}

/// Portable message flags understood by the workspace.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MessageFlag {
    Seen,
    Answered,
    Flagged,
    Draft,
    Deleted,
}

/// Lightweight message data used by lists and search results.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageSummary {
    pub id: MessageId,
    pub account_id: AccountId,
    pub mailbox_id: MailboxId,
    pub from: MailAddress,
    pub subject: String,
    pub preview: String,
    /// Unix timestamp in milliseconds.
    pub received_at_ms: i64,
    pub flags: Vec<MessageFlag>,
    pub has_attachments: bool,
}

impl MessageSummary {
    #[must_use]
    pub fn is_unread(&self) -> bool {
        !self.flags.contains(&MessageFlag::Seen)
    }
}

/// Message content loaded independently from list metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageBody {
    pub message_id: MessageId,
    pub plain_text: Option<String>,
    /// Sanitized HTML only. Original MIME data belongs in object storage.
    pub sanitized_html: Option<String>,
}

/// Searchable recipient headers retained independently from a message body.
///
/// Keeping these compact strings available for every catalogued message makes
/// participant search reliable even when the body is not cached locally.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageRecipients {
    pub message_id: MessageId,
    pub to: String,
    pub cc: String,
    pub bcc: String,
}

/// Metadata for one locally cached or selectively downloadable attachment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageAttachment {
    pub id: AttachmentId,
    pub message_id: MessageId,
    pub file_name: String,
    pub content_type: String,
    pub size_bytes: u64,
    /// Relative path below the profile's object directory when cached.
    pub object_key: Option<String>,
    /// Validated IMAP MIME section for an on-demand server download.
    pub remote_section: Option<String>,
    /// MIME Content-Transfer-Encoding used by the remote section.
    pub transfer_encoding: Option<String>,
}

impl MessageAttachment {
    #[must_use]
    pub const fn is_available_locally(&self) -> bool {
        self.object_key.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::{MailAddress, MessageFlag, MessageSummary};
    use crate::{AccountId, DomainError, MailboxId, MessageId};

    #[test]
    fn validates_basic_mail_addresses() {
        let address =
            MailAddress::new("anna@example.org", Some("Anna".into())).expect("valid address");
        assert_eq!(address.address(), "anna@example.org");
        assert_eq!(address.display_name(), Some("Anna"));
    }

    #[test]
    fn rejects_whitespace_in_mail_address() {
        assert_eq!(
            MailAddress::new("anna @example.org", None),
            Err(DomainError::InvalidEmailAddress)
        );
    }

    #[test]
    fn derives_unread_state_from_flags() {
        let summary = MessageSummary {
            id: MessageId::parse("message-1").expect("valid id"),
            account_id: AccountId::parse("personal").expect("valid id"),
            mailbox_id: MailboxId::parse("inbox").expect("valid id"),
            from: MailAddress::new("anna@example.org", None).expect("valid address"),
            subject: "Hello".into(),
            preview: "Preview".into(),
            received_at_ms: 0,
            flags: vec![MessageFlag::Flagged],
            has_attachments: false,
        };

        assert!(summary.is_unread());
    }
}
