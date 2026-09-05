# MAICENTA Roadmap

This roadmap describes the current direction of MAICENTA. It is a planning
document, not a promise of specific features or release dates. Priorities may
change as the project evolves.

## Current status

MAICENTA has a runnable desktop alpha with SQLite-backed mail and personal
workspace data, real password- and OAuth-based IMAP/SMTP connectivity, a
Microsoft Graph connector for Exchange Online tenants without IMAP, a rich-text
composer, and a standards-oriented safe rendering foundation. Account sync is
not production-ready and there is no stable release yet.

## Guiding priorities

- Build a reliable desktop foundation before expanding the feature set.
- Keep personal data local and exportable.
- Avoid mandatory accounts, cloud services, and web servers.
- Prefer open protocols and replaceable provider integrations.
- Treat security, offline use, and profile recovery as core features.
- Design features as independently controllable modules.
- Introduce extensions and AI only after the underlying workspace is stable.

## Phase 0: Project foundation

- [x] Initialize the monorepo and development environment.
- [x] Establish contribution, security, and coding guidelines.
- [x] Scaffold the Flutter application for Windows, macOS, and Linux.
- [x] Connect the Flutter interface to the Rust core.
- [x] Define initial domain, storage, and connector interfaces.
- [x] Add automated tests and continuous integration.

## Phase 1: Desktop mail MVP

The first usable release will focus on a stable, local desktop mail client.

- [x] Replace the initial web-like workspace shell with a dense Outlook
  Classic-inspired desktop hierarchy: quick-access title area, grouped ribbon,
  Favorites/account navigation, compact message list, reading pane, bottom
  module selector, and connection status bar.
- [x] Align the message composer with the classic desktop window: functional
  File/Message/Format/Insert/Options ribbon tabs, prominent Send command,
  compact address header, continuous editor, attachment strip, and quick draft
  saving.
- [x] Open messages on double-click in a dedicated classic desktop window with
  detailed envelope fields, safe HTML, attachments, reply-all, organization,
  marking, and view/zoom commands.
- [x] Add desktop drag and drop for moving messages within an account, adding,
  reordering, and removing encrypted-profile Favorites, and dropping native
  files into the composer as validated attachments.
- [x] Add an Outlook-style message context menu for reply and forward actions,
  read and follow-up state, account-local moves, archive, trash, spam, and
  not-spam handling.
- [x] Add a profile-persisted light/dark desktop theme and open editable drafts
  directly in the composer on double-click without an intermediate window.

### Accounts and synchronization

- [x] Configure one or multiple IMAP and SMTP accounts.
- [x] Add, edit, and remove accounts locally and select the SMTP sender while
  composing.
- [x] Discover subscribed remote folders, catalogue every selectable folder in
  resumable newest-first batches, and automatically continue until its compact
  metadata history is locally available.
- [x] Encrypt each complete local profile with a random master key protected by
  the operating-system credential store; keep account passwords inside the
  encrypted profile for portable migration.
- [x] Provide a local offline cache.
- [x] Persist and upload read state, flags, mailbox moves, and moves to trash
  using UIDVALIDITY checks plus `MOVE` or a safe `UIDPLUS` fallback.
- [x] Reuse cached remote identities during later synchronization passes:
  refresh flags without re-downloading bodies, fetch new UIDs in bounded
  catch-up batches, retry incomplete bodies, and reconcile deletions or changed
  UIDVALIDITY snapshots transactionally.
- [x] Page older locally catalogued messages into the desktop list instead of
  limiting a folder to one fixed workspace snapshot.
- [x] Persist UIDVALIDITY, UIDNEXT, HIGHESTMODSEQ, catalogue completion, and the
  last full reconciliation per mailbox. Use UIDNEXT ranges for new mail and
  CONDSTORE `CHANGEDSINCE` flag deltas when supported, with a periodic complete
  UID safety reconciliation and automatic fallback on inconsistent state.
- [x] Use QRESYNC `VANISHED` deletion deltas when supported, filter them against
  known UIDs in the current UIDVALIDITY generation, and remove matching local
  messages and attachment objects transactionally.
- [x] Run silent IMAP synchronization at startup, every five minutes while the
  client is active, and after resume; immediately discard a stale local entry
  when an on-demand body request confirms that its UID vanished.
- [x] Use bounded RFC 2177 IMAP IDLE waits for the currently visible remote
  mailbox, trigger a silent sync on server notifications, and retain polling
  plus targeted full reconciliation when IDLE or QRESYNC is unavailable.
- [x] Add Authorization Code + PKCE sign-in, encrypted refresh-token storage,
  automatic token refresh, and IMAP/SMTP XOAUTH2 for Microsoft 365/Exchange
  Online and Google, with provider-specific setup guidance.
- [x] Generalize remote message identity from IMAP UIDs to provider IDs so
  API-based connectors share the same storage, mutation queue, and draft
  lifecycle as IMAP.
- [x] Add Microsoft Graph mail as a separate connector for Exchange Online
  tenants where IMAP/SMTP AUTH is disabled: per-folder delta synchronization,
  immutable message IDs, bounded HTML bodies with inline images, on-demand
  attachments, read/flag/move mutations, server drafts, and sending.
- [x] Replace the protocol-first account dialog with an e-mail-first flow:
  probe public signals (Entra ID tenant discovery, MX records, Google hosting,
  IMAP autodiscovery, `_autodiscover` SRV), preselect the recommended method,
  keep every alternative one click away, and move protocol vocabulary into an
  "Erweitert" section.
- [x] Show nested folders as a collapsible tree with a persisted collapsed
  state for folders, the Favorites section, and each account group; indicate
  running synchronization and remaining catalogue work in the status bar.
- [ ] Add on-premises Exchange/EWS discovery and an explicit support policy.
  Exchange Online retires EWS in October 2026, so EWS is relevant only for
  on-premises servers.
- [x] Upload new and edited drafts with an encrypted offline queue, stable
  retry identity, exact UID/UIDVALIDITY replacement, and server-draft cleanup
  after sending.
- [ ] Implement an explicit permanent-deletion workflow.
- [ ] Synchronize remote folder creation, renaming, and deletion.
- [ ] Add backoff/retries and richer conflict handling; retain periodic
  `UID SEARCH ALL` reconciliation as a defensive server-consistency check.

### Reading and writing

- Safely display plain-text and sanitized HTML messages. The independent MIME
  parsing and HTML sanitizing core and native reading-pane integration are
  implemented for the local alpha.
- Control remote content. HTTP(S) images are blocked by default in the rendering
  core; per-message UI controls and trusted-sender preferences remain.
- Compose, reply to, and forward messages. SMTP submission with plain-text and
  sanitized HTML as `multipart/alternative` is implemented, including
  importance headers.
- [x] Send outgoing attachments and retain durable local copies for locally
  composed messages and drafts. User-selected files are MIME encoded, copied
  into the active profile's object directory, listed in the reading pane, and
  exportable through the native save dialog.
- [x] Keep the bounded complete-MIME import/cache path for local or legacy
  sources while excluding inline resources and incomplete attachment sets.
- [x] Resolve bounded inline `cid:` PNG, JPEG, and GIF resources against their
  MIME Content-ID and render them from memory without file or network access.
- [x] Extract normal attachment metadata from IMAP `BODYSTRUCTURE` and fetch a
  selected server-backed section with `BODY.PEEK`, exact UIDVALIDITY/UID
  verification, bounded transfer decoding, and a native Save As destination.
- [x] Replace initial full-RFC822 transfer with header/`BODYSTRUCTURE` discovery
  followed by bounded `BODY.PEEK` requests for primary text/HTML parts and safe
  inline raster resources. Normal attachment sections are not requested.
- [ ] Add download and synchronization progress, cancellation, resumable large
  transfers, and additional safe inline formats.
- Support drafts, signatures, sender identities, read state, and flags.
  Local drafts reopen with persisted To/Cc/Bcc fields, Quill rich-text state,
  importance, and retained attachment objects. Read state, flags, mailbox
  moves, and custom-folder mutations are persisted through the Rust bridge and
  SQLite. Remote-message flags, moves, and complete rich MIME draft lifecycle
  operations are queued for IMAP. Fully cached server drafts without
  remote-only attachments become editable. Identity management, safe editing
  of server drafts with deferred attachments, and remote custom-folder
  mutations remain.

### Local data and recovery

- [x] Provide profile-wide weighted search that prioritizes subjects, senders,
  and recipients from a progressive compact metadata catalogue. An explicit
  second stage includes previews, cached bodies, and attachment names. Its
  FTS5 index remains inside the encrypted SQLCipher profile.
- [ ] Add cancellable cross-folder IMAP server-body search for content that is
  not cached locally, with clear provider-specific limitations and progress.
- [x] Create an authenticated, password-protected manual profile export with
  Argon2id key wrapping.
- [x] Restore accounts, credentials, local mail, attachments, calendars, tasks,
  contacts, and settings from an exported profile using a staged rollback-safe
  import.
- Resynchronize server-hosted messages after restoration.

### MVP completion

- [x] Introduce ARB-based German/English localization and localize standard
  mailbox roles independently from their exact IMAP server paths.
- Move the remaining prototype interface strings into the localization
  catalogue and add an explicit language preference alongside automatic
  system-language selection.
- Add protocol and integration tests.
- Improve error handling and recovery behavior.
- Validate supported workflows on Windows, macOS, and Linux.
- Add Swift Package Manager support to the generated macOS Rust bridge before
  Flutter removes its current CocoaPods compatibility path.
- Publish an initial alpha release.

## Phase 2: Personal workspace

- [x] Persist local calendar entries, tasks, and contacts in SQLite across
  application restarts.
- Add CalDAV synchronization for calendars.
- Support iCalendar events, reminders, recurrence, and time zones.
- Add local tasks and VTODO/CalDAV synchronization.
- Add contacts with vCard and CardDAV support.
- Provide a combined daily overview.
- Create tasks and calendar events from email.

## Later phases

### Extensions

- Publish a versioned extension SDK.
- Introduce a permission-based, sandboxed extension runtime.
- Support controlled access to mail, calendar, tasks, contacts, network
  domains, notifications, and dedicated plugin storage.
- Allow sideloaded extensions before considering an official marketplace.

### Optional AI assistants

- Support replaceable local and external AI providers.
- Let users choose which accounts, folders, messages, and capabilities an
  assistant may access.
- Keep read and write permissions separate.
- Require confirmation for sensitive actions such as sending or deleting.
- Support search, summaries, draft creation, and extracting tasks or events.

### Profile synchronization and additional platforms

- Explore encrypted, object-based synchronization through user-selected
  storage providers.
- Avoid synchronizing an active SQLite database directly between devices.
- Evaluate Android and iOS applications after the desktop experience is
  stable.

## Explicitly outside the first MVP

- A full browser-based mail client
- Mobile applications
- A mandatory MAICENTA cloud account
- Automatic multi-device profile synchronization
- Built-in external AI integrations
- An extension marketplace
- Advanced Exchange-specific features
- Central enterprise administration
- Shared mailboxes, delegation, and team task management

## How to participate

Contribution workflows will be documented as the repository foundation is
created. Until then, discussions and issues should focus on the active phase so
the mail foundation remains manageable and reliable.
