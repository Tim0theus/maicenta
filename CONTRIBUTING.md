# Contributing to MAICENTA

MAICENTA is an early desktop alpha. Focus contributions on the foundations in
the current roadmap phase and keep production account credentials out of test
data, logs, screenshots, commits, and issues.

## Development setup

Install a stable Flutter SDK with desktop support and a stable Rust toolchain.
Then resolve the Flutter dependencies:

```sh
cd apps/client_flutter
flutter pub get
```

Run the desktop client with `flutter run -d macos`, `flutter run -d windows`,
or `flutter run -d linux`. The application creates a local demonstration
profile. It accesses an email server only after a contributor explicitly adds
an IMAP/SMTP account; use a non-critical test account during development.

## Before opening a pull request

From the repository root, run:

```sh
cargo fmt --all -- --check
cargo test --locked --workspace --all-targets
cargo clippy --locked --workspace --all-targets -- -D warnings

cd apps/client_flutter
dart format --output=none --set-exit-if-changed lib test
flutter analyze
flutter test
```

Keep pull requests focused, explain visible behavior changes, and add tests for
new parsing, storage, or interaction behavior. Generated Flutter/Rust bridge
files are committed and must be regenerated whenever the public bridge API
changes.

## Website, documentation and translations

The public website for maicenta.com lives in `website/` and is built with
Astro. Fixing typos, improving the guides, or translating pages is one of the
easiest ways to help.

- Interface text is in `website/src/i18n/en.ts` and `website/src/i18n/de.ts`.
  English is the reference; the German file is type-checked against it, so a
  missing key fails the build.
- Documentation articles are Markdown files under
  `website/src/content/docs/<lang>/`. Keep the same file name in every
  language folder so the language switcher can pair the pages.
- Legal pages live in `website/src/content/legal/`. Do not change wording
  there without discussing it in an issue first.
- Adding a language is described in `website/README.md`.

Before opening a pull request for the website, run from the repository root:

```sh
cd website
npm ci
npm run check
npm run build
```

Website changes are deployed automatically after they are merged into `main`;
pull requests only build the site as a check. Note that content and brand
assets in `website/` are licensed differently from the code, see
`website/LICENSE.md`.

## Security-sensitive code

Treat MIME input, HTML, attachments, protocol responses, extensions, and AI
provider output as untrusted. Do not weaken sanitizing, remote-content blocking,
secret storage, or confirmation boundaries without documenting and testing the
security implications. Follow [SECURITY.md](SECURITY.md) for vulnerabilities.
