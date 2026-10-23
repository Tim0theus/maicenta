# Changelog

All notable changes to MAICENTA are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/). The release workflow copies the
section of the tagged version into the GitHub Release notes.

## [Unreleased]

## [0.1.0-beta.1] - 2026-10-23

First public beta of the desktop client for macOS, Windows, and Linux.

### Added

- Local-first desktop mail client with an Outlook Classic-inspired layout:
  Favorites, account folder trees, message list, reading pane, composer, and
  a dedicated message window.
- IMAP/SMTP accounts with password or app password, and Microsoft 365 /
  Google sign-in through the browser (OAuth 2.0 with PKCE, refresh tokens in
  the encrypted profile).
- Microsoft Graph connector for Exchange Online tenants where IMAP/SMTP AUTH
  is disabled: folder discovery, per-folder delta synchronization, bounded
  HTML bodies with inline images, on-demand attachments, read/flag/move,
  server drafts, and sending.
- E-mail-first account setup: enter the address, MAICENTA detects Microsoft
  365, Google, or IMAP/SMTP settings and recommends a method; every
  alternative stays one click away.
- Encrypted SQLCipher profile with a random master key in the operating
  system credential store, authenticated attachment objects, and
  password-protected profile export and import.
- Bounded incremental synchronization: UIDNEXT/CONDSTORE/QRESYNC for IMAP,
  IMAP IDLE for the visible folder, delta cursors for Graph, silent
  background passes with progress in the status bar.
- Safe HTML rendering: HTML5 sanitizer, remote images blocked by default,
  inline `cid:` images resolved from memory.
- Rich-text composer with attachments, drafts with an encrypted offline
  queue, reply, reply all, forward, importance.
- Sending from provider aliases ("send as"): each account keeps a list of
  additional sender addresses, the composer offers them in the "Von" field,
  replies default to the alias the original message was addressed to, and
  drafts remember their sender.
- Profile-wide weighted search over subjects, senders, recipients, and, on
  request, cached bodies and attachment names.
- Local calendar, tasks, and contacts persisted in the profile.
- Collapsible folder tree with persisted state for nested folders, the
  Favorites section, and each account.
- "Über MAICENTA" with version, platform, a copyable diagnostics line for
  bug reports, and open-source licenses of the Flutter and Rust
  dependencies.
- German and English localization of the folder shell.

### Known limitations

- **Unsigned builds.** macOS shows "cannot be opened" until the app is
  started once via right-click → Open; Windows SmartScreen warns on first
  start. Both are documented in the downloads. Signing and notarization
  follow with the Apple Developer and code-signing certificates.
- **Google sign-in in the browser is not yet available** because the
  Google OAuth client is still under verification. Gmail and Google
  Workspace accounts work through IMAP with an app password; the account
  dialog recommends that path automatically.
- **Microsoft consent screen** shows "unverified publisher" until the Entra
  publisher verification is complete. Tenants that block unverified apps
  need a one-time admin consent.
- **On-premises Exchange (EWS)** is detected but not supported.
- **Permanent deletion** of messages has no dedicated workflow yet;
  messages moved to the trash stay there until the server or another client
  removes them.
- **Server folders** cannot be created, renamed, or deleted from MAICENTA
  yet; local folders in the local profile can.
- **Retry and backoff** after synchronization errors are basic; a failed pass
  is reported and retried on the next interval.
- **Windows and Linux** builds are produced by the release workflow but have
  had less manual testing than macOS.
- Profiles are migrated in place between versions. Keep a password-protected
  export before upgrading a beta anyway.

[Unreleased]: https://github.com/Tim0theus/maicenta/compare/v0.1.0-beta.1...HEAD
[0.1.0-beta.1]: https://github.com/Tim0theus/maicenta/releases/tag/v0.1.0-beta.1
