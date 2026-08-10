# MAICENTA desktop client

This directory contains the Flutter desktop prototype for Windows, macOS, and
Linux.

## Run locally

Install a current stable Flutter SDK with desktop support, then run:

```sh
flutter pub get
flutter run -d macos
```

Replace `macos` with `windows` or `linux` on the corresponding platform. List
available devices with:

```sh
flutter devices
```

The alpha opens a local SQLCipher-encrypted profile through the Rust core. It seeds
demonstration data when that profile is empty, renders sanitized HTML mail, and
provides the Outlook Classic-inspired reading and rich-text composition flows.
The main mail workspace uses a dense native-desktop layout with a light title
area, grouped ribbon, Favorites and account trees, bottom module navigation,
compact message rows, reading pane, and status bar instead of a web-mail-style
application rail and colored web header.
The composer follows the corresponding classic message window: a light quick
access title area, functional message-specific ribbon tabs, a prominent Send
button beside compact address fields, rectangular attachment entries, a
continuous white editing surface, direct draft saving, and a desktop status
bar.
Double-clicking a message opens a dedicated classic message window with
functional Message/File/View ribbon tabs, detailed envelope fields, safe HTML,
attachments, zoom, reply-all, marking, moving, archiving, and deletion.
It can connect to password-based IMAP/SMTP accounts and send sanitized HTML
messages through SMTP, with a plain-text MIME fallback.

Visible workspace commands are wired to local behavior: messages can be
filtered, sorted, marked, archived, or moved to trash; folders and personal
workspace items can be created; sent messages and drafts appear in their local
folders; and the complete profile can be exported and imported with a separate
password. Messages can also be dragged onto folders in the same account.
Folders can be dragged into Favorites, reordered there, or dragged back to the
account heading to remove the shortcut; the order persists in the encrypted
profile. Native files dropped anywhere on the compose window are validated and
added to its attachment strip. The title-bar search first prioritizes subjects,
senders, and recipients from the encrypted profile catalogue rather than only
the currently loaded folder. Its document button explicitly expands the search
to cached message bodies, previews, and attachment names. Selecting a header-only result
while online loads only that message's bounded display content from IMAP.
Mail messages,
drafts, read and flag state, mailbox moves, and custom-folder changes persist
in the encrypted database across restarts. Calendar entries, tasks, contacts,
and account configuration are durable as well. Mail passwords and app passwords
live inside that encrypted profile; the operating-system credential store holds
only its random master key. Changes
to read state, flags, archive placement, and trash placement for downloaded
IMAP mail are queued in SQLite and applied during the next synchronization;
the status bar shows the number still pending.

Use **Datei → Optionen → Dunkler Modus** to switch the complete desktop shell,
composer, dialogs, and message window between the light and dark palettes. The
selection is stored inside the encrypted profile. Double-clicking an editable
draft opens it directly in the composer and closing that composer returns to
the main workspace instead of another message window.

When at least one real account is configured, the workspace starts online.
Local drafts have a dedicated **Entwurf bearbeiten** action and reopen with
their recipients, rich-text editor state, importance, and retained local
attachments. Online saves are uploaded immediately to the account's discovered
IMAP drafts mailbox. Offline saves and failed creates, replacements, or
deletions stay in the encrypted queue for the next synchronization. A fully
downloaded server draft without remote-only attachments also becomes locally
editable; attachment-bearing server drafts stay read-only until safe retained
attachment editing is available.

Use **Datei → Kontoeinstellungen** to add, edit, test, or remove IMAP and SMTP
accounts. The composer provides an account selector. Synchronization downloads
up to 25 recent message bodies and progressively catalogues up to 250 additional
compact header/attachment metadata records from every subscribed selectable
folder per account and pass. The client automatically continues bounded passes
while older catalogue entries remain. It initially shows the newest local page
for each folder and exposes **Ältere Nachrichten laden** for further encrypted
SQLite pages without imposing a fixed 500-message folder ceiling.
Moves use IMAP `MOVE` or a UID-scoped `UIDPLUS` fallback and never a global
expunge. The native file dialog and desktop file drop can attach up to ten
files with a combined raw size of 18 MiB to an SMTP message. Locally composed
messages and drafts retain independent attachment copies in the profile object
directory; the reading pane can export them through the native save dialog.
Incoming synchronization first loads headers and `BODYSTRUCTURE`, then
requests only bounded text/HTML parts and safe inline images. Normal attachment
entries are marked **Auf Server** and
are fetched individually through `BODY.PEEK` into the native Save As
destination. Later passes reuse known UIDs, refresh their flags without loading
their bodies, use persisted UIDNEXT ranges for new messages, and use CONDSTORE
mod-sequence flag deltas when the server supports them. A periodic full UID
reconciliation detects deletions safely. QRESYNC-capable servers additionally
send `VANISHED` deletion deltas, which remove exact generation-matched local
records and attachment objects. The synchronization status reports how many
folders used the delta, full, and QRESYNC paths. Bounded PNG, JPEG, and GIF
`cid:` resources are rendered from memory without implicit network access.
Detailed progress/cancellation, additional inline formats, OAuth, permanent
deletion, remote folder mutations, automatic full-history body caching, CalDAV, and
CardDAV are still pending.

Generated Flutter/Rust bindings live in `lib/src/rust` and
`core/bridge/src/frb_generated.rs`. After changing the public bridge API,
regenerate them from this directory with:

```sh
cargo install flutter_rust_bridge_codegen --version 2.12.0 --locked
flutter_rust_bridge_codegen generate
```

## Checks

```sh
dart format --output=none --set-exit-if-changed lib test
flutter analyze
flutter test
```
