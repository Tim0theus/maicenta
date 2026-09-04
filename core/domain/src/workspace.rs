use crate::{AccountId, MailAddress, WorkspaceItemId};

/// TLS mode used by an incoming or outgoing mail endpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportSecurity {
    /// TLS is established immediately after connecting.
    Tls,
    /// A plaintext connection is upgraded with STARTTLS before login.
    StartTls,
}

/// Remote protocol family used to reach one mail account.
///
/// The standards connector speaks IMAP and SMTP. Provider-specific connectors
/// are explicit variants so synchronization, identity handling, and
/// capabilities can be selected without inspecting server names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MailProvider {
    /// IMAP for incoming mail and SMTP for submission.
    ImapSmtp,
    /// Microsoft Graph mail API for Exchange Online tenants.
    MicrosoftGraph,
}

/// Persisted configuration for one mail account.
///
/// Passwords and OAuth tokens are intentionally absent from snapshots. They
/// belong in the encrypted profile vault and are referenced by `id`; only the
/// profile master key is stored by the operating system.
///
/// The IMAP/SMTP endpoint fields describe the standards endpoints of the
/// account. A [`MailProvider::MicrosoftGraph`] account keeps them as a
/// documented fallback description but is synchronized through Graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MailAccount {
    pub id: AccountId,
    pub provider: MailProvider,
    pub display_name: String,
    pub email: MailAddress,
    pub imap_host: String,
    pub imap_port: u16,
    pub imap_security: TransportSecurity,
    pub imap_username: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_security: TransportSecurity,
    pub smtp_username: String,
    pub last_sync_at_ms: Option<i64>,
}

/// One local calendar event represented using portable Unix timestamps.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalendarEvent {
    pub id: WorkspaceItemId,
    pub title: String,
    pub starts_at_ms: i64,
    pub ends_at_ms: i64,
    pub location: Option<String>,
}

/// One local task with an optional due date.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskItem {
    pub id: WorkspaceItemId,
    pub title: String,
    pub due_at_ms: Option<i64>,
    pub completed: bool,
}

/// One local address-book contact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Contact {
    pub id: WorkspaceItemId,
    pub name: String,
    pub email: MailAddress,
}

#[cfg(test)]
mod tests {
    use crate::{AccountId, MailAddress, WorkspaceItemId};

    use super::{MailAccount, MailProvider, TransportSecurity};

    #[test]
    fn account_configuration_contains_no_secret_material() {
        let account = MailAccount {
            id: AccountId::parse("work").expect("account id"),
            provider: MailProvider::ImapSmtp,
            display_name: "Arbeit".into(),
            email: MailAddress::new("user@example.org", Some("User".into())).expect("mail address"),
            imap_host: "imap.example.org".into(),
            imap_port: 993,
            imap_security: TransportSecurity::Tls,
            imap_username: "user@example.org".into(),
            smtp_host: "smtp.example.org".into(),
            smtp_port: 587,
            smtp_security: TransportSecurity::StartTls,
            smtp_username: "user@example.org".into(),
            last_sync_at_ms: None,
        };

        assert_eq!(account.id.as_str(), "work");
        assert_eq!(account.smtp_security, TransportSecurity::StartTls);
        assert!(WorkspaceItemId::parse("local.contact.1").is_ok());
    }
}
