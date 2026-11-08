use crate::{AccountId, CollectionId};

/// What a synchronized collection holds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollectionKind {
    /// `CalDAV` calendar with `VEVENT` components.
    Calendar,
    /// `CalDAV` collection with `VTODO` components.
    TaskList,
    /// `CardDAV` address book.
    AddressBook,
}

/// Persisted configuration of one `CalDAV`/`CardDAV` account.
///
/// The password lives in the encrypted profile vault under the account
/// identifier and is never part of this snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DavAccount {
    pub id: AccountId,
    pub display_name: String,
    /// Server root or principal URL the user entered, `https://` only.
    pub server_url: String,
    pub username: String,
    pub last_sync_at_ms: Option<i64>,
}

/// One calendar, task list, or address book discovered on a DAV account.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DavCollection {
    pub id: CollectionId,
    pub account_id: AccountId,
    pub kind: CollectionKind,
    /// Absolute path of the collection on the server.
    pub href: String,
    pub display_name: String,
    /// CSS-style color such as `#0F6CBD`, when the server supplies one.
    pub color: Option<String>,
    /// Collection tag of the last reconciled server state.
    pub ctag: Option<String>,
    /// Shown and synchronized; the user can switch a collection off.
    pub enabled: bool,
    pub position: u32,
}

/// A record the user deleted locally whose server copy still has to go.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteDeletion {
    pub collection_id: CollectionId,
    pub href: String,
    pub etag: Option<String>,
}
