# Security Policy

## Supported versions

MAICENTA has not published a production release. Security fixes currently
target the latest commit on `main`; the desktop alpha must not be used for
sensitive or production email accounts.

## Reporting a vulnerability

Do not disclose exploitable vulnerabilities, credentials, private messages, or
personal profile data in a public issue.

Use GitHub's private vulnerability reporting option in the repository's
**Security** tab when it is available. If that option is not available, open a
public issue containing only a request for a private maintainer contact and no
technical exploit details.

Include affected versions, impact, reproduction prerequisites, and a minimal
proof of concept without real personal data. Reports will be acknowledged as
soon as practical for this volunteer project.

## Local profile protection

The current alpha encrypts structured profile data with SQLCipher and local
objects with authenticated encryption. The operating-system credential store
holds one random master key per profile. Password-protected profile archives
use Argon2id-derived key wrapping and are ignored by Git.

This protects closed profiles and backup files at rest. It does not protect an
unlocked profile from malicious software running under the same user account.
There is currently no maintainer recovery key: losing the native credential
entry and every working protected export permanently loses access to the local
profile. Continue to use a non-critical test account during the alpha.
