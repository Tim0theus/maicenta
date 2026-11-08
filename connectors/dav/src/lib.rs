//! CalDAV/CardDAV connector for MAICENTA.
//!
//! The crate maps calendar events and tasks to iCalendar, contacts to
//! vCard, talks `WebDAV` with `PROPFIND`/`REPORT`/`PUT`/`DELETE`, and
//! reconciles each collection in both directions using entity tags.

pub mod client;
pub mod ical;
pub mod sync;
pub mod text;
pub mod vcard;

pub use client::{DavClient, DavError, DiscoveredCollection};
pub use sync::{CollectionReport, SyncReport, synchronize_account};
