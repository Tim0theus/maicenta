---
title: Getting started
description: Install MAICENTA, create your first profile and connect a mail account in a few minutes.
order: 1
---

## 1. Install

Download the build for your platform from [GitHub Releases](https://github.com/Tim0theus/maicenta/releases) and start the app. MAICENTA runs on Windows, macOS and Linux.

MAICENTA is in early alpha. Use a non-critical mail account for your first tests and keep an eye on the release notes: profile formats may still change between alpha versions.

## 2. Create a profile

On first start MAICENTA creates a local profile. A profile contains your accounts, settings, local calendars, tasks, contacts and notes. It is stored encrypted on your device and never leaves it unless you export it or set up sync.

Choose a strong profile password. It protects the encryption keys of your profile. MAICENTA cannot recover it for you.

## 3. Add a mail account

Open **Accounts** and enter your email address. MAICENTA tries to detect the right settings automatically:

- **Microsoft 365, Outlook.com**: sign in through Microsoft in your browser. MAICENTA uses Microsoft Graph, so it also works in tenants where IMAP is switched off.
- **Google Workspace, Gmail**: sign in through Google in your browser. Mail is fetched over IMAP and sent over SMTP with OAuth 2.0.
- **Other providers**: MAICENTA looks up known IMAP and SMTP settings. If nothing is found, enter host, port and encryption manually.

MAICENTA never stores your provider password when OAuth is available. Tokens are kept inside your encrypted profile.

See [Connecting accounts](/en/docs/accounts/) for provider details and troubleshooting.

## 4. Work offline

Messages, folders and flags are cached locally. You can read, search, flag and draft mail without a connection. Changes are queued and synchronized when you are back online.

## 5. Back up your profile

Use **Vault → Export** to create an encrypted backup of your whole profile. Import it on another device to move your workspace. Encrypted sync between devices is the next step on the roadmap.

## Getting help

- Bugs and questions: [GitHub Issues](https://github.com/Tim0theus/maicenta/issues)
- Security reports: see the [security policy](https://github.com/Tim0theus/maicenta/blob/main/SECURITY.md)
