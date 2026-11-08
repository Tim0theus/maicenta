use crate::{AccountId, CollectionId, MailAddress, WorkspaceItemId};

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
    /// Additional sender addresses the provider lets this account submit as,
    /// for example aliases configured at Strato, IONOS or mailbox.org, or
    /// Exchange "send as" addresses. The primary `email` is never repeated
    /// here. An alias without a display name uses `display_name`.
    pub aliases: Vec<MailAddress>,
    pub last_sync_at_ms: Option<i64>,
}

impl MailAccount {
    /// Resolves the sender identity used for a message sent from `address`.
    ///
    /// The primary address and every configured alias are accepted, compared
    /// case-insensitively. The returned address always carries a display
    /// name: the alias's own name or, as a fallback, the account name.
    /// Returns `None` for an address that does not belong to this account so
    /// a composer can never submit with an arbitrary `From` header.
    #[must_use]
    pub fn sender_identity(&self, address: &str) -> Option<MailAddress> {
        let fallback_name = Some(self.display_name.trim())
            .filter(|name| !name.is_empty())
            .map(str::to_owned);
        if self.email.address().eq_ignore_ascii_case(address) {
            return MailAddress::new(self.email.address(), fallback_name).ok();
        }
        self.aliases
            .iter()
            .find(|alias| alias.address().eq_ignore_ascii_case(address))
            .and_then(|alias| {
                let name = alias.display_name().map(str::to_owned).or(fallback_name);
                MailAddress::new(alias.address(), name).ok()
            })
    }
}

/// Where a calendar event, task, or contact lives on a server, if anywhere.
///
/// Local-only records keep every field empty. Synchronized records remember
/// their collection, the server's identifier (`uid`), resource location, and
/// entity tag, plus whether a local change still has to be pushed.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SyncMetadata {
    /// Calendar, task list, or address book the record belongs to; `None`
    /// for the local personal collection.
    pub collection_id: Option<CollectionId>,
    /// iCalendar or vCard `UID`.
    pub uid: Option<String>,
    /// Path of the resource on the server, relative to the server root.
    pub remote_href: Option<String>,
    /// Entity tag of the server copy this record was last reconciled with.
    pub etag: Option<String>,
    /// Changed locally since the last reconciliation with the server.
    pub dirty: bool,
}

impl SyncMetadata {
    /// Metadata for a record that lives only in the local profile.
    #[must_use]
    pub fn local() -> Self {
        Self::default()
    }
}

/// How often a recurring event repeats.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecurrenceFrequency {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

/// Repetition pattern of a calendar event, a subset of the iCalendar RRULE.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Recurrence {
    pub frequency: RecurrenceFrequency,
    /// Every `interval` days, weeks, months, or years; at least 1.
    pub interval: u32,
    /// ISO weekdays (1 = Monday … 7 = Sunday) on which a weekly event
    /// repeats; empty means the weekday of the first occurrence.
    pub weekdays: Vec<u8>,
    /// Last occurrence may start at or before this Unix timestamp.
    pub until_ms: Option<i64>,
    /// Total number of occurrences including the first.
    pub count: Option<u32>,
}

/// One local calendar event represented using portable Unix timestamps.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalendarEvent {
    pub id: WorkspaceItemId,
    pub title: String,
    pub starts_at_ms: i64,
    pub ends_at_ms: i64,
    /// The event covers whole days. `starts_at_ms` and `ends_at_ms` then
    /// mark local midnight boundaries supplied by the client; the end is the
    /// midnight after the last day.
    pub all_day: bool,
    pub location: Option<String>,
    pub description: Option<String>,
    /// Minutes before `starts_at_ms` at which the reminder becomes due.
    /// `None` means the event has no reminder.
    pub reminder_minutes_before: Option<u32>,
    /// Occurrences starting at or before this Unix timestamp had their
    /// reminder closed by the user. `None` means no reminder was closed.
    pub reminder_dismissed_through_ms: Option<i64>,
    /// Repetition pattern; `None` for a single event.
    pub recurrence: Option<Recurrence>,
    /// ISO 8601 calendar dates (`YYYY-MM-DD`) of occurrences the user removed
    /// from a recurring event.
    pub exception_dates: Vec<String>,
    pub sync: SyncMetadata,
}

impl CalendarEvent {
    /// Unix timestamp at which the reminder of the first occurrence becomes
    /// due, if one is set.
    #[must_use]
    pub fn reminder_at_ms(&self) -> Option<i64> {
        self.reminder_minutes_before
            .map(|minutes| self.starts_at_ms - i64::from(minutes) * 60_000)
    }

    /// Whether the reminder of the occurrence starting at `occurrence_start_ms`
    /// was closed by the user.
    #[must_use]
    pub fn reminder_dismissed_for(&self, occurrence_start_ms: i64) -> bool {
        self.reminder_dismissed_through_ms
            .is_some_and(|through| occurrence_start_ms <= through)
    }
}

/// Importance of a task, mirroring the three classic levels.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TaskPriority {
    Low,
    #[default]
    Normal,
    High,
}

/// Progress state of a task.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TaskStatus {
    #[default]
    NotStarted,
    InProgress,
    Waiting,
    Deferred,
    Completed,
}

/// One local task with optional dates, a reminder, and notes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskItem {
    pub id: WorkspaceItemId,
    pub title: String,
    pub description: Option<String>,
    pub starts_at_ms: Option<i64>,
    pub due_at_ms: Option<i64>,
    pub status: TaskStatus,
    pub priority: TaskPriority,
    /// Absolute time at which the reminder becomes due.
    pub reminder_at_ms: Option<i64>,
    /// The user closed the reminder. Clients reset the flag when the
    /// reminder time changes.
    pub reminder_dismissed: bool,
    pub completed_at_ms: Option<i64>,
    pub sync: SyncMetadata,
}

impl TaskItem {
    /// Whether the task reached its final state.
    #[must_use]
    pub fn completed(&self) -> bool {
        self.status == TaskStatus::Completed
    }
}

/// Kind of a contact detail such as an email address or phone number.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ContactLabel {
    Work,
    Home,
    Mobile,
    Fax,
    #[default]
    Other,
}

/// One labelled email address of a contact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContactEmail {
    pub label: ContactLabel,
    pub address: MailAddress,
}

/// One labelled phone number of a contact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContactPhone {
    pub label: ContactLabel,
    pub number: String,
}

/// One labelled postal address of a contact. Individual lines may be empty;
/// at least one line is filled.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PostalAddress {
    pub label: ContactLabel,
    pub street: String,
    pub postal_code: String,
    pub city: String,
    pub country: String,
}

/// One local address-book contact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Contact {
    pub id: WorkspaceItemId,
    /// Display name shown in lists and used as the sender/recipient name.
    pub name: String,
    pub company: Option<String>,
    pub job_title: Option<String>,
    pub emails: Vec<ContactEmail>,
    pub phones: Vec<ContactPhone>,
    pub addresses: Vec<PostalAddress>,
    /// ISO 8601 calendar date (`YYYY-MM-DD`) without a time zone.
    pub birthday: Option<String>,
    pub website: Option<String>,
    pub notes: Option<String>,
    pub sync: SyncMetadata,
}

impl Contact {
    /// The first email address, carrying the contact's display name so it can
    /// be used directly as a recipient.
    #[must_use]
    pub fn primary_email(&self) -> Option<MailAddress> {
        self.emails.first().and_then(|email| {
            MailAddress::new(email.address.address(), Some(self.name.clone())).ok()
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{AccountId, MailAddress, WorkspaceItemId};

    use super::{
        CalendarEvent, Contact, ContactEmail, ContactLabel, MailAccount, MailProvider,
        SyncMetadata, TaskItem, TaskPriority, TaskStatus, TransportSecurity,
    };

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
            aliases: Vec::new(),
            last_sync_at_ms: None,
        };

        assert_eq!(account.id.as_str(), "work");
        assert_eq!(account.smtp_security, TransportSecurity::StartTls);
        assert!(WorkspaceItemId::parse("local.contact.1").is_ok());
    }

    #[test]
    fn resolves_primary_address_and_aliases_but_rejects_foreign_senders() {
        let account = MailAccount {
            id: AccountId::parse("strato").expect("account id"),
            provider: MailProvider::ImapSmtp,
            display_name: "Timm Beckmann".into(),
            email: MailAddress::new("timm@example.org", None).expect("primary"),
            imap_host: "imap.example.org".into(),
            imap_port: 993,
            imap_security: TransportSecurity::Tls,
            imap_username: "timm@example.org".into(),
            smtp_host: "smtp.example.org".into(),
            smtp_port: 465,
            smtp_security: TransportSecurity::Tls,
            smtp_username: "timm@example.org".into(),
            aliases: vec![
                MailAddress::new("kontakt@example.org", Some("Example Support".into()))
                    .expect("named alias"),
                MailAddress::new("info@example.org", None).expect("unnamed alias"),
            ],
            last_sync_at_ms: None,
        };

        let primary = account
            .sender_identity("TIMM@example.org")
            .expect("primary");
        assert_eq!(primary.address(), "timm@example.org");
        assert_eq!(primary.display_name(), Some("Timm Beckmann"));

        let named = account
            .sender_identity("kontakt@example.org")
            .expect("named alias");
        assert_eq!(named.display_name(), Some("Example Support"));

        let unnamed = account.sender_identity("Info@Example.org").expect("alias");
        assert_eq!(unnamed.address(), "info@example.org");
        assert_eq!(unnamed.display_name(), Some("Timm Beckmann"));

        assert_eq!(account.sender_identity("ceo@bank.example"), None);
    }

    #[test]
    fn derives_reminder_and_completion_state() {
        let event = CalendarEvent {
            id: WorkspaceItemId::parse("local.event.1").expect("event id"),
            title: "Planung".into(),
            starts_at_ms: 3_600_000,
            ends_at_ms: 7_200_000,
            all_day: false,
            location: None,
            description: None,
            reminder_minutes_before: Some(15),
            reminder_dismissed_through_ms: Some(3_600_000),
            recurrence: None,
            exception_dates: Vec::new(),
            sync: SyncMetadata::local(),
        };
        assert_eq!(event.reminder_at_ms(), Some(2_700_000));
        assert!(event.reminder_dismissed_for(3_600_000));
        assert!(!event.reminder_dismissed_for(90_000_000));

        let task = TaskItem {
            id: WorkspaceItemId::parse("local.task.1").expect("task id"),
            title: "Aufgabe".into(),
            description: None,
            starts_at_ms: None,
            due_at_ms: None,
            status: TaskStatus::Completed,
            priority: TaskPriority::High,
            reminder_at_ms: None,
            reminder_dismissed: false,
            completed_at_ms: Some(1),
            sync: SyncMetadata::local(),
        };
        assert!(task.completed());

        let contact = Contact {
            id: WorkspaceItemId::parse("local.contact.1").expect("contact id"),
            name: "Anna Schneider".into(),
            company: None,
            job_title: None,
            emails: vec![ContactEmail {
                label: ContactLabel::Work,
                address: MailAddress::new("anna@example.org", None).expect("address"),
            }],
            phones: Vec::new(),
            addresses: Vec::new(),
            birthday: None,
            website: None,
            notes: None,
            sync: SyncMetadata::local(),
        };
        let primary = contact.primary_email().expect("primary email");
        assert_eq!(primary.address(), "anna@example.org");
        assert_eq!(primary.display_name(), Some("Anna Schneider"));
    }
}
