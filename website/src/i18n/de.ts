import type { Translations } from './en';

export const de: Translations = {
  meta: {
    siteTitle: 'MAICENTA',
    defaultDescription:
      'MAICENTA ist ein freier, local-first Open-Source-Desktop-Workspace für E-Mail, Kalender, Aufgaben, Kontakte, Notizen und optionale KI-Assistenten.',
  },
  nav: {
    home: 'Start',
    features: 'Funktionen',
    pricing: 'Preise',
    download: 'Download',
    docs: 'Doku',
    support: 'Unterstützen',
    github: 'GitHub',
    menu: 'Menü',
    language: 'Sprache',
    skipToContent: 'Zum Inhalt springen',
  },
  footer: {
    tagline: 'Der offene Workspace für deinen digitalen Tag.',
    product: 'Produkt',
    project: 'Projekt',
    legal: 'Rechtliches',
    imprint: 'Impressum',
    privacy: 'Datenschutz',
    security: 'Sicherheitsrichtlinie',
    license: 'Lizenz',
    issues: 'Fehler melden',
    releases: 'Releases',
    patreon: 'Auf Patreon unterstützen',
    supporters: 'Unterstützer',
    copyright:
      'MAICENTA ist frei und Open Source. Name und Logo von MAICENTA sind nicht Teil der Softwarelizenz.',
  },
  home: {
    title: 'MAICENTA – Der offene Workspace für deinen digitalen Tag',
    description:
      'E-Mail, Kalender, Aufgaben, Kontakte, Notizen und optionale KI in einer freien, local-first Desktop-App für Windows, macOS und Linux. Open Source, ohne Zwangs-Account.',
    badge: 'Frühe Alpha · Windows, macOS, Linux',
    headline: 'Der offene Workspace für deinen digitalen Tag.',
    subline:
      'MAICENTA bringt E-Mail, Kalender, Aufgaben, Kontakte, Notizen und optionale KI-Assistenten in einer Desktop-App zusammen. Local-first, Open Source und ohne Zwangs-Account.',
    ctaDownload: 'Alpha herunterladen',
    ctaGithub: 'Auf GitHub ansehen',
    alphaNote:
      'MAICENTA ist eine frühe Alpha mit echter IMAP/SMTP- und Microsoft-365-Anbindung. Teste sie zuerst mit einem unkritischen Konto.',
    principlesTitle: 'Gebaut auf Prinzipien, nicht auf Lock-in',
    principlesIntro:
      'Eine offene Alternative zu Outlook, Thunderbird und reinen Cloud-Suiten. Deine Konten, deine Daten und deine Entscheidungen bleiben bei dir.',
    principles: [
      {
        title: 'Frei und Open Source',
        text: 'Die Desktop-App ist kostenlos, für immer. Der Code ist öffentlich und du kannst ihn lesen, bauen und verbessern.',
      },
      {
        title: 'Local-first und offline',
        text: 'Postfach, Kalender und Notizen liegen auf deinem Gerät. Alles funktioniert ohne Verbindung und synchronisiert, sobald du wieder online bist.',
      },
      {
        title: 'Privacy by Design',
        text: 'Keine Telemetrie, kein Tracking, keine versteckten Uploads. Profile sind verschlüsselt und bleiben unter deiner Kontrolle.',
      },
      {
        title: 'Kein Zwangs-Account',
        text: 'MAICENTA verlangt nie ein MAICENTA-Konto, einen Cloud-Dienst oder einen Webserver. Optionale Dienste bleiben optional.',
      },
      {
        title: 'Offene Standards',
        text: 'IMAP, SMTP, OAuth 2.0, iCalendar, vCard, CalDAV und CardDAV. Du kannst jederzeit gehen und deine Daten mitnehmen.',
      },
      {
        title: 'Optionale KI mit Berechtigungen',
        text: 'Nutze lokale oder externe KI-Anbieter, wenn du willst. Mit feinen Berechtigungen und standardmäßig nichts aktiviert.',
      },
    ],
    modulesTitle: 'Ein Workspace, viele Module',
    modulesIntro:
      'Jedes Modul lässt sich ein- oder ausschalten. Deaktivierte Module verschwinden aus der Navigation und stoppen Hintergrundarbeit, deine Daten bleiben aber erhalten, bis du sie löschst.',
    modules: [
      { name: 'Mail', text: 'IMAP/SMTP- und Microsoft-365-Konten, Offline-Cache, Suche, Identitäten.', phase: 'Verfügbar' },
      { name: 'Vault', text: 'Verschlüsselter Profil-Export, -Import und Backup. Die Grundlage für Sync.', phase: 'Verfügbar' },
      { name: 'Kalender', text: 'Lokale Kalender jetzt, iCalendar und CalDAV später.', phase: 'Phase 2' },
      { name: 'Aufgaben', text: 'Lokale Aufgaben jetzt, VTODO und CalDAV später.', phase: 'Phase 2' },
      { name: 'Kontakte', text: 'Lokale Kontakte jetzt, vCard und CardDAV später.', phase: 'Phase 2' },
      { name: 'Notizen', text: 'Persönliche Notizen direkt im Workspace.', phase: 'Später' },
      { name: 'Assistent', text: 'Optionale lokale oder externe KI-Anbieter.', phase: 'Später' },
      { name: 'Erweiterungen', text: 'Berechtigungsbasierte Plugins von Dritten.', phase: 'Später' },
    ],
    providersTitle: 'Funktioniert mit den Konten, die du schon hast',
    providersIntro:
      'MAICENTA ist als offizieller Client bei Microsoft, Google und Apple registriert. Die Anmeldung läuft über OAuth 2.0, ohne dass dein Passwort gespeichert wird.',
    providers: [
      { name: 'Microsoft 365 und Outlook.com', text: 'Über Microsoft Graph, auch in Tenants mit deaktiviertem IMAP.' },
      { name: 'Google Workspace und Gmail', text: 'OAuth-2.0-Anmeldung mit IMAP und SMTP.' },
      { name: 'Jeder IMAP/SMTP-Anbieter', text: 'Autodiscovery für gängige Anbieter, manuelle Einrichtung für alle anderen.' },
    ],
    syncTitle: 'Dein Workspace auf jedem Gerät. Optional.',
    syncText:
      'MAICENTA speichert dein Profil in einem verschlüsselten Vault. Bald kannst du diesen Vault zwischen deinen Geräten synchronisieren, entweder über einen Speicher deiner Wahl oder über MAICENTA Sync, einen kleinen bezahlten Dienst, der immer nur verschlüsselte Daten sieht.',
    syncCta: 'Preise ansehen',
    openSourceTitle: 'Offene Entwicklung',
    openSourceText:
      'Roadmap, Architektur und Sicherheitsrichtlinie sind öffentlich. Beiträge, Fehlerberichte und Ideen sind auf GitHub willkommen.',
    openSourceCta: 'Roadmap lesen',
    supportTitle: 'Kostenlos für alle. Finanziert von wenigen.',
    supportText:
      'MAICENTA hat keine Werbung, keine Telemetrie und keine Bezahlfunktionen in der App. Was es hat, sind echte Kosten: Entwicklerkonten bei Microsoft, Google und Apple, Code-Signing-Zertifikate und bald Sync-Server. Patreon-Mitglieder tragen sie.',
    supportCta: 'Auf Patreon unterstützen',
    supportSecondary: 'Wer schon dabei ist',
  },
  support: {
    title: 'MAICENTA unterstützen',
    description:
      'Hilf, einen unabhängigen Open-Source-Workspace zu finanzieren. Patreon-Mitgliedschaften zahlen Entwicklerkonten, Code-Signing und Sync-Server.',
    headline: 'MAICENTA unterstützen',
    intro:
      'Die App ist kostenlos und bleibt es. Deine Mitgliedschaft zahlt, was Open Source allein nicht kann: die Entwicklerkonten, die die offizielle Anmeldung bei Microsoft, Google und Apple am Laufen halten, Code-Signing, damit Installer keine Warnungen auslösen, und später die Server für den optionalen verschlüsselten Sync.',
    ctaPatreon: 'Mitglied werden auf Patreon',
    tiersTitle: 'Mitgliedschaften',
    tiersIntro: 'Alle Mitgliedschaften werden über Patreon abgerechnet und sind dort jederzeit kündbar.',
    tiers: [
      { name: 'Supporter', price: '1 € / Monat', text: 'Auf Wunsch dein Name in der Liste unten und Zugang zu Beiträgen nur für Mitglieder.' },
      { name: 'Insider', price: '5 € / Monat', text: 'Entwicklungsberichte, Vorschauen und Umfragen zu Prioritäten. MAICENTA Sync inklusive, sobald es startet.' },
      { name: 'Backer', price: '12 € / Monat', text: 'Alles aus Insider, dazu Name und Link auf dieser Seite, direkter Kontakt für Feedback und frühe Testbuilds.' },
      { name: 'Project Sponsor', price: '30 € / Monat', text: 'Alles aus Backer, dazu ein geprüfter Feature-Vorschlag pro Quartal und dein Logo auf dieser Seite. Begrenzt auf 15.' },
    ],
    otherTitle: 'Andere Wege zu helfen',
    other: [
      { title: 'Alpha testen', text: 'Nutze MAICENTA mit einem unkritischen Konto und melde, was nicht funktioniert. Jeder gute Fehlerbericht spart Stunden.', cta: 'Issue eröffnen', href: 'issues' },
      { title: 'Doku verbessern', text: 'Einen Tippfehler korrigieren, einen Schritt klarer erklären oder eine Seite übersetzen. Die Doku liegt im Repository neben dem Code.', cta: 'Contributing lesen', href: 'contributing' },
      { title: 'Weitersagen', text: 'Erzähl es jemandem, der Outlook leid ist. Ein Stern auf GitHub hilft anderen, das Projekt zu finden.', cta: 'Auf GitHub sternen', href: 'github' },
    ],
    listTitle: 'Unterstützer',
    listIntro: 'Danke an alle, die MAICENTA unabhängig halten. Genannt mit Einverständnis.',
    sponsors: 'Project Sponsors',
    backers: 'Backer',
    supporters: 'Supporter',
    empty: 'Die Liste ist noch leer. Die ersten Namen erscheinen hier bald.',
    emptyCta: 'Sei die oder der Erste',
  },
  pricing: {
    title: 'Preise',
    description:
      'Die MAICENTA-Desktop-App ist für immer kostenlos. MAICENTA Sync ist ein optionales, günstiges Abo für verschlüsselte Synchronisation zwischen Geräten.',
    headline: 'Kostenlose App. Optionaler Sync.',
    intro:
      'MAICENTA ist frei und Open Source und bleibt es. Das optionale Sync-Abo finanziert die Server sowie die Entwicklerkonten bei Microsoft, Google und Apple, die die offiziellen Integrationen am Laufen halten.',
    freeName: 'MAICENTA Desktop',
    freePrice: '0 €',
    freePeriod: 'für immer',
    freeFeatures: [
      'Alle Module auf Windows, macOS und Linux',
      'Unbegrenzt viele Konten und Postfächer',
      'Verschlüsselte lokale Profile, Export und Backup',
      'Sync über deinen eigenen Speicher (geplant)',
      'Kein Konto, keine Telemetrie, keine Werbung',
      'Community-Support auf GitHub',
    ],
    freeCta: 'Herunterladen',
    syncName: 'MAICENTA Sync',
    syncBadgePlanned: 'Geplant',
    syncPriceTba: 'Preis wird noch bekannt gegeben',
    perMonth: '/ Monat',
    perYear: '/ Jahr',
    syncTagline: 'Verschlüsselter Vault-Sync zwischen deinen Geräten, gehostet für dich.',
    syncFeatures: [
      'Alles aus MAICENTA Desktop',
      'Synchronisiere deinen verschlüsselten Vault auf allen Geräten',
      'Ende-zu-Ende verschlüsselt: der Server speichert nur Ciphertext',
      'Kein eigener Speicher nötig',
      'Jederzeit kündbar, jederzeit exportierbar',
      'Unterstützt die Weiterentwicklung von MAICENTA',
    ],
    syncCta: 'Abonnieren',
    syncCtaPlanned: 'Start auf GitHub verfolgen',
    syncPortal: 'Abo verwalten',
    patreonNote: 'Schon Patreon-Mitglied ab Insider? MAICENTA Sync ist in deiner Mitgliedschaft enthalten, sobald es startet.',
    patreonLink: 'Auf Patreon unterstützen',
    faqTitle: 'Fragen und Antworten',
    faq: [
      {
        q: 'Ist die App wirklich kostenlos?',
        a: 'Ja. Die Desktop-App ist kostenlos und Open Source. Alle Module, alle Konten, keine Limits und keine Werbung. Sync ist die einzige bezahlte Option, und sie ist optional.',
      },
      {
        q: 'Brauche ich ein Konto, um MAICENTA zu nutzen?',
        a: 'Nein. MAICENTA funktioniert vollständig ohne MAICENTA-Konto. Du meldest dich nur bei deinen eigenen Mail-Anbietern an.',
      },
      {
        q: 'Was sieht der Sync-Server?',
        a: 'Nur verschlüsselte Vault-Objekte und die Metadaten, die zum Speichern nötig sind, etwa Größe und Zeitstempel. Schlüssel verlassen deine Geräte nie. Wir können weder Mails noch Kalender oder Notizen lesen.',
      },
      {
        q: 'Kann ich auch ohne zu zahlen synchronisieren?',
        a: 'Das ist der Plan. Verschlüsselter Vault-Sync über einen Speicher deiner Wahl, zum Beispiel WebDAV oder S3-kompatibler Speicher, steht auf der Roadmap und wird kostenlos sein.',
      },
      {
        q: 'Läuft meine E-Mail über MAICENTA Sync?',
        a: 'Nein. Deine E-Mail bleibt bei deinem Mail-Anbieter und wird direkt von der App abgerufen. Sync betrifft nur dein verschlüsseltes Profil: Einstellungen, Konten, lokale Kalender, Aufgaben, Kontakte und Notizen.',
      },
      {
        q: 'Wie kündige ich?',
        a: 'Über das Kundenportal des Zahlungsanbieters, mit einem Klick und ohne uns kontaktieren zu müssen. Deine lokalen Daten sind von einer Kündigung nie betroffen.',
      },
    ],
  },
  download: {
    title: 'Download',
    description: 'Lade die MAICENTA-Alpha für Windows, macOS und Linux herunter oder baue sie aus dem Quellcode.',
    headline: 'MAICENTA herunterladen',
    intro:
      'MAICENTA ist in einer frühen Alpha. Builds erscheinen auf GitHub Releases. Bitte teste zuerst mit einem unkritischen Konto und melde, was nicht funktioniert.',
    platforms: [
      { name: 'Windows', text: 'Windows 10 oder neuer, 64-Bit.' },
      { name: 'macOS', text: 'macOS 12 oder neuer, Apple Silicon und Intel.' },
      { name: 'Linux', text: '64-Bit-Desktop-Distributionen.' },
    ],
    releaseCta: 'Zu den Releases',
    sourceTitle: 'Aus dem Quellcode bauen',
    sourceText:
      'MAICENTA besteht aus einem Rust-Kern und einem Flutter-Desktop-Client. Die README im Repository erklärt Toolchain und Build-Schritte.',
    sourceCta: 'Repository öffnen',
    alphaTitle: 'Was Alpha bedeutet',
    alphaPoints: [
      'Echte IMAP/SMTP- und Microsoft-365-Anbindung mit bewusst begrenzter Synchronisation.',
      'Einige anbieterspezifische und Wiederherstellungs-Abläufe sind noch nicht vollständig.',
      'Profilformate können sich zwischen Alpha-Versionen noch ändern. Nutze Export und Backup.',
    ],
  },
  docs: {
    title: 'Dokumentation',
    description: 'Einstiegsanleitungen und Dokumentation für MAICENTA.',
    headline: 'Dokumentation',
    intro: 'Kurze Anleitungen, um MAICENTA zum Laufen zu bringen und mit deinen Konten zu verbinden.',
    readMore: 'Lesen',
    backToDocs: 'Alle Artikel',
    editHint: 'Fehler gefunden? Die Doku liegt im Repository, Pull Requests sind willkommen.',
  },
  legal: {
    imprintTitle: 'Impressum',
    privacyTitle: 'Datenschutzerklärung',
  },
  common: {
    learnMore: 'Mehr erfahren',
    comingSoon: 'Bald verfügbar',
    external: 'öffnet in neuem Tab',
  },
};
