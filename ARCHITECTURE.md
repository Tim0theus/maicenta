# MAICENTA Architecture

## Overview

MAICENTA is designed as a local-first, modular desktop workspace. A Flutter
interface provides the cross-platform user experience, while a Rust core owns
domain logic, synchronization, storage, encryption, search, and permission
enforcement.

The initial targets are Windows, macOS, and Linux. The architecture should
permit mobile clients later without weakening the desktop-first scope or
requiring a central MAICENTA service.

## Architectural principles

- Local-first and offline-capable
- No mandatory account, cloud service, or web server
- Open protocols and portable data
- Clear separation of interface, domain logic, connectors, and storage
- Modules with stable, versioned boundaries
- Least-privilege access for extensions and AI providers
- Secrets encrypted at rest; the operating-system facility protects the
  profile master key
- Explicit migrations, atomic writes, checksums, and recoverable operations

## High-level components

```text
┌─────────────────────────────────────────────────────────────┐
│                    Flutter applications                     │
│       Navigation · Views · Settings · Module controls       │
└─────────────────────────────┬───────────────────────────────┘
                              │ Versioned bridge
┌─────────────────────────────▼───────────────────────────────┐
│                         Rust core                           │
│ Domain · Sync · Storage · Search · Crypto · Permissions    │
└───────────────┬───────────────────────┬─────────────────────┘
                │                       │
┌───────────────▼──────────────┐  ┌─────▼─────────────────────┐
│ Protocol/provider connectors │  │ Local platform services   │
│ IMAP · SMTP · CalDAV ·       │  │ SQLite · Object store ·   │
│ CardDAV · future providers   │  │ Keychain · Notifications  │
└──────────────────────────────┘  └───────────────────────────┘
```

## User interface

Flutter is the interface framework for desktop and possible future mobile
clients. The interface is responsible for presentation, navigation,
accessibility, user interaction, and module settings.

Business rules, protocol behavior, secret handling, and durable storage belong
in the Rust core rather than in interface widgets. Navigation and background
services are built from the modules a user has enabled.

Server identifiers and presentation labels are deliberately separate. The
core persists and uses the exact IMAP mailbox path, such as `INBOX.Drafts`, for
SELECT, APPEND, moves, checkpoints, and UID identities. Flutter derives the
visible name from the provider-independent mailbox role and the active ARB
locale, so the same mailbox can appear as `Entwürfe` or `Drafts` without ever
renaming it on the server. Custom folders retain their provider name, with a
leading `INBOX` namespace hidden in the flat prototype folder list. The first
localization catalogue covers the folder shell in German and English; all new
interface text should be added through the same catalogue.

## Rust core

The core is responsible for:

- Accounts, identities, and settings
- Mail, calendar, task, and contact domain models
- IMAP and SMTP synchronization
- MIME and message processing
- Offline state and conflict handling
- Local search and indexing
- Encryption and profile import/export
- Module lifecycle and extension permissions
- AI provider boundaries and sensitive-action confirmation

The interface between Flutter and Rust must be explicit and versioned. Domain
types should not expose database implementation details or protocol-library
types to the interface.

The current core is split into:

- `maicenta-domain`: provider-independent identifiers, mail models, flags,
  account, calendar, task, contact, and module state
- `maicenta-application`: ports for durable mail and personal workspace storage
- `maicenta-storage`: SQLite schema migrations and transactional persistence
  for mailboxes, message summaries, flags, sanitized message bodies, and
  attachment metadata, derived mailbox counters, provider-neutral remote
  identities (IMAP UID pairs or opaque provider IDs), and a compacted mutation
  queue, plus account configuration, calendar entries, tasks, and contacts
- `maicenta-vault`: profile master-key handling, OS credential-store access,
  authenticated attachment objects, legacy plaintext migration, and portable
  password-protected profile archives
- `maicenta-rendering`: RFC 5322/MIME parsing, character-set and transfer
  decoding, body selection, and HTML5-based sanitizing under an explicit
  remote-content policy
- `maicenta-mail-connector`: async IMAP folder discovery and bounded
  multi-folder synchronization, MIME `BODYSTRUCTURE` extraction, validated
  section downloads through `BODY.PEEK`, UID-safe flag/move application, SMTP
  submission over TLS or STARTTLS, password and XOAUTH2 SASL authentication,
  shared RFC 5322 rendering of outgoing messages, plus temporary legacy
  credential migration
- `maicenta-graph-connector`: Microsoft Graph mail synchronization with
  per-folder delta cursors, immutable message IDs, synthetic MIME for the
  renderer, on-demand attachment download, mutations, drafts, and `sendMail`
- `maicenta-bridge`: generated, type-safe Flutter/Rust bindings for workspace
  snapshots, account operations, provider-dispatched synchronization, message
  submission, attachment export, and local mutations

Concrete IMAP, SMTP, vault, keychain, and Flutter-bridge implementations belong in
adapter crates outside the domain and application packages. This keeps domain
and storage use cases testable without network or platform dependencies.

At application startup, Flutter resolves the platform-specific application
support directory and asks the Rust bridge to open `maicenta.sqlite`. Rust
loads the profile identifier from a non-secret sidecar manifest, unwraps one
random 256-bit profile key through the operating-system credential store, and
caches that key only for the app process. SQLCipher authenticates and decrypts
database pages with that key. Rust then applies schema migrations, seeds
prototype records only when the profile is empty, and returns mail, calendar,
task, contact, and account DTOs. Generated bridge files are checked in so
ordinary Flutter builds do not require the code generator.

Account snapshots contain only non-secret server configuration and an OAuth
provider identifier. Passwords, app passwords, OAuth access/refresh tokens,
client IDs, scopes, token endpoints, and expiry timestamps are independent
named rows in the encrypted profile and stored tokens never cross back into a
workspace snapshot. The platform keychain contains only the profile key.

The Flutter account flow implements OAuth Authorization Code + PKCE for native
public clients and never embeds a client secret. It opens the platform's
browser/authentication session. Windows and Linux use an external browser with
a fixed localhost loopback redirect; macOS and Android use their native
authentication session and a registered app callback scheme. The flow verifies
the returned state, exchanges the code directly with the provider,
and passes the resulting token set once to Rust. Before every network operation
the bridge selects a password or OAuth credential; an expiring OAuth token is
refreshed in Rust and atomically replaced in the encrypted profile. Refresh
requests are restricted to compiled trusted Google and Microsoft token
endpoints so an imported profile cannot turn the bridge into an arbitrary POST
client. IMAP uses SASL XOAUTH2 and SMTP restricts authentication to XOAUTH2 for
OAuth accounts.

Microsoft 365/Exchange Online has two connectors. The standards connector uses
`outlook.office365.com` IMAP and `smtp.office365.com` SMTP with XOAUTH2. The
Microsoft Graph connector (`connectors/microsoft_graph`) covers tenants where
IMAP/SMTP AUTH is disabled: it discovers folders and their well-known roles,
runs per-folder delta queries whose opaque `nextLink`/`deltaLink` cursor is
persisted as the mailbox sync state, requests bodies as HTML and wraps them
with bounded inline images into a synthetic RFC 5322 message so the existing
renderer and `cid:` resolution apply unchanged, lists normal attachments by ID
for on-demand download, applies read/flag/move mutations, uploads drafts as
MIME, and sends through `sendMail`. Every request uses immutable IDs so a
message keeps its identity across folder moves. Graph change notifications
need a public webhook, so Graph accounts rely on the regular polling interval.
An account records its provider (`imap` or `microsoft_graph`); the bridge
dispatches synchronization, content download, mutations, drafts, and sending
on that field. EWS, on-premises Exchange discovery, shared/delegated mailbox
semantics, and Exchange calendar/contact data remain separate future work.
A connection test authenticates to IMAP and opens an authenticated SMTP
connection without sending a message, or reads the Graph inbox folder.

Remote message identity is provider-neutral. Storage keeps exactly one of an
IMAP `UIDVALIDITY`/`UID` pair or an opaque provider ID per cached message, per
queued mutation, and per queued draft operation; reconciliation and vanished
removal compare complete identities, so a UIDVALIDITY change or a Graph delta
removal is handled by the same code path. Mailboxes carry a separate
`remote_name` (the IMAP mailbox name or the Graph folder ID) next to their
display name, and mailbox sync states hold either the IMAP triple or a delta
cursor.
The current synchronization pass discovers selectable folders, prioritizes
standard roles, and inspects up to 25 recent message bodies from every
subscribed folder. A first UID fetch retrieves flags, headers, and
`BODYSTRUCTURE`; a
second per-message fetch requests only selected primary text/HTML sections and
bounded inline PNG, JPEG, or GIF parts. Normal attachment sections are omitted.
The connector reconstructs a conservative internal MIME envelope from the
returned header fields, verified body metadata, and fetched bytes, then passes
that envelope through the normal rendering pipeline. Remote UIDs are scoped by
account, mailbox, and UIDVALIDITY.

Later synchronization passes provide those persisted remote identities to the
connector. Once a mailbox catalogue is complete and its UIDVALIDITY is stable,
the connector compares the persisted and current UIDNEXT values and searches
only that exact new-UID range. If the server advertises CONDSTORE, SELECT also
returns HIGHESTMODSEQ and flag changes are requested with `CHANGEDSINCE` across
all known UIDs. Without CONDSTORE, a bounded recent flag refresh remains the
fallback. Previously unknown new UIDs are handled before incomplete-body
retries, with at most 25 bodies fetched per mailbox and pass.

Persistent clients start a silent synchronization after launch, poll every five
minutes while active, and synchronize again after the application resumes. For
the currently selected remote mailbox, the Flutter client issues bounded bridge
waits backed by RFC 2177 IMAP IDLE when the authenticated server advertises it.
Any unsolicited mailbox change ends the wait and triggers a silent sync; folder
changes, offline mode, suspension, connection failure, and servers without IDLE
fall back safely to bounded waits or periodic polling. If IDLE is available but
QRESYNC cannot be enabled, the changed mailbox checkpoint is marked for a full
UID reconciliation before that sync. On
the next synchronization after 15 minutes, or immediately after an absent,
incomplete, inconsistent, or UIDVALIDITY-mismatched checkpoint, the connector
performs a complete `UID SEARCH ALL` safety reconciliation. The bridge then removes rows
missing on the server or belonging to an obsolete UIDVALIDITY generation,
together with their pending operations and attachment metadata, followed by
cleanup of local attachment objects. When the server advertises QRESYNC, the
session enables it and requests `VANISHED` together with a CONDSTORE
`CHANGEDSINCE` UID fetch. Only UIDs already known in the selected mailbox and
current UIDVALIDITY generation are accepted, then removed transactionally with
their attachment metadata and local objects. If QRESYNC fails, synchronization
falls back to CONDSTORE and then to the bounded/full flag path without advancing
an unsafe checkpoint. The periodic full scan remains a defensive fallback.
If a selective body request discovers that its exact UID has already vanished,
the bridge removes that message and its attachment objects transactionally and
returns a structured missing result instead of exposing a protocol error.

SMTP emits `multipart/alternative` with a plain-text fallback and sanitized HTML.
When the user selects files, that alternative part is nested in
`multipart/mixed` with base64-safe attachment parts. Bcc recipients remain in
the SMTP envelope and are not emitted as message headers.

Before downloading incoming state, synchronization applies the account's
persistent mutation queue. Every operation selects its recorded source mailbox,
verifies UIDVALIDITY and the exact UID, and then updates `\\Seen` and
`\\Flagged` independently so unrelated server flags remain intact. Moves use
the IMAP `MOVE` capability when available. The only fallback is `COPY`,
`\\Deleted`, and UID-scoped `EXPUNGE` on servers advertising `UIDPLUS`; a
global `EXPUNGE` is deliberately forbidden. Failed operations remain queued,
are reported to the interface, and are not overwritten by the incoming pass.

Multi-account synchronization isolates failures per account. Successful
accounts commit their cache and timestamps even when another account reports a
credential or protocol error; the bridge returns those warnings alongside the
refreshed snapshot.

Removing an account deletes its configuration, all vault secrets, and cached
mail in one encrypted SQLite transaction. Other workspace modules and the
server-side mailbox remain untouched.

Mail mutations initiated by Flutter cross the same bridge and are committed by
the Rust storage adapter before the interface updates its in-memory view.
Composed messages save their summary and body atomically. Read and flagged
state, mailbox moves, custom-folder creation and renaming, and folder deletion
are durable; deletion moves contained messages to a fallback mailbox in the
same transaction. Mailbox totals and unread counts are recalculated inside the
relevant storage transaction. For downloaded IMAP messages, repeated local
changes compact into one desired-state record. The interface exposes the exact
pending-operation count, including after an application restart.

The desktop drag-and-drop layer invokes these same application operations; it
does not maintain a separate UI-only move state. Dropping a message on a folder
is currently accepted only inside the message's account, then persists the
mailbox change and queues the same safe IMAP move used by ribbon commands.
Folder drags change only the ordered local Favorites preference: dropping on
the Favorites heading appends a folder, dropping before an existing favorite
reorders it, and dropping a favorite onto its account heading removes the
shortcut without changing or deleting the underlying mailbox.

The top-level Flutter application owns the active light/dark `ThemeMode` and
provides one shared palette to the workspace, composer, and message window.
Changing the option updates the interface immediately, then writes the boolean
preference through the Rust bridge. A failed write rolls the interface back to
its previous mode rather than displaying a state that will be lost on restart.

Rich message composition uses a structured Quill Delta document in the UI.
Delta remains the editable draft representation. A conservative converter maps
the visible font, size, color, emphasis, alignment, list, indentation, and link
attributes to email HTML. The Rust core sanitizes that HTML again before both
local persistence and SMTP MIME construction. The plain-text representation is
always included as a fallback. Locally saved drafts persist their raw To/Cc/Bcc
fields and serialized Delta separately from the sanitized reading copy. Opening
a draft restores that state in the composer; retained attachment object IDs are
validated against the same message and can be submitted without exposing local
object paths to Flutter. Draft creates, replacements, and removals enter an
encrypted persistent queue. Online composition drains that queue immediately;
normal synchronization retries it before cataloguing incoming mail. IMAP
`APPEND` uses `\\Draft` plus a stable generated Message-ID, then resolves the
new UID inside the exact mailbox. Edits remove the previous UID/UIDVALIDITY
identity before appending the successor. Removal requires UIDPLUS so the client
can issue UID EXPUNGE and never risk a mailbox-wide `EXPUNGE`. Fully fetched
server drafts without deferred attachments receive local editable metadata
without queuing an unchanged upload.

Desktop attachments are selected through the operating system's native file
dialog or dropped directly from the desktop file manager onto the composer.
On macOS, the drop layer keeps a security-scoped resource active until sending
or local draft persistence has finished, then releases it. The bridge
revalidates that each path names a regular file, derives a
conservative media type, rejects control characters in file names, and limits a
message to ten files and 18 MiB of raw attachment data. SMTP submission reads
the selected bytes immediately before MIME construction. Local persistence
writes each attachment to a temporary file, syncs it, and atomically renames it
below the active profile's sibling `maicenta.objects/attachments` directory.
SQLite stores the attachment ID, message relationship, display metadata, size,
and a validated profile-relative object key. The message, body, and complete
metadata set are committed together; newly written objects are removed if that
transaction fails. The reading pane exports a stored object only to a path
chosen through the native save dialog. The macOS sandbox therefore grants
read/write access only to user-selected files. During normal IMAP
synchronization, attachment bytes are not requested; sanitized `BODYSTRUCTURE`
metadata is stored without a local object key and the reading pane marks each
file as server-backed. An explicit Save As action selects the recorded mailbox,
verifies UIDVALIDITY and the exact UID, issues `BODY.PEEK[section]`, bounds the
encoded response to 128 MiB and the decoded file to 100 MiB, decodes its
transfer encoding in Rust, and writes only to the user-selected path. The
complete-MIME cache path remains for local/imported messages and legacy
records. Safe inline raster resources remain part of the rendering pipeline
rather than the attachment download list.

Selective display synchronization limits message headers to 256 KiB, primary
text/HTML to four parts, 5 MiB per part, and 10 MiB total, and encoded inline
raster data to 20 parts, 3 MiB per part, and 7 MiB total. The renderer applies
its stricter decoded image limits afterwards. Missing or excessive declared
display parts are omitted and surfaced as a synchronization warning; the
connector does not silently fall back to transferring the full RFC822 message.
Detailed transfer progress, cancellation, and resumable downloads remain
planned.

### Message rendering

The reading pane must not inherit Outlook's historical rendering engine or its
HTML quirks. Complete local/imported messages and the connector's selectively
reconstructed internal MIME envelopes pass through `maicenta-rendering` before
UI code sees them:

1. Parse RFC 5322 and MIME structure, transfer encodings, and declared
   character sets.
2. Select the appropriate `multipart/alternative` body and retain a plain-text
   fallback.
3. Parse HTML using HTML5 rules and remove scripts, event handlers, unsafe URL
   schemes, frames, forms, and dangerous CSS.
4. Preserve common table layout, typography, and safe inline styles used by
   real-world HTML email.
5. Block HTTP(S) images by default and report the number blocked. Resolve
   `cid:` references exclusively against validated MIME inline resources from
   the same message.
6. Render the sanitized fragment in an isolated viewer with no scripting,
   storage, navigation, or implicit network privileges.

Inline resolution accepts only MIME-declared PNG, JPEG, and GIF parts with a
matching raster signature and bounded dimensions. Each image is limited to
2 MiB, each message to 20 inline images and 5 MiB, and decoded dimensions to
4096 pixels per side and 16 megapixels. Matching is based on normalized,
percent-decoded Content-IDs. The core converts accepted resources to internal
data URLs after MIME decoding; Flutter renders those bytes as in-memory images.
Sender-authored data URLs, SVG, unsupported raster formats, unresolved CIDs,
and excessive images are removed by the sanitizer.

“Standards-oriented” does not imply pixel-identical output across every mail
client: email HTML has no single complete layout profile and many senders rely
on client-specific CSS. The goal is predictable HTML5 behavior, graceful
fallbacks, and security rather than emulating Outlook rendering defects.

## Storage

### Structured data

SQLite stores structured local state such as:

- Message metadata, threads, folders, and labels
- Synchronization checkpoints
- Calendar events and tasks
- Contacts
- Module and extension configuration
- Permission grants
- Search indexes

Schema migrations are versioned and tested. Writes that span related data must
be transactional.

Schema version 2 adds non-secret mail-account configuration, calendar events,
tasks, and contacts.

Schema version 3 adds the remote account/mailbox/UIDVALIDITY/UID identity for
cached messages and the persistent, per-message IMAP mutation queue. Queue rows
reference cached messages with cascading deletion, so removing an account also
removes its pending local operations atomically.

Schema version 4 adds attachment metadata related to cached messages. Object
keys are relative to the profile object root and are validated before every
read, write, export, or removal. Deleting an account removes its attachment
metadata transactionally and its locally cached object files afterwards.

Schema version 5 makes the attachment object key optional and adds a validated
IMAP section path plus transfer encoding. A row must reference either a local
object or a complete server-section pair. The migration preserves all v4
object-backed records and is covered by a profile-upgrade test.

Schema version 6 records whether the bounded display body for a remote message
was fetched completely. Existing cached rows default to complete during the
migration; new incomplete rows remain eligible for a later bounded retry while
their header, flags, and available body content stay usable offline.

Schema version 7 adds composition metadata for locally editable drafts:
To/Cc/Bcc fields and the serialized Quill Delta. Existing `local.*` messages
carrying the Draft flag receive an empty editable record during migration so
their recoverable body and attachments can be reopened instead of remaining a
read-only message.

Schema version 8 adds account secrets inside the encrypted database. On the
first encrypted start, legacy per-account credentials are read once from the
platform keychain, committed to this table, and removed from their old entries
where possible.

Schema version 9 adds a profile-wide FTS5 index for sender names and addresses,
recipients, subjects, previews, and cached message bodies. Migration backfills
existing messages, while database triggers keep later message, body, and local
draft changes synchronized transactionally. Search terms are normalized into
bounded prefix queries before they reach FTS5. Because the virtual table lives
in the same SQLCipher database, its terms and duplicated body text remain
encrypted at rest; no plaintext sidecar index is created.

Schema version 10 separates compact recipient metadata from draft editing
state and rebuilds the FTS5 index with prefix indexes plus field-aware ranking.
Routine search covers subject, sender address/name, and To/Cc/Bcc values.
Preview, cached body text, and attachment filenames form an explicit broader
scope. Subject matches receive the greatest BM25 weight, followed by addresses
and participant names; content is deliberately weighted below metadata.
Existing remote rows are marked for a bounded one-time header refresh because
profiles created before version 10 did not retain incoming To/Cc/Bcc values.
That refresh preserves already cached bodies, previews, and local attachment
objects.

Schema version 11 adds one encrypted incremental state row per account and
remote mailbox. It records UIDVALIDITY, optional UIDNEXT and HIGHESTMODSEQ,
whether compact catalogue backfill is complete, and the last successful full
reconciliation time. HIGHESTMODSEQ is stored as decimal text so the complete
unsigned 64-bit IMAP range remains portable across SQLite's signed integer
representation. Account deletion removes these rows through a cascading
foreign key.

Schema version 12 adds the durable IMAP draft-operation queue. Each row records
an upsert or deletion, the target drafts mailbox, and the complete previous
UID/UIDVALIDITY identity when one exists. Queue completion atomically replaces
or removes the `remote_messages` identity, so subsequent incremental sync sees
the uploaded local draft as the same cached message instead of creating a
duplicate.

Schema version 13 adds ordered workspace preferences. The first preferences
are the exact list of favorite mailbox IDs and the explicit dark-mode choice.
Unknown and duplicate mailbox IDs are rejected on write, stale IDs are filtered
from snapshots, and an explicitly empty list remains distinct from an
uninitialized profile so removing every favorite survives an application
restart. The table lives inside the same SQLCipher profile and is included in
encrypted profile export/import.

IMAP synchronization progressively adds up to 250 previously unknown header
and `BODYSTRUCTURE` records per subscribed selectable mailbox and pass while
retaining the existing limit of 25 recent or explicitly retried message bodies.
There is no fixed folder-count cutoff. The bridge returns the combined number
of catalogue entries still missing, and the desktop client immediately starts
another bounded pass while that value decreases. Persisted account, mailbox,
UIDVALIDITY, and UID rows make each pass resume newest-to-oldest instead of
starting over. Header-only rows record that no body was requested, so this
complete compact catalogue does not silently become an unlimited body cache.
New UIDs are handled before failed-body retries. The catalogue is encrypted
with the rest of the SQLCipher profile and may be rebuilt independently on each
device. Opening a header-only result may request its body explicitly: the
connector verifies UIDVALIDITY and the exact UID, fetches bounded display
sections with `BODY.PEEK`, and persists the sanitized result without fetching
attachments.

Workspace snapshots contain only the newest 100 locally catalogued messages
per mailbox. Older summaries are queried directly from encrypted SQLite with a
bounded `LIMIT`/`OFFSET` page through the Rust bridge. This is a presentation
and memory boundary only: mailbox counts and profile-wide search operate over
the complete local catalogue.

### Object storage

Large or immutable content is stored separately from SQLite. The first
implemented object type is the ID-addressed attachment copy of a locally
composed, imported, or legacy fully cached incoming message. Normal selective
IMAP synchronization stores only attachment metadata in SQLite. The object
directory sits beside the active database and is ignored by Git. Every local
object is encrypted with XChaCha20-Poly1305 using an object-specific key derived
from the profile key and its validated relative object key. Future object
types include original message sources, inline images, and local archives.
Content addressing, integrity checks, and deduplication remain planned
refinements.

An active SQLite database must never be synchronized directly between devices.
Future profile synchronization should use encrypted individual objects,
manifests, change journals, checksums, and snapshots.

### Secrets

Passwords, OAuth tokens, API keys, private keys, and recovery keys must not be
stored in the repository or in plaintext files. Account secrets are stored as
logical profile records inside the encrypted database. Only the random profile
master key is stored through the platform facility:

- Windows Credential Manager
- macOS Keychain
- Linux Secret Service

The active database uses SQLCipher, including its WAL. Local objects remain
individually authenticated and encrypted. A complete `.maicenta-profile`
archive contains those encrypted files and an encrypted manifest; Argon2id
derives a key from the user-supplied export password and XChaCha20-Poly1305
wraps the profile key. Import extracts to a private staging directory, verifies
the database and password before replacement, and retains rollback copies until
the new platform key entry has been installed.

Encryption at rest does not protect data from malware running as the user while
the profile is unlocked. Losing both the platform key and every protected
export makes the profile unrecoverable.

## Connectors and standards

External services are accessed through replaceable adapters rather than being
embedded directly in domain logic.

| Area | Initial standards |
| --- | --- |
| Mail | IMAP, SMTP, RFC 5322, MIME |
| Calendar | iCalendar, CalDAV |
| Tasks | VTODO, CalDAV |
| Contacts | vCard, CardDAV |
| Future mail | JMAP |

Provider-specific integrations, including Microsoft services, may be added
behind the same connector boundaries after the open-protocol mail MVP is
stable.

Connectors translate external data into canonical domain models while
preserving original formats whenever practical. Network failures, retries,
rate limits, authentication expiry, and partial synchronization are represented
explicitly.

## Modules

Mail, Calendar, Tasks, Contacts, Notes, Vault, Assistant, and Extensions are
separate modules with declared dependencies.

When a module is disabled:

- It is removed from navigation.
- Unnecessary background work stops.
- Unneeded permissions are not requested.
- Its data is retained by default.
- It can be enabled again later.

Uninstalling a module is a separate action and must ask whether associated data
should also be removed.

## Extensions

Extensions do not receive direct database access. They use a stable, versioned
API and declare explicit permissions. A future runtime should provide:

- Sandboxed execution, preferably through WebAssembly
- Memory and execution-time limits
- Controlled network access
- Dedicated extension storage
- Declarative interface components
- Package signing for trusted distribution channels

Sensitive operations such as sending email, deleting data, changing calendar
events, or transferring data externally require confirmation by default.

## AI providers

AI is an optional provider capability, not a dependency of the core workspace.
Local models, user-supplied API credentials, and external providers should use
the same replaceable boundary.

Access is granular by account, folder, item, data type, and operation. Reading
and writing are authorized separately. Sending, deleting, or transmitting
sensitive data requires additional confirmation, and access should be
auditable.

## Security boundaries

- Sanitize untrusted HTML email.
- Control remote images and other remote message content.
- Validate message and attachment inputs.
- Keep plugins and AI providers behind permission checks.
- Use least privilege for filesystem and network access.
- Encrypt exported profiles.
- Preserve an independent recovery mechanism.
- Avoid telemetry unless users explicitly opt in.
- Keep personal data exportable at all times.

## Suggested repository layout

```text
apps/
  client_flutter/       Flutter desktop interface
core/
  application/          Use-case ports and adapter boundaries
  bridge/               Flutter bridge API and generated Rust bindings
  domain/               Domain models and rules
  rendering/            MIME decoding and safe HTML preparation
  storage/              SQLite and object storage
  sync/                 Synchronization engine
  vault/                Encryption, key management, and profile archives
  search/               Local search
  plugins/              Extension runtime and permissions
connectors/
  mail/                 IMAP, SMTP, MIME rendering, and legacy credential migration
  microsoft_graph/      Microsoft Graph mail connector for Exchange Online
  caldav/
  carddav/
platform/
  windows/
  macos/
  linux/
schemas/                 Versioned data and extension schemas
tests/                   Unit, integration, and protocol tests
docs/                    Additional design documentation
```

This layout is directional. Component boundaries and ownership matter more than
the final directory names.

## Architectural decisions

Significant changes to storage formats, security boundaries, protocols, module
interfaces, licensing assumptions, or server requirements should be recorded
as explicit architectural decisions before implementation.
