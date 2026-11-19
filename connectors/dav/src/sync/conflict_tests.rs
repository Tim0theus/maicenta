//! Offline regression tests: every request goes to an ephemeral loopback server.
use std::{
    fmt::Write as _,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use maicenta_application::{DavStore, WorkspaceStore};
use maicenta_domain::{AccountId, CollectionKind, DavAccount, DavCollection, SyncMetadata};
use maicenta_storage::SqliteMailStore;

use super::{LocalRecord, collection_id_for, record_from_resource, synchronize_account};

const EVENT: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:item\r\nSUMMARY:Local edit\r\nDTSTART:20260803T070000Z\r\nDTEND:20260803T080000Z\r\nDESCRIPTION:Private synthetic notes\r\nRRULE:FREQ=WEEKLY;COUNT=4\r\nEXDATE:20260810T070000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
const TASK: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTODO\r\nUID:item\r\nSUMMARY:Local edit\r\nDESCRIPTION:Synthetic task notes\r\nDUE:20260804T070000Z\r\nPRIORITY:1\r\nSTATUS:IN-PROCESS\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
const CONTACT: &str = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:item\r\nFN:Local edit\r\nORG:Synthetic Company\r\nTITLE:Tester\r\nEMAIL;TYPE=WORK:synthetic@example.invalid\r\nEMAIL;TYPE=HOME:home@example.invalid\r\nTEL;TYPE=CELL:+12025550123\r\nADR;TYPE=HOME:;;Test Street;Test City;;12345;Test Country\r\nNOTE:Private synthetic notes\r\nEND:VCARD\r\n";

struct MockDav {
    url: String,
    // 0: listing fails, 1: multiget fails, 2: success, 3: unchanged ETag,
    // 4: malformed object, 5: vanished resource, 6: discovery fails.
    mode: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

fn multistatus(href: &str, props: &str) -> String {
    format!(
        r#"<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav" xmlns:A="urn:ietf:params:xml:ns:carddav"><D:response><D:href>{href}</D:href><D:propstat><D:prop>{props}</D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response></D:multistatus>"#
    )
}

fn read_request(stream: &mut TcpStream) -> String {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let read = stream.read(&mut buffer).unwrap();
        assert_ne!(read, 0, "request ended prematurely");
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..end]);
            let length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            if bytes.len() >= end + 4 + length {
                break;
            }
        }
    }
    String::from_utf8(bytes).unwrap()
}

impl MockDav {
    fn new(kind: CollectionKind, data: &str, initial_mode: usize) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let mode = Arc::new(AtomicUsize::new(initial_mode));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (worker_mode, worker_requests, worker_stop) =
            (mode.clone(), requests.clone(), stop.clone());
        let data = data.replace("Local edit", "Server edit");
        let worker = thread::spawn(move || {
            while !worker_stop.load(Ordering::SeqCst) {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("accept: {error}"),
                };
                let request = read_request(&mut stream);
                let first = request.lines().next().unwrap().to_owned();
                worker_requests.lock().unwrap().push(request);
                let mode = worker_mode.load(Ordering::SeqCst);
                // A truncated response exercises reqwest's connection-error
                // path, independently of HTTP error status handling.
                if mode == 7 && first.starts_with("REPORT ") {
                    stream.write_all(b"HTTP/1.1 207 Test\r\nContent-Length: 100\r\nConnection: close\r\n\r\ntruncated").unwrap();
                    continue;
                }

                let (status, body) = if first.starts_with("PUT ") {
                    (412, String::new())
                } else if mode == 6
                    || (mode == 0 && first.starts_with("PROPFIND /collection/ "))
                    || (mode == 1 && first.starts_with("REPORT "))
                {
                    (503, "injected outage".into())
                } else if first.starts_with("PROPFIND / ") {
                    (
                        207,
                        multistatus(
                            "/",
                            "<C:calendar-home-set><D:href>/home/</D:href></C:calendar-home-set>",
                        ),
                    )
                } else if first.starts_with("PROPFIND /home/ ") {
                    let props = match kind {
                        CollectionKind::AddressBook => {
                            "<D:resourcetype><A:addressbook/></D:resourcetype>"
                        }
                        CollectionKind::Calendar | CollectionKind::TaskList => {
                            "<D:resourcetype><C:calendar/></D:resourcetype>"
                        }
                    };
                    (
                        207,
                        multistatus(
                            "/collection/",
                            &format!("{props}<D:displayname>Synthetic collection</D:displayname>"),
                        ),
                    )
                } else if mode == 5 {
                    (207, "<D:multistatus xmlns:D=\"DAV:\"/>".into())
                } else {
                    let etag = if mode == 3 { "old" } else { "new" };
                    let mut props = format!("<D:getetag>&quot;{etag}&quot;</D:getetag>");
                    if first.starts_with("REPORT ") {
                        let tag = if kind == CollectionKind::AddressBook {
                            "A:address-data"
                        } else {
                            "C:calendar-data"
                        };
                        let body = if mode == 4 { "invalid object" } else { &data };
                        write!(props, "<{tag}><![CDATA[{body}]]></{tag}>").unwrap();
                    }
                    (207, multistatus(resource_href(kind), &props))
                };
                let response = format!(
                    "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: application/xml\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        Self {
            url,
            mode,
            requests,
            stop,
            worker: Some(worker),
        }
    }

    fn put_count(&self) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request.starts_with("PUT "))
            .count()
    }
}

impl Drop for MockDav {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let result = self.worker.take().unwrap().join();
        if !thread::panicking() {
            result.unwrap();
        }
    }
}

fn resource_href(kind: CollectionKind) -> &'static str {
    if kind == CollectionKind::AddressBook {
        "/collection/item.vcf"
    } else {
        "/collection/item.ics"
    }
}

fn seed(
    store: &mut SqliteMailStore,
    server: &MockDav,
    kind: CollectionKind,
    data: &str,
) -> (DavAccount, DavCollection) {
    let account = DavAccount {
        id: AccountId::parse("synthetic").unwrap(),
        display_name: "Synthetic account".into(),
        server_url: server.url.clone(),
        username: "synthetic".into(),
        last_sync_at_ms: None,
    };
    store.save_dav_account(&account).unwrap();
    let collection = DavCollection {
        id: collection_id_for(&account.id, "/collection/"),
        account_id: account.id.clone(),
        kind,
        href: "/collection/".into(),
        display_name: "Synthetic collection".into(),
        color: None,
        ctag: None,
        enabled: true,
        position: 0,
    };
    store.save_dav_collection(&collection).unwrap();
    let mut record = record_from_resource(
        &collection,
        resource_href(kind),
        Some("\"old\"".into()),
        data,
        None,
    )
    .unwrap();
    match &mut record {
        LocalRecord::Event(event) => {
            event.reminder_minutes_before = Some(15);
            event.reminder_dismissed_through_ms = Some(event.starts_at_ms);
            event.location = Some("Synthetic location".into());
        }
        LocalRecord::Task(task) => {
            task.starts_at_ms = Some(1_785_738_000_000);
            task.reminder_at_ms = Some(1_785_737_000_000);
            task.reminder_dismissed = true;
        }
        LocalRecord::Contact(contact) => {
            contact.birthday = Some("1980-01-01".into());
            contact.website = Some("https://example.invalid".into());
        }
    }
    record.sync_mut().dirty = true;
    record.save(store).unwrap();
    (account, collection)
}

fn recovery_copies(store: &SqliteMailStore) -> Vec<LocalRecord> {
    let mut records = Vec::new();
    records.extend(
        store
            .list_calendar_events()
            .unwrap()
            .into_iter()
            .map(LocalRecord::Event),
    );
    records.extend(
        store
            .list_tasks()
            .unwrap()
            .into_iter()
            .map(LocalRecord::Task),
    );
    records.extend(
        store
            .list_contacts()
            .unwrap()
            .into_iter()
            .map(LocalRecord::Contact),
    );
    records
        .into_iter()
        .filter(|record| record.sync() == &SyncMetadata::local())
        .collect()
}

#[tokio::test]
async fn preserves_conflict_before_listing_failure() {
    for (kind, data) in [
        (CollectionKind::Calendar, EVENT),
        (CollectionKind::TaskList, TASK),
        (CollectionKind::AddressBook, CONTACT),
    ] {
        let server = MockDav::new(kind, data, 0);
        let mut store = SqliteMailStore::open_in_memory().unwrap();
        let (account, _) = seed(&mut store, &server, kind, data);
        let report = synchronize_account(&mut store, &account, "synthetic-password")
            .await
            .unwrap();
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("503"))
        );
        assert_eq!(
            recovery_copies(&store).len(),
            1,
            "a 412 must durably preserve the local record before any pull"
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("Konfliktkopie")),
            "the account report must retain the conflict warning alongside the outage"
        );
        assert_eq!(server.put_count(), 1);
    }
}

struct TemporaryProfile(std::path::PathBuf);

impl TemporaryProfile {
    fn new() -> Self {
        static SERIAL: AtomicUsize = AtomicUsize::new(0);
        let serial = SERIAL.fetch_add(1, Ordering::SeqCst);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self(std::env::temp_dir().join(format!(
            "maicenta-dav-conflict-{}-{nanos}-{serial}.sqlite",
            std::process::id()
        )))
    }
}

impl Drop for TemporaryProfile {
    fn drop(&mut self) {
        for suffix in ["", "-shm", "-wal"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.0.display()));
        }
    }
}

fn assert_recovery_matches(mut recovery: LocalRecord, original: &LocalRecord) {
    assert_eq!(recovery.sync(), &SyncMetadata::local());
    assert_eq!(
        recovery.title(),
        format!("{} (DAV-Konfliktkopie)", original.title())
    );
    match (&mut recovery, original) {
        (LocalRecord::Event(copy), LocalRecord::Event(source)) => {
            copy.id = source.id.clone();
            copy.title.clone_from(&source.title);
        }
        (LocalRecord::Task(copy), LocalRecord::Task(source)) => {
            copy.id = source.id.clone();
            copy.title.clone_from(&source.title);
        }
        (LocalRecord::Contact(copy), LocalRecord::Contact(source)) => {
            copy.id = source.id.clone();
            copy.name.clone_from(&source.name);
        }
        _ => panic!("recovery changed the record kind"),
    }
    *recovery.sync_mut() = original.sync().clone();
    assert_eq!(
        &recovery, original,
        "all fields, including recurrence, reminders and contact details, must survive"
    );
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn retries_after_restart_without_duplicate_copies_or_uploads() {
    for (kind, data) in [
        (CollectionKind::Calendar, EVENT),
        (CollectionKind::TaskList, TASK),
        (CollectionKind::AddressBook, CONTACT),
    ] {
        for failure in [0, 1, 4, 7] {
            let server = MockDav::new(kind, data, failure);
            let path = TemporaryProfile::new();
            let mut store = SqliteMailStore::open(&path.0).unwrap();
            let (account, collection) = seed(&mut store, &server, kind, data);
            let original = super::local_records(&store, &collection)
                .unwrap()
                .pop()
                .unwrap();
            let first = synchronize_account(&mut store, &account, "synthetic-password")
                .await
                .unwrap();
            assert!(
                first
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("Konfliktkopie"))
            );
            let copy = recovery_copies(&store).pop().unwrap();
            let recovery_id = copy.id().clone();
            assert_recovery_matches(copy, &original);
            drop(store);

            let mut store = SqliteMailStore::open(&path.0).unwrap();
            let retry = synchronize_account(&mut store, &account, "synthetic-password")
                .await
                .unwrap();
            assert!(
                retry
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("Konfliktkopie"))
            );
            assert_eq!(recovery_copies(&store).len(), 1);
            assert_eq!(recovery_copies(&store)[0].id(), &recovery_id);
            assert_eq!(
                server.put_count(),
                1,
                "the rejected write must not be retried"
            );

            // Even a stale listing with the old ETag must download the winner.
            server.mode.store(3, Ordering::SeqCst);
            let success = synchronize_account(&mut store, &account, "synthetic-password")
                .await
                .unwrap();
            assert_eq!(success.collections[0].downloaded, 1);
            assert!(
                success
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("Konfliktkopie"))
            );
            let active = super::local_records(&store, &collection).unwrap();
            assert_eq!(active.len(), 1);
            assert_eq!(active[0].id(), original.id());
            assert_eq!(active[0].title(), "Server edit");
            assert!(!active[0].sync().dirty);
            assert_recovery_matches(recovery_copies(&store).pop().unwrap(), &original);
            let again = synchronize_account(&mut store, &account, "synthetic-password")
                .await
                .unwrap();
            assert_eq!(again.collections[0].downloaded, 0);
            assert_eq!(server.put_count(), 1);
            let requests = server.requests.lock().unwrap();
            let put = requests
                .iter()
                .find(|request| request.starts_with("PUT "))
                .unwrap()
                .to_ascii_lowercase();
            assert!(put.contains("if-match: \"old\""));
            assert!(!put.contains("if-none-match:"));
        }
    }
}

#[tokio::test]
async fn preserves_create_collision_and_new_edit_as_separate_copies() {
    for (kind, data) in [
        (CollectionKind::Calendar, EVENT),
        (CollectionKind::TaskList, TASK),
        (CollectionKind::AddressBook, CONTACT),
    ] {
        let server = MockDav::new(kind, data, 0);
        let mut store = SqliteMailStore::open_in_memory().unwrap();
        let (account, collection) = seed(&mut store, &server, kind, data);
        let mut original = super::local_records(&store, &collection)
            .unwrap()
            .pop()
            .unwrap();
        original.sync_mut().remote_href = None;
        original.sync_mut().etag = None;
        original.save(&mut store).unwrap();
        synchronize_account(&mut store, &account, "synthetic-password")
            .await
            .unwrap();
        let first_copy = recovery_copies(&store).pop().unwrap();
        assert_recovery_matches(first_copy.clone(), &original);
        let mut edited = super::local_records(&store, &collection)
            .unwrap()
            .pop()
            .unwrap();
        match &mut edited {
            LocalRecord::Event(item) => item.description = Some("another edit".into()),
            LocalRecord::Task(item) => item.description = Some("another edit".into()),
            LocalRecord::Contact(item) => item.notes = Some("another edit".into()),
        }
        edited.sync_mut().dirty = true;
        edited.save(&mut store).unwrap();
        synchronize_account(&mut store, &account, "synthetic-password")
            .await
            .unwrap();
        let copies = recovery_copies(&store);
        assert_eq!(copies.len(), 2);
        assert!(
            copies.contains(&first_copy),
            "a later conflict must never overwrite an older recovery copy"
        );
        assert_recovery_matches(
            copies
                .into_iter()
                .find(|copy| copy.id() != first_copy.id())
                .unwrap(),
            &edited,
        );
        server.mode.store(2, Ordering::SeqCst);
        synchronize_account(&mut store, &account, "synthetic-password")
            .await
            .unwrap();
        let active = super::local_records(&store, &collection).unwrap();
        assert_eq!(
            active.len(),
            1,
            "create collision must retain a single server record"
        );
        assert_eq!(active[0].id(), original.id());
        assert_eq!(active[0].title(), "Server edit");
        assert_eq!(server.put_count(), 2);
        assert!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|r| r.starts_with("PUT "))
                .all(|r| r.to_ascii_lowercase().contains("if-none-match: *"))
        );
    }
}

#[tokio::test]
async fn recovery_survives_discovery_failure_remote_removal_and_account_deletion() {
    for (kind, data) in [
        (CollectionKind::Calendar, EVENT),
        (CollectionKind::TaskList, TASK),
        (CollectionKind::AddressBook, CONTACT),
    ] {
        let server = MockDav::new(kind, data, 0);
        let mut store = SqliteMailStore::open_in_memory().unwrap();
        let (account, collection) = seed(&mut store, &server, kind, data);
        synchronize_account(&mut store, &account, "synthetic-password")
            .await
            .unwrap();
        server.mode.store(6, Ordering::SeqCst);
        assert!(
            synchronize_account(&mut store, &account, "synthetic-password")
                .await
                .is_err()
        );
        assert_eq!(super::conflict_warnings(&store, None).unwrap().len(), 1);
        server.mode.store(5, Ordering::SeqCst);
        synchronize_account(&mut store, &account, "synthetic-password")
            .await
            .unwrap();
        assert_eq!(
            super::local_records(&store, &collection).unwrap(),
            Vec::new()
        );
        assert_eq!(recovery_copies(&store).len(), 1);
        assert_eq!(
            store.list_remote_deletions(&collection.id).unwrap(),
            Vec::new()
        );
        store.delete_dav_account(&account.id).unwrap();
        assert_eq!(super::conflict_warnings(&store, None).unwrap().len(), 1);
        let copy = recovery_copies(&store).pop().unwrap();
        copy.delete(&mut store).unwrap();
        assert_eq!(
            super::conflict_warnings(&store, None).unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(server.put_count(), 1);
        assert!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .all(|r| !r.starts_with("DELETE "))
        );
    }
}
