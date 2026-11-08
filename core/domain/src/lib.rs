//! Provider-independent domain types for the MAICENTA workspace.
//!
//! This crate intentionally contains no Flutter, database, or mail-protocol
//! dependencies. Integrations depend on these stable domain types.

mod collections;
mod identifiers;
mod mail;
mod modules;
mod workspace;

pub use collections::{CollectionKind, DavAccount, DavCollection, RemoteDeletion};
pub use identifiers::{
    AccountId, AttachmentId, CollectionId, DomainError, MailboxId, MessageId, WorkspaceItemId,
};
pub use mail::{
    MailAddress, Mailbox, MailboxRole, MessageAttachment, MessageBody, MessageFlag,
    MessageRecipients, MessageSummary,
};
pub use modules::{ModuleState, WorkspaceModule};
pub use workspace::{
    CalendarEvent, Contact, ContactEmail, ContactLabel, ContactPhone, MailAccount, MailProvider,
    PostalAddress, Recurrence, RecurrenceFrequency, SyncMetadata, TaskItem, TaskPriority,
    TaskStatus, TransportSecurity,
};
