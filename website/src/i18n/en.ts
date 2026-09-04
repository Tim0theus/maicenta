export const en = {
  meta: {
    siteTitle: 'MAICENTA',
    defaultDescription:
      'MAICENTA is a free, local-first, open-source desktop workspace for mail, calendar, tasks, contacts, notes and optional AI assistants.',
  },
  nav: {
    home: 'Home',
    features: 'Features',
    pricing: 'Pricing',
    download: 'Download',
    docs: 'Docs',
    github: 'GitHub',
    menu: 'Menu',
    language: 'Language',
    skipToContent: 'Skip to content',
  },
  footer: {
    tagline: 'The open workspace for your digital day.',
    product: 'Product',
    project: 'Project',
    legal: 'Legal',
    imprint: 'Imprint',
    privacy: 'Privacy',
    security: 'Security policy',
    license: 'License',
    issues: 'Report an issue',
    releases: 'Releases',
    copyright: 'MAICENTA is free and open source. The MAICENTA name and logo are not covered by the software license.',
  },
  home: {
    title: 'MAICENTA – The open workspace for your digital day',
    description:
      'Mail, calendar, tasks, contacts, notes and optional AI in one free, local-first desktop app for Windows, macOS and Linux. Open source, no mandatory account.',
    badge: 'Early alpha · Windows, macOS, Linux',
    headline: 'The open workspace for your digital day.',
    subline:
      'MAICENTA brings email, calendars, tasks, contacts, notes and optional AI assistants together in one desktop app. Local-first, open source, and without a mandatory account.',
    ctaDownload: 'Download the alpha',
    ctaGithub: 'View on GitHub',
    alphaNote:
      'MAICENTA is an early alpha with real IMAP/SMTP and Microsoft 365 connectivity. Try it with a non-critical account first.',
    principlesTitle: 'Built on principles, not on lock-in',
    principlesIntro:
      'An open alternative to Outlook, Thunderbird and cloud-only suites. You keep your accounts, your data and your choices.',
    principles: [
      {
        title: 'Free and open source',
        text: 'The desktop app is free, forever. The code is public and you can read, build and improve it.',
      },
      {
        title: 'Local-first and offline',
        text: 'Your mailbox, calendars and notes live on your device. Everything works without a connection and syncs when you are back online.',
      },
      {
        title: 'Privacy by design',
        text: 'No telemetry, no tracking, no hidden uploads. Profiles are encrypted and stay under your control.',
      },
      {
        title: 'No mandatory account',
        text: 'MAICENTA never requires a MAICENTA account, cloud service or web server. Optional services stay optional.',
      },
      {
        title: 'Open standards',
        text: 'IMAP, SMTP, OAuth 2.0, iCalendar, vCard, CalDAV and CardDAV. You can leave any time and take your data with you.',
      },
      {
        title: 'Optional AI with permissions',
        text: 'Use local or external AI providers if you want to, with granular permissions and nothing enabled by default.',
      },
    ],
    modulesTitle: 'One workspace, many modules',
    modulesIntro:
      'Every module can be switched on or off. Disabled modules disappear from navigation and stop background work, but your data stays until you remove it.',
    modules: [
      { name: 'Mail', text: 'IMAP/SMTP and Microsoft 365 accounts, offline cache, search, identities.', phase: 'Available' },
      { name: 'Vault', text: 'Encrypted profile export, import and backup. The foundation for sync.', phase: 'Available' },
      { name: 'Calendar', text: 'Local calendars now, iCalendar and CalDAV later.', phase: 'Phase 2' },
      { name: 'Tasks', text: 'Local tasks now, VTODO and CalDAV later.', phase: 'Phase 2' },
      { name: 'Contacts', text: 'Local contacts now, vCard and CardDAV later.', phase: 'Phase 2' },
      { name: 'Notes', text: 'Personal notes inside your workspace.', phase: 'Later' },
      { name: 'Assistant', text: 'Optional local or external AI providers.', phase: 'Later' },
      { name: 'Extensions', text: 'Permission-based third-party plugins.', phase: 'Later' },
    ],
    providersTitle: 'Works with the accounts you already have',
    providersIntro:
      'MAICENTA is a registered client with Microsoft, Google and Apple so sign-in works the official way, with OAuth 2.0 and without storing your password.',
    providers: [
      { name: 'Microsoft 365 and Outlook.com', text: 'Via Microsoft Graph, including tenants where IMAP is disabled.' },
      { name: 'Google Workspace and Gmail', text: 'OAuth 2.0 sign-in with IMAP and SMTP.' },
      { name: 'Any IMAP/SMTP provider', text: 'Autodiscovery for common providers, manual setup for everything else.' },
    ],
    syncTitle: 'Your workspace on every device. Optional.',
    syncText:
      'MAICENTA stores your profile in an encrypted vault. Soon you will be able to sync that vault between your devices, either through storage you choose yourself or through MAICENTA Sync, a small paid service that only ever sees encrypted data.',
    syncCta: 'See pricing',
    openSourceTitle: 'Open development',
    openSourceText:
      'Roadmap, architecture and security policy are public. Contributions, bug reports and feature ideas are welcome on GitHub.',
    openSourceCta: 'Read the roadmap',
  },
  pricing: {
    title: 'Pricing',
    description:
      'The MAICENTA desktop app is free forever. MAICENTA Sync is an optional, low-cost subscription for encrypted multi-device sync.',
    headline: 'Free app. Optional sync.',
    intro:
      'MAICENTA is free and open source and will stay that way. The optional sync subscription pays for the servers and the developer accounts with Microsoft, Google and Apple that keep the official integrations working.',
    freeName: 'MAICENTA Desktop',
    freePrice: '€0',
    freePeriod: 'forever',
    freeFeatures: [
      'All modules on Windows, macOS and Linux',
      'Unlimited accounts and mailboxes',
      'Encrypted local profiles, export and backup',
      'Sync through your own storage (planned)',
      'No account, no telemetry, no ads',
      'Community support on GitHub',
    ],
    freeCta: 'Download',
    syncName: 'MAICENTA Sync',
    syncBadgePlanned: 'Planned',
    syncPriceTba: 'Price to be announced',
    perMonth: '/ month',
    perYear: '/ year',
    syncTagline: 'Encrypted vault sync between your devices, hosted for you.',
    syncFeatures: [
      'Everything in MAICENTA Desktop',
      'Sync your encrypted vault across all your devices',
      'End-to-end encrypted: the server only stores ciphertext',
      'No setup of your own storage needed',
      'Cancel any time, export any time',
      'Supports the ongoing development of MAICENTA',
    ],
    syncCta: 'Subscribe',
    syncCtaPlanned: 'Follow the launch on GitHub',
    syncPortal: 'Manage your subscription',
    faqTitle: 'Questions and answers',
    faq: [
      {
        q: 'Is the app really free?',
        a: 'Yes. The desktop app is free and open source. All modules, all accounts, no limits and no ads. Sync is the only paid option, and it is optional.',
      },
      {
        q: 'Do I need an account to use MAICENTA?',
        a: 'No. MAICENTA works completely without a MAICENTA account. You only sign in to your own mail providers.',
      },
      {
        q: 'What does the sync server see?',
        a: 'Only encrypted vault objects and the metadata needed to store them, such as size and timestamps. Keys never leave your devices. We cannot read your mail, calendars or notes.',
      },
      {
        q: 'Can I sync without paying?',
        a: 'That is the plan. Encrypted vault sync through storage you choose yourself, for example WebDAV or S3-compatible storage, is on the roadmap and will be free.',
      },
      {
        q: 'Does MAICENTA Sync handle my email?',
        a: 'No. Your email stays with your mail provider and is fetched directly by the app. Sync only covers your encrypted profile: settings, accounts, local calendars, tasks, contacts and notes.',
      },
      {
        q: 'How do I cancel?',
        a: 'Through the customer portal of the payment provider, with one click and without contacting us. Your local data is never affected by a cancellation.',
      },
    ],
  },
  download: {
    title: 'Download',
    description: 'Download the MAICENTA alpha for Windows, macOS and Linux or build it from source.',
    headline: 'Download MAICENTA',
    intro:
      'MAICENTA is in early alpha. Builds are published on GitHub Releases. Please test with a non-critical account first and report what breaks.',
    platforms: [
      { name: 'Windows', text: 'Windows 10 or newer, 64-bit.' },
      { name: 'macOS', text: 'macOS 12 or newer, Apple Silicon and Intel.' },
      { name: 'Linux', text: '64-bit desktop distributions.' },
    ],
    releaseCta: 'Go to releases',
    sourceTitle: 'Build from source',
    sourceText:
      'MAICENTA is a Rust core with a Flutter desktop client. The repository README explains the toolchain and build steps.',
    sourceCta: 'Open the repository',
    alphaTitle: 'What alpha means',
    alphaPoints: [
      'Real IMAP/SMTP and Microsoft 365 connectivity, with bounded synchronization.',
      'Some provider-specific and recovery workflows are not complete yet.',
      'Profile formats may still change between alpha releases. Use export and backup.',
    ],
  },
  docs: {
    title: 'Documentation',
    description: 'Getting started guides and documentation for MAICENTA.',
    headline: 'Documentation',
    intro: 'Short guides to get MAICENTA running and connected to your accounts.',
    readMore: 'Read',
    backToDocs: 'All docs',
    editHint: 'Found a mistake? The docs live in the repository and pull requests are welcome.',
  },
  legal: {
    imprintTitle: 'Imprint',
    privacyTitle: 'Privacy policy',
  },
  common: {
    learnMore: 'Learn more',
    comingSoon: 'Coming soon',
    external: 'opens in a new tab',
  },
};

export type Translations = typeof en;
