---
title: Connecting accounts
description: How MAICENTA signs in to Microsoft, Google and classic IMAP/SMTP providers, and what to do when it does not work.
order: 2
---

## How sign-in works

MAICENTA is registered as an official client with Microsoft, Google and Apple. Wherever a provider offers OAuth 2.0, MAICENTA opens your browser, you sign in with the provider directly, and MAICENTA only receives an access token. Your password is never typed into or stored by MAICENTA.

All tokens and credentials are stored inside your encrypted profile on your device.

## Microsoft 365 and Outlook.com

MAICENTA talks to Microsoft through the Microsoft Graph API. This works for personal Outlook.com accounts as well as work and school accounts, including tenants where IMAP and SMTP AUTH are disabled by policy.

Supported today: per-folder delta synchronization, reading and searching mail, attachments, read and flag changes, moving messages, server-side drafts and sending.

If your organization restricts third-party apps, an administrator may need to grant consent for MAICENTA once.

## Google Workspace and Gmail

MAICENTA signs in through Google with OAuth 2.0 and then uses IMAP for reading and SMTP for sending. IMAP has to be enabled in your Gmail settings; Google Workspace administrators can also disable it centrally.

## Other IMAP/SMTP providers

Enter your email address and MAICENTA looks up known server settings. If your provider is not recognized, enter:

- IMAP host, port and encryption (usually port 993 with TLS)
- SMTP host, port and encryption (usually port 465 with TLS or 587 with STARTTLS)
- Your username and an app password if your provider offers one

Providers that require OAuth but are not yet supported natively are listed in the [roadmap](https://github.com/Tim0theus/maicenta/blob/main/ROADMAP.md).

## Troubleshooting

**The browser window closes but MAICENTA does not continue.**
Make sure the MAICENTA app is still running and that no firewall blocks the local redirect. Try the sign-in again.

**Folders appear but no messages load.**
Synchronization in the alpha is deliberately bounded. Open a folder to fetch its recent messages. Older messages are loaded on demand.

**I changed my password at the provider.**
Remove and re-add the account. Existing local data is kept.

Anything else: open an issue on [GitHub](https://github.com/Tim0theus/maicenta/issues) with the provider name and the exact error message. Please never paste tokens or passwords.
