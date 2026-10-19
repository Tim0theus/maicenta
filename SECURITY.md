# Security Policy

## Supported versions

| Version | Status |
| --- | --- |
| 0.1.0-beta.x (latest beta) | Security fixes are released as the next beta. |
| Older alphas and development builds | Not supported; please upgrade. |

The beta is suitable for everyday use with accounts you can afford to
re-synchronize. Keep a password-protected profile export before upgrading, and
do not use it as the only client for a mailbox that you cannot recover from the
server.

## Reporting a vulnerability

Do not disclose exploitable vulnerabilities, credentials, private messages, or
personal profile data in a public issue.

Report privately through GitHub's private vulnerability reporting:
<https://github.com/Tim0theus/maicenta/security/advisories/new>. The report is
visible only to the maintainers until a fix is published.

Include the affected version (Datei → Optionen → Über MAICENTA shows a
copyable line), platform, impact, reproduction prerequisites, and a minimal
proof of concept without real personal data. Reports are acknowledged within
seven days for this volunteer project; fixes for confirmed issues ship as the
next beta and are credited in the release notes unless you prefer otherwise.

## Scope

In scope: the desktop client, its Rust core, the MIME and HTML rendering
pipeline, the encrypted profile and export format, account credential
handling, and the release artifacts.

Out of scope: the marketing website's static content, third-party mail
servers, and issues that require an already compromised user account on the
same machine (see below).

## Local profile protection

MAICENTA encrypts structured profile data with SQLCipher and local objects
with authenticated encryption. The operating-system credential store holds one
random master key per profile; account passwords and OAuth tokens live inside
the encrypted profile, never in the keychain directly. Password-protected
profile archives use Argon2id-derived key wrapping.

This protects closed profiles and backup files at rest. It does not protect an
unlocked profile from malicious software running under the same user account.
There is no maintainer recovery key: losing the native credential entry and
every working protected export permanently loses access to the local profile.
