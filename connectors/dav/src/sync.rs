//! Two-way reconciliation of one DAV account with the local profile.
//!
//! Each enabled collection is handled in three steps: pending local
//! deletions are sent, dirty records are uploaded with conditional `PUT`,
//! then the server listing is compared with the local copies by entity tag
//! and anything new or changed is downloaded. The server wins conflicts: a
//! record whose upload is refused keeps its local edits only until the
//! server copy arrives in the same pass.

use maicenta_application::{ApplicationError, DavStore, WorkspaceStore};
use maicenta_domain::{
    AccountId, CalendarEvent, CollectionId, CollectionKind, Contact, DavAccount, DavCollection,
    RemoteDeletion, SyncMetadata, TaskItem, WorkspaceItemId,
};

use crate::{
    client::{DavClient, DavError, DiscoveredCollection, encode_segment, stable_key},
    ical::{event_from_ical, ical_from_event, ical_from_task, task_from_ical},
    vcard::{contact_from_vcard, vcard_from_contact},
};

const ICAL_CONTENT_TYPE: &str = "text/calendar; charset=utf-8";
const VCARD_CONTENT_TYPE: &str = "text/vcard; charset=utf-8";

/// Outcome of synchronizing one collection.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CollectionReport {
    pub collection_id: String,
    pub display_name: String,
    /// Local changes written to the server.
    pub uploaded: u32,
    /// Server changes written to the profile.
    pub downloaded: u32,
    /// Records removed locally because they vanished on the server.
    pub removed_locally: u32,
    /// Local deletions carried out on the server.
    pub removed_remotely: u32,
}

/// Outcome of synchronizing one account.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SyncReport {
    pub collections: Vec<CollectionReport>,
    /// Problems that did not stop the pass, one line each.
    pub warnings: Vec<String>,
}

/// Errors raised while synchronizing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SyncError {
    Dav(DavError),
    Storage(ApplicationError),
}

impl From<DavError> for SyncError {
    fn from(error: DavError) -> Self {
        Self::Dav(error)
    }
}

impl From<ApplicationError> for SyncError {
    fn from(error: ApplicationError) -> Self {
        Self::Storage(error)
    }
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dav(error) => write!(formatter, "{error}"),
            Self::Storage(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for SyncError {}

/// Local identifier of a collection, derived from its server path.
///
/// # Panics
///
/// Never in practice: the identifier is built from a valid account
/// identifier and a hex digest.
#[must_use]
pub fn collection_id_for(account_id: &AccountId, href: &str) -> CollectionId {
    CollectionId::parse(format!("{account_id}.{}", stable_key(href))).expect("derived identifier")
}

fn item_id_for(collection_id: &CollectionId, href: &str) -> WorkspaceItemId {
    WorkspaceItemId::parse(format!("{collection_id}.{}", stable_key(href)))
        .expect("derived identifier")
}

/// Records the collections of an account found on the server: new ones are
/// added switched on, known ones keep their settings, vanished ones are
/// removed together with their records.
///
/// # Errors
///
/// Returns a storage error when the collections cannot be written.
pub fn reconcile_collections<S: DavStore>(
    store: &mut S,
    account_id: &AccountId,
    discovered: &[DiscoveredCollection],
) -> Result<Vec<DavCollection>, ApplicationError> {
    let known: Vec<DavCollection> = store
        .list_dav_collections()?
        .into_iter()
        .filter(|collection| &collection.account_id == account_id)
        .collect();
    let mut result = Vec::new();
    for (position, found) in discovered.iter().enumerate() {
        let existing = known
            .iter()
            .find(|collection| collection.href == found.href);
        let collection = DavCollection {
            id: existing.map_or_else(
                || collection_id_for(account_id, &found.href),
                |c| c.id.clone(),
            ),
            account_id: account_id.clone(),
            kind: found.kind,
            href: found.href.clone(),
            display_name: found.display_name.clone(),
            color: found
                .color
                .clone()
                .or_else(|| existing.and_then(|c| c.color.clone())),
            ctag: existing.and_then(|c| c.ctag.clone()),
            enabled: existing.is_none_or(|c| c.enabled),
            position: u32::try_from(position).unwrap_or(u32::MAX),
        };
        store.save_dav_collection(&collection)?;
        result.push(collection);
    }
    for stale in known
        .iter()
        .filter(|collection| !discovered.iter().any(|found| found.href == collection.href))
    {
        store.delete_dav_collection(&stale.id)?;
    }
    Ok(result)
}

/// Synchronizes every enabled collection of one account.
///
/// # Errors
///
/// Returns a [`SyncError`] when discovery fails or the profile cannot be
/// read; problems with single collections or records become warnings.
pub async fn synchronize_account<S: WorkspaceStore + DavStore>(
    store: &mut S,
    account: &DavAccount,
    password: &str,
) -> Result<SyncReport, SyncError> {
    let client = DavClient::new(&account.server_url, &account.username, password)?;
    let discovered = client.discover().await?;
    let collections = reconcile_collections(store, &account.id, &discovered)?;
    let mut report = SyncReport::default();
    for collection in collections.iter().filter(|collection| collection.enabled) {
        match synchronize_collection(store, &client, collection).await {
            Ok((collection_report, warnings)) => {
                report.collections.push(collection_report);
                report.warnings.extend(warnings);
            }
            Err(error) => report
                .warnings
                .push(format!("{}: {error}", collection.display_name)),
        }
    }
    store.update_dav_account_last_sync(&account.id, now_ms())?;
    Ok(report)
}

fn now_ms() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default(),
    )
    .unwrap_or(i64::MAX)
}

/// A local record of any of the three kinds, with the pieces the
/// synchronization needs.
enum LocalRecord {
    Event(CalendarEvent),
    Task(TaskItem),
    Contact(Contact),
}

impl LocalRecord {
    fn id(&self) -> &WorkspaceItemId {
        match self {
            Self::Event(event) => &event.id,
            Self::Task(task) => &task.id,
            Self::Contact(contact) => &contact.id,
        }
    }

    fn sync(&self) -> &SyncMetadata {
        match self {
            Self::Event(event) => &event.sync,
            Self::Task(task) => &task.sync,
            Self::Contact(contact) => &contact.sync,
        }
    }

    fn sync_mut(&mut self) -> &mut SyncMetadata {
        match self {
            Self::Event(event) => &mut event.sync,
            Self::Task(task) => &mut task.sync,
            Self::Contact(contact) => &mut contact.sync,
        }
    }

    fn title(&self) -> &str {
        match self {
            Self::Event(event) => &event.title,
            Self::Task(task) => &task.title,
            Self::Contact(contact) => &contact.name,
        }
    }

    fn body(&self, uid: &str) -> String {
        match self {
            Self::Event(event) => ical_from_event(event, uid),
            Self::Task(task) => ical_from_task(task, uid),
            Self::Contact(contact) => vcard_from_contact(contact, uid),
        }
    }

    fn extension(&self) -> &'static str {
        match self {
            Self::Event(_) | Self::Task(_) => "ics",
            Self::Contact(_) => "vcf",
        }
    }

    fn content_type(&self) -> &'static str {
        match self {
            Self::Event(_) | Self::Task(_) => ICAL_CONTENT_TYPE,
            Self::Contact(_) => VCARD_CONTENT_TYPE,
        }
    }

    fn save<S: WorkspaceStore>(&self, store: &mut S) -> Result<(), ApplicationError> {
        match self {
            Self::Event(event) => store.save_calendar_event(event),
            Self::Task(task) => store.save_task(task),
            Self::Contact(contact) => store.save_contact(contact),
        }
    }

    fn delete<S: WorkspaceStore>(&self, store: &mut S) -> Result<(), ApplicationError> {
        match self {
            Self::Event(event) => store.delete_calendar_event(&event.id),
            Self::Task(task) => store.delete_task(&task.id),
            Self::Contact(contact) => store.delete_contact(&contact.id),
        }
    }
}

fn local_records<S: WorkspaceStore>(
    store: &S,
    collection: &DavCollection,
) -> Result<Vec<LocalRecord>, ApplicationError> {
    let belongs = |sync: &SyncMetadata| sync.collection_id.as_ref() == Some(&collection.id);
    let mut records = Vec::new();
    match collection.kind {
        CollectionKind::Calendar => {
            records.extend(
                store
                    .list_calendar_events()?
                    .into_iter()
                    .filter(|event| belongs(&event.sync))
                    .map(LocalRecord::Event),
            );
            records.extend(
                store
                    .list_tasks()?
                    .into_iter()
                    .filter(|task| belongs(&task.sync))
                    .map(LocalRecord::Task),
            );
        }
        CollectionKind::TaskList => records.extend(
            store
                .list_tasks()?
                .into_iter()
                .filter(|task| belongs(&task.sync))
                .map(LocalRecord::Task),
        ),
        CollectionKind::AddressBook => records.extend(
            store
                .list_contacts()?
                .into_iter()
                .filter(|contact| belongs(&contact.sync))
                .map(LocalRecord::Contact),
        ),
    }
    Ok(records)
}

/// Builds the local record for a downloaded resource. A calendar may hold
/// events and tasks; the component decides.
fn record_from_resource(
    collection: &DavCollection,
    href: &str,
    etag: Option<String>,
    data: &str,
    existing: Option<&LocalRecord>,
) -> Result<LocalRecord, String> {
    let id = existing.map_or_else(
        || item_id_for(&collection.id, href),
        |record| record.id().clone(),
    );
    let sync = SyncMetadata {
        collection_id: Some(collection.id.clone()),
        uid: existing.and_then(|record| record.sync().uid.clone()),
        remote_href: Some(href.to_owned()),
        etag,
        dirty: false,
    };
    match collection.kind {
        CollectionKind::AddressBook => contact_from_vcard(data, id, sync)
            .map(LocalRecord::Contact)
            .map_err(|error| error.to_string()),
        CollectionKind::Calendar | CollectionKind::TaskList => {
            let upper = data.to_ascii_uppercase();
            if upper.contains("BEGIN:VTODO") {
                let mut task = task_from_ical(data, id, sync).map_err(|error| error.to_string())?;
                // The closed-reminder flag is local state the server does
                // not track unless we wrote it ourselves.
                if let Some(LocalRecord::Task(previous)) = existing {
                    task.reminder_dismissed = task.reminder_dismissed
                        || (previous.reminder_dismissed
                            && previous.reminder_at_ms == task.reminder_at_ms);
                }
                Ok(LocalRecord::Task(task))
            } else if upper.contains("BEGIN:VEVENT") {
                let mut event =
                    event_from_ical(data, id, sync).map_err(|error| error.to_string())?;
                if let Some(LocalRecord::Event(previous)) = existing {
                    if event.reminder_dismissed_through_ms.is_none() {
                        event.reminder_dismissed_through_ms =
                            previous.reminder_dismissed_through_ms;
                    }
                }
                Ok(LocalRecord::Event(event))
            } else {
                Err("resource holds neither VEVENT nor VTODO".into())
            }
        }
    }
}

#[allow(clippy::too_many_lines)]
async fn synchronize_collection<S: WorkspaceStore + DavStore>(
    store: &mut S,
    client: &DavClient,
    collection: &DavCollection,
) -> Result<(CollectionReport, Vec<String>), SyncError> {
    let mut report = CollectionReport {
        collection_id: collection.id.to_string(),
        display_name: collection.display_name.clone(),
        ..CollectionReport::default()
    };
    let mut warnings = Vec::new();

    // 1. Deletions the user made locally.
    for deletion in store.list_remote_deletions(&collection.id)? {
        match client
            .delete(&deletion.href, deletion.etag.as_deref())
            .await
        {
            Ok(()) => {
                report.removed_remotely += 1;
                store.clear_remote_deletion(&collection.id, &deletion.href)?;
            }
            Err(DavError::PreconditionFailed) => {
                warnings.push(format!(
                    "{}: ein auf dem Server geänderter Eintrag wurde nicht gelöscht und kommt zurück.",
                    collection.display_name
                ));
                store.clear_remote_deletion(&collection.id, &deletion.href)?;
            }
            Err(error) => return Err(error.into()),
        }
    }

    // 2. Local changes.
    let mut records = local_records(store, collection)?;
    let mut local_hrefs = std::collections::HashMap::new();
    for record in &mut records {
        if record.sync().dirty {
            match upload(client, collection, record).await {
                Ok(()) => {
                    report.uploaded += 1;
                    record.save(store)?;
                }
                Err(DavError::PreconditionFailed) => {
                    warnings.push(format!(
                        "{}: „{}“ wurde auf dem Server geändert; die Serverversion wird übernommen.",
                        collection.display_name,
                        record.title()
                    ));
                    // Drop the local copy's claim so the pull replaces it.
                    record.sync_mut().dirty = false;
                    record.save(store)?;
                }
                Err(error) => return Err(error.into()),
            }
        }
        if let Some(href) = &record.sync().remote_href {
            local_hrefs.insert(href.clone(), record.sync().etag.clone());
        }
    }

    // 3. Server changes.
    let listing = client.list_resources(&collection.href).await?;
    let mut changed = Vec::new();
    for resource in &listing.resources {
        match local_hrefs.get(&resource.href) {
            Some(local_etag) if local_etag.is_some() && *local_etag == resource.etag => {}
            _ => changed.push(resource.href.clone()),
        }
    }
    if !changed.is_empty() {
        let fetched = client
            .fetch_resources(&collection.href, collection.kind, &changed)
            .await?;
        for resource in fetched {
            let existing = records.iter().find(|record| {
                record.sync().remote_href.as_deref() == Some(resource.href.as_str())
            });
            match record_from_resource(
                collection,
                &resource.href,
                resource.etag,
                &resource.data,
                existing,
            ) {
                Ok(record) => {
                    record.save(store)?;
                    report.downloaded += 1;
                }
                Err(detail) => warnings.push(format!(
                    "{}: {} wurde übersprungen ({detail}).",
                    collection.display_name, resource.href
                )),
            }
        }
    }
    let remote_hrefs: std::collections::HashSet<&str> = listing
        .resources
        .iter()
        .map(|resource| resource.href.as_str())
        .collect();
    for record in &records {
        if let Some(href) = &record.sync().remote_href {
            if !remote_hrefs.contains(href.as_str()) {
                record.delete(store)?;
                // The deletion came from the server; nothing to send back.
                store.clear_remote_deletion(&collection.id, href)?;
                report.removed_locally += 1;
            }
        }
    }
    let mut updated = collection.clone();
    updated.ctag = listing.ctag;
    store.save_dav_collection(&updated)?;
    Ok((report, warnings))
}

/// Uploads one record, assigning a UID and server path on first upload.
async fn upload(
    client: &DavClient,
    collection: &DavCollection,
    record: &mut LocalRecord,
) -> Result<(), DavError> {
    let uid = record
        .sync()
        .uid
        .clone()
        .unwrap_or_else(|| format!("{}@maicenta", record.id()));
    let href = record.sync().remote_href.clone().unwrap_or_else(|| {
        let name: String = uid
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        format!(
            "{}/{}.{}",
            collection.href.trim_end_matches('/'),
            encode_segment(&name),
            record.extension()
        )
    });
    let body = record.body(&uid);
    let etag = client
        .put(
            &href,
            body,
            record.content_type(),
            record.sync().etag.as_deref(),
        )
        .await?;
    let etag = match etag {
        Some(etag) => Some(etag),
        None => client.etag(&href).await.unwrap_or(None),
    };
    let sync = record.sync_mut();
    sync.uid = Some(uid);
    sync.remote_href = Some(href);
    sync.etag = etag;
    sync.dirty = false;
    Ok(())
}

/// Keeps the [`RemoteDeletion`] type referenced for readers of this module.
#[allow(dead_code)]
fn _remote_deletion_marker(_: &RemoteDeletion) {}

#[cfg(test)]
mod tests {
    //! The end-to-end test talks to a real CalDAV/CardDAV server. It runs
    //! only when `MAICENTA_DAV_TEST_URL`, `MAICENTA_DAV_TEST_USER`, and
    //! `MAICENTA_DAV_TEST_PASSWORD` point at one (for example a local
    //! Radicale) and is skipped otherwise.

    use maicenta_application::{DavStore, WorkspaceStore};
    use maicenta_domain::{
        AccountId, CalendarEvent, CollectionKind, Contact, ContactEmail, ContactLabel, DavAccount,
        MailAddress, SyncMetadata, TaskItem, TaskPriority, TaskStatus, WorkspaceItemId,
    };
    use maicenta_storage::SqliteMailStore;

    use super::{collection_id_for, synchronize_account};
    use crate::client::DavClient;

    fn test_server() -> Option<(String, String, String)> {
        Some((
            std::env::var("MAICENTA_DAV_TEST_URL").ok()?,
            std::env::var("MAICENTA_DAV_TEST_USER").ok()?,
            std::env::var("MAICENTA_DAV_TEST_PASSWORD").ok()?,
        ))
    }

    #[test]
    fn derives_stable_collection_identifiers() {
        let account = AccountId::parse("dav.home").expect("account");
        let id = collection_id_for(&account, "/dav/calendars/timm/personal/");
        assert!(id.as_str().starts_with("dav.home."));
        assert_eq!(
            id,
            collection_id_for(&account, "/dav/calendars/timm/personal/")
        );
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn synchronizes_both_ways_with_a_real_server() {
        let Some((url, user, password)) = test_server() else {
            eprintln!("MAICENTA_DAV_TEST_URL not set; skipping the server test");
            return;
        };
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_millis();
        let client = DavClient::new(&url, &user, &password).expect("client");
        let base_path = client.base().path().trim_end_matches('/').to_owned();
        let calendar_href = format!("{base_path}/{user}/maicenta-test-{unique}/");
        let book_href = format!("{base_path}/{user}/maicenta-book-{unique}/");
        client
            .create_collection(&calendar_href, CollectionKind::Calendar, "Testkalender")
            .await
            .expect("create calendar");
        client
            .create_collection(&book_href, CollectionKind::AddressBook, "Testbuch")
            .await
            .expect("create address book");

        let mut store = SqliteMailStore::open_in_memory().expect("store");
        let account = DavAccount {
            id: AccountId::parse("dav.test").expect("account"),
            display_name: "Test".into(),
            server_url: url.clone(),
            username: user.clone(),
            last_sync_at_ms: None,
        };
        store.save_dav_account(&account).expect("save account");

        // First pass discovers the collections.
        let report = synchronize_account(&mut store, &account, &password)
            .await
            .expect("initial sync");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let collections = store.list_dav_collections().expect("collections");
        let calendar = collections
            .iter()
            .find(|collection| collection.href == calendar_href)
            .expect("calendar discovered");
        let book = collections
            .iter()
            .find(|collection| collection.href == book_href)
            .expect("address book discovered");
        assert_eq!(calendar.kind, CollectionKind::Calendar);
        assert_eq!(book.kind, CollectionKind::AddressBook);

        // Local records are uploaded.
        let event = CalendarEvent {
            id: WorkspaceItemId::parse("local.event.up").expect("id"),
            title: "Hochgeladener Termin".into(),
            starts_at_ms: 1_785_828_600_000,
            ends_at_ms: 1_785_830_400_000,
            all_day: false,
            location: Some("Berlin".into()),
            description: None,
            reminder_minutes_before: Some(10),
            reminder_dismissed_through_ms: None,
            recurrence: None,
            exception_dates: Vec::new(),
            sync: SyncMetadata {
                collection_id: Some(calendar.id.clone()),
                dirty: true,
                ..SyncMetadata::local()
            },
        };
        store.save_calendar_event(&event).expect("save event");
        let task = TaskItem {
            id: WorkspaceItemId::parse("local.task.up").expect("id"),
            title: "Hochgeladene Aufgabe".into(),
            description: Some("Notiz".into()),
            starts_at_ms: None,
            due_at_ms: Some(1_785_916_800_000),
            status: TaskStatus::InProgress,
            priority: TaskPriority::High,
            reminder_at_ms: None,
            reminder_dismissed: false,
            completed_at_ms: None,
            sync: SyncMetadata {
                collection_id: Some(calendar.id.clone()),
                dirty: true,
                ..SyncMetadata::local()
            },
        };
        store.save_task(&task).expect("save task");
        let contact = Contact {
            id: WorkspaceItemId::parse("local.contact.up").expect("id"),
            name: "Lena Fischer".into(),
            company: Some("Fischer GmbH".into()),
            job_title: None,
            emails: vec![ContactEmail {
                label: ContactLabel::Work,
                address: MailAddress::new("lena@example.org", None).expect("address"),
            }],
            phones: Vec::new(),
            addresses: Vec::new(),
            birthday: Some("1980-05-17".into()),
            website: None,
            notes: None,
            sync: SyncMetadata {
                collection_id: Some(book.id.clone()),
                dirty: true,
                ..SyncMetadata::local()
            },
        };
        store.save_contact(&contact).expect("save contact");
        let report = synchronize_account(&mut store, &account, &password)
            .await
            .expect("upload sync");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(
            report.collections.iter().map(|c| c.uploaded).sum::<u32>(),
            3
        );
        let stored_event = store
            .find_calendar_event(&event.id)
            .expect("find")
            .expect("event kept");
        assert!(!stored_event.sync.dirty);
        assert!(stored_event.sync.remote_href.is_some());
        assert!(stored_event.sync.etag.is_some());

        // A change made on the server is downloaded, a record deleted on
        // the server disappears locally.
        let event_href = stored_event.sync.remote_href.clone().expect("href");
        let changed = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:test\r\nBEGIN:VEVENT\r\n\
                       UID:local.event.up@maicenta\r\nSUMMARY:Vom Server geändert\r\n\
                       DTSTART:20260803T070000Z\r\nDTEND:20260803T080000Z\r\n\
                       RRULE:FREQ=DAILY;COUNT=3\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        client
            .put(
                &event_href,
                changed.into(),
                "text/calendar; charset=utf-8",
                None,
            )
            .await
            .expect_err("If-None-Match must refuse an existing resource");
        client
            .put(
                &event_href,
                changed.into(),
                "text/calendar; charset=utf-8",
                stored_event.sync.etag.as_deref(),
            )
            .await
            .expect("server-side change");
        let task_href = store
            .find_task(&task.id)
            .expect("find")
            .expect("task kept")
            .sync
            .remote_href
            .expect("task href");
        client
            .delete(&task_href, None)
            .await
            .expect("server-side delete");
        let report = synchronize_account(&mut store, &account, &password)
            .await
            .expect("download sync");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let stored_event = store
            .find_calendar_event(&event.id)
            .expect("find")
            .expect("event kept");
        assert_eq!(stored_event.title, "Vom Server geändert");
        assert_eq!(
            stored_event.recurrence.as_ref().map(|r| r.count),
            Some(Some(3))
        );
        assert_eq!(store.find_task(&task.id).expect("find"), None);

        // A local deletion is carried to the server.
        store.delete_contact(&contact.id).expect("delete contact");
        let report = synchronize_account(&mut store, &account, &password)
            .await
            .expect("deletion sync");
        assert_eq!(
            report
                .collections
                .iter()
                .map(|c| c.removed_remotely)
                .sum::<u32>(),
            1
        );
        let listing = client.list_resources(&book_href).await.expect("listing");
        assert_eq!(listing.resources, Vec::new());

        let _ = client.delete(&calendar_href, None).await;
        let _ = client.delete(&book_href, None).await;
    }
}
