<p align="center">
  <img src="assets/branding/maicenta-banner.png" alt="MAICENTA – local-first mail and productivity workspace" width="100%">
</p>

# MAICENTA

> Mail · AI · Calendar · Events · Notes · Tasks · Assistant

**The open workspace for your digital day.**

MAICENTA is a free, local-first, modular, and extensible open-source desktop
application under active development. It brings email, calendars, tasks,
contacts, notes, and optional AI assistants together in one personal workspace.

> [!IMPORTANT]
> MAICENTA currently provides an early desktop alpha with real IMAP/SMTP
> connectivity. Test it with a non-critical account first: synchronization is
> deliberately bounded and several provider-specific and recovery workflows
> are not complete yet.

## Vision

MAICENTA aims to become an open, private, and user-friendly alternative to
Outlook, Thunderbird and other apps. It is designed to work without a mandatory account, web server, cloud
service, or vendor lock-in. Users should retain control of their accounts and
data and be able to export, back up, and restore their complete profile.

The initial focus is a native desktop application for Windows, macOS, and
Linux. Mobile applications may follow later. A full browser-based mail client
is not currently planned because direct, persistent IMAP and SMTP access would
require a server or self-hosted gateway.

## Principles

- Free and open source
- Local-first and offline-capable
- Privacy by design and secure by default
- No mandatory account, cloud, or web server
- Open standards and no vendor lock-in
- Modular architecture with user-controlled features
- Exportable data and portable, encrypted profiles
- Optional AI with granular permissions

## Planned modules

| Module | Purpose | Planned phase |
| --- | --- | --- |
| Mail | IMAP/SMTP accounts, offline cache, search, and identities | MVP |
| Calendar | Local calendars now; iCalendar and CalDAV later | Phase 2 |
| Tasks | Local tasks now; VTODO and CalDAV later | Phase 2 |
| Contacts | Local contacts now; vCard and CardDAV later | Phase 2 |
| Notes | Personal notes within the workspace | Later |
| Vault | Encrypted profile export, import, backup, and future sync | MVP foundation |
| Assistant | Optional local or external AI providers | Later |
| Extensions | Permission-based third-party plugins | Architecture in MVP, runtime later |

Modules are intended to be independently enabled or disabled. Disabled modules
should disappear from navigation, stop unnecessary background work, and retain
their data unless the user explicitly chooses to remove it.

## Current status

The repository contains a runnable local desktop alpha. In addition to the
offline demonstration mailbox, it can configure multiple password-based
IMAP/SMTP accounts, discover their folders, progressively catalogue compact
headers from every subscribed selectable folder, download a bounded set of recent message
bodies, and send standards-oriented HTML mail with a plain-text alternative and
file attachments through SMTP. Incoming sync retains sender, recipient,
subject, date, and `BODYSTRUCTURE` attachment metadata independently from body
content, then fetches only bounded text/HTML sections and safe inline raster
resources for the recent cache. Normal attachment bytes remain on the server;
their metadata stays visible and selecting an entry fetches its validated MIME
section with `BODY.PEEK` into the user-selected destination. Later sync runs
reuse cached UID identities, update only flags for known messages, fetch new
UIDs in bounded batches, and remove cache entries that disappeared from a
selected mailbox or became invalid after a UIDVALIDITY change. Account
configuration, cached workspace data, and account credentials are stored in an
encrypted SQLCipher profile. A random profile master key is kept in the
operating-system credential store and cached only for the running app process.
Local attachment objects are encrypted and authenticated separately. Global
search first prioritizes subjects, senders, and recipients from that catalogue.
After an explicit user action it additionally covers previews, locally cached
message bodies, and attachment names. The weighted FTS5 index remains inside
the encrypted profile. Opening a header-only result while online verifies its
stored UID identity and fetches only that message's bounded display parts.
Existing alpha profiles refill previously unavailable recipient metadata
progressively without discarding already cached bodies or attachments.
When a server contains more than one catalogue batch, the desktop client
continues synchronization passes automatically while the reported remaining
count decreases. The workspace initially opens a bounded local page per folder
and can load older encrypted catalogue entries page by page; folder counters
continue to represent the complete locally known folder rather than only the
visible page.

The desktop client uses Flutter backed by a Rust core and SQLite for local
structured data. Attachments of locally composed messages are copied into a
profile-local object directory and can be saved again from the reading pane.

### Run the desktop prototype

The current prototype contains an Outlook Classic-inspired workspace. On its
first start, the Rust core creates a local SQLite profile and adds demonstration
mailboxes and messages. Later starts load the same persisted data. A stable
Flutter SDK with desktop support and a Rust toolchain are required.

The primary mail window deliberately follows a native desktop information
hierarchy rather than a web-mail shell: a light quick-access title area, flat
tabbed ribbon groups, Favorites before account folder trees, compact message
rows, a persistent reading pane, module navigation at the bottom of the folder
pane, and a classic connection/status bar.

Desktop drag and drop is part of that workflow. A message can be dragged onto
another folder of the same account; the local encrypted state changes
immediately and an IMAP move is queued for the next synchronization. Real
folders can be dragged into Favorites, reordered there, or dragged back onto
their account heading to remove the shortcut. The exact favorite order is
stored in the encrypted profile. Files dropped from Finder, Explorer, or a
Linux file manager onto the compose window become attachments and follow the
same ten-file/18-MiB validation as the native file picker.

The desktop chrome, folder and message panes, dialogs, composer, and dedicated
message window support a profile-specific dark mode. It can be changed through
**Datei → Optionen → Dunkler Modus** and is restored from the encrypted profile
on the next start.

Technical IMAP paths remain unchanged for synchronization but are not exposed
as standard-folder labels. For example, `INBOX.Drafts` is presented as
**Entwürfe** on a German system and **Drafts** on an English system. MAICENTA
uses Flutter ARB localization files for this presentation layer; the folder
shell is available in German and English while migration of the remaining
prototype strings is still in progress.

The desktop interface includes a tabbed ribbon and an Outlook Classic-inspired
compose window. Its large Send command sits beside compact From/To/Cc/Bcc and
Subject rows; Message, Format Text, Insert, Options, and File tabs expose
different functional ribbon groups. The continuous HTML editor supports
font family and size, text and highlight colors, bold, italic, underline,
strike-through, alignment, lists, indentation, links, undo/redo, signatures,
attachments, importance, and direct local-plus-IMAP draft saving.

Double-clicking a message opens a dedicated classic desktop message window
with its own ribbon, detailed From/To/Cc/Bcc header, safe HTML body, attachment
actions, zoom, read/unread state, flags, reply/reply-all/forward, archive,
move, and delete commands.

The interaction alpha also supports two-stage encrypted message search, local
message filtering and sorting,
marking, archiving, moving to trash, folder creation and renaming, reading-pane
layouts and zoom, local sent items and drafts, calendar entries, tasks, contacts,
and a password-protected complete profile export/import. Composed messages,
drafts, read and flag state,
mailbox moves, custom folders, calendar entries, tasks, and contacts are written
to SQLite and survive application restarts. Local drafts reopen in the rich-text
composer with their recipients, formatting, importance, and retained local
attachments. Double-clicking an editable draft opens that composer directly;
it no longer creates an intermediate message window that has to be closed
separately. Read state, flags, archive moves,
and moves to trash for downloaded IMAP messages are placed in a persistent
queue and applied to the server during the next synchronization.
Configured online accounts also upload saved drafts immediately with IMAP
`APPEND`. Offline saves, failed uploads, edits, and removal after sending remain
in the encrypted persistent queue and retry during synchronization. Stable
draft Message-IDs make retries idempotent; replacing or removing a known server
draft uses its exact UID and UIDVALIDITY and never falls back to a global
`EXPUNGE`.

Use **Datei → Importieren/Exportieren** to create a `.maicenta-profile`
backup. A full backup includes account credentials and therefore requires an
export password of at least twelve characters. Import authenticates and stages
the backup before replacing the active profile. The password cannot be
recovered by MAICENTA. Existing alpha databases and attachment objects are
encrypted automatically on first launch; existing per-account keychain entries
are then moved into the profile vault and removed where possible.

```sh
cd apps/client_flutter
flutter pub get
flutter run -d macos    # or windows / linux
```

Open **Datei → Kontoeinstellungen → Konto hinzufügen** to enter IMAP and SMTP
settings, test both connections, and save the account. Saving starts the first
synchronization. The mail reading pane renders downloaded MIME messages as
sanitized HTML, blocks active and remote content, and does not automatically
open external links. Reply and forward actions open a prefilled rich-text
compose window.

Current account limitations are important:

- Authentication uses a password or provider-issued app password. OAuth 2.0,
  including modern Microsoft 365 sign-in, is not implemented yet.
- Incoming synchronization reads every subscribed selectable folder and
  downloads up to 25 recent bounded display bodies per folder on the first
  pass, then up to 25 new or previously incomplete bodies per pass. Compact
  headers and `BODYSTRUCTURE` metadata are catalogued newest-first in batches
  of 250 and automatically continued until no older entries remain. Known
  messages receive only a flag refresh. Normal attachment sections are
  deferred. Queued read/flag changes and mailbox moves are uploaded first.
  Moves require the server's `MOVE` capability or the safe `UIDPLUS`
  copy-and-UID-expunge fallback; MAICENTA never falls back to an account-wide
  `EXPUNGE`.
- Accounts synchronize independently: a failure is reported for that account
  while successful accounts still update their local cache.
- SMTP delivery sends `multipart/alternative` plain-text and sanitized HTML.
  Up to ten user-selected files with a combined raw size of 18 MiB can be sent
  as MIME attachments. The local sent-item or draft cache retains independent
  copies of those files and exposes them through the native **Save As** dialog.
  Incoming normal attachments remain server-backed after synchronization.
  `BODYSTRUCTURE` metadata is visible in the reading pane and an individual
  file can be fetched with `BODY.PEEK` after an explicit click, subject to a
  100 MiB decoded-size limit and fresh mailbox UIDVALIDITY/UID checks.
- Calendar, tasks, and contacts are durable local records; CalDAV, CardDAV,
  iCalendar, VTODO, and vCard synchronization are not implemented yet.
- Safe inline `cid:` PNG, JPEG, and GIF resources are resolved fully offline;
  sender-provided data URLs remain blocked. Display-part synchronization is
  bounded and reports an explicit warning when a declared section is missing,
  unsupported, or over its safety limit. Completed mailboxes use persisted
  UIDNEXT checkpoints for new-message ranges. Servers advertising CONDSTORE
  additionally use HIGHESTMODSEQ/CHANGEDSINCE for flag deltas. The next
  synchronization after a checkpoint becomes 24 hours old performs a complete
  `UID SEARCH ALL` safety reconciliation. On servers advertising QRESYNC,
  `VANISHED` deltas remove generation-matched local messages and attachments
  immediately; the periodic reconciliation remains as a defensive fallback.
  Detailed per-message progress,
  cancellation, additional inline formats, permanent
  deletion, remote folder creation/renaming, and automatic full-history body or
  attachment caching are not implemented yet.

SQLCipher is built with its vendored OpenSSL backend so profile encryption uses
the same database layer on all supported desktop platforms. A typical
Linux development system still needs the standard C/C++ build toolchain and
`pkg-config`.

### Validate the workspace

```sh
cargo fmt --all -- --check
cargo test --locked --workspace --all-targets
cargo clippy --locked --workspace --all-targets -- -D warnings

cd apps/client_flutter
dart format --output=none --set-exit-if-changed lib test
flutter analyze
flutter test
```

The same checks run for pushes and pull requests through GitHub Actions.

## Roadmap

See the public [roadmap](ROADMAP.md) for the complete, living plan.

1. Initialize the monorepo and development environment.
2. Run the Flutter application on Windows, macOS, and Linux.
3. Connect the Rust core and create the local profile and database schema.
4. Harden IMAP/SMTP synchronization, OAuth, retries, and server-side state.
5. Add progress and cancellation, additional safe inline formats, and advanced
   identities.
6. Add identities, signature management, and provider-specific setup help.
7. Harden the encrypted profile vault with an optional independent recovery
   key and broader corruption/recovery testing.
8. Stabilize the MVP with tests, error handling, and an initial alpha release.
9. Expand into calendar, tasks, contacts, extensions, and optional AI.

## Architecture

The planned component boundaries, storage model, security rules, protocols, and
repository layout are documented in [ARCHITECTURE.md](ARCHITECTURE.md).

The versioned logo, wordmark, and banner files live in
[`assets/branding`](assets/branding/README.md). Application and platform icons
are derived from the canonical symbol stored there.

## Contributing

Ideas, bug reports, documentation, and focused code contributions are welcome.
See [CONTRIBUTING.md](CONTRIBUTING.md) for setup, validation, and pull-request
guidance, and [SECURITY.md](SECURITY.md) for private vulnerability reports.

Please do not include passwords, OAuth tokens, API keys, private keys, recovery
keys, personal profiles, or local databases in commits or issue reports.

## Support the project

MAICENTA is free and open source. If you would like to support its development,
you can use one of the voluntary funding options below.

| Method | Details |
| --- | --- |
| Bank transfer | `Placeholder: account holder, IBAN and BIC` |
| PayPal | `Placeholder: PayPal link` |
| Cryptocurrency | `Placeholder: wallet address and network` |
| Patreon | `Placeholder: Patreon link` |

Support is voluntary and does not purchase control over the roadmap. Essential
features and security updates will not be placed behind a supporter paywall.
Payments should be understood as project support, not as tax-deductible
donations unless explicitly stated otherwise.

## Security

Security and privacy are core design requirements. MAICENTA sanitizes HTML
email and controls remote content. Structured local data is protected at rest
by SQLCipher, attachment objects use authenticated encryption, and the
operating-system credential store contains one random key per profile rather
than individual account passwords. Complete profile backups wrap that key with
an Argon2id-derived export key. Plugin isolation and independent recovery keys
remain planned.

Mail rendering is deliberately independent of the Outlook-inspired interface.
The Rust core parses RFC 5322/MIME messages, decodes transfer encodings and
character sets, sanitizes HTML using HTML5 parsing rules, preserves common safe
email layouts, and blocks remote images by default. This avoids coupling the
viewer to Outlook's historical HTML renderer while retaining a plain-text
fallback for malformed or incompatible messages.

Please do not report security vulnerabilities in public issues. A private
reporting process will be documented before the first public release.

## License

MAICENTA is licensed under the [Mozilla Public License 2.0](LICENSE).
Commercial use of the community edition is permitted under the license. MPL
copyleft applies at the file level, allowing separate plugins to use their own
compatible licensing terms.

The license does not grant rights to project names, trademarks, service marks,
or logos. The MAICENTA name is provisional and has not yet received final
trademark clearance.
