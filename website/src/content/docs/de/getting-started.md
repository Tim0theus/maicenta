---
title: Erste Schritte
description: MAICENTA installieren, ein Profil anlegen und in wenigen Minuten ein Mailkonto verbinden.
order: 1
---

## 1. Installieren

Lade den Build für deine Plattform von [GitHub Releases](https://github.com/Tim0theus/maicenta/releases) herunter und starte die App. MAICENTA läuft auf Windows, macOS und Linux.

MAICENTA befindet sich in einer frühen Alpha. Nutze für erste Tests ein unkritisches Mailkonto und behalte die Release-Notes im Blick: Profilformate können sich zwischen Alpha-Versionen noch ändern.

## 2. Profil anlegen

Beim ersten Start legt MAICENTA ein lokales Profil an. Ein Profil enthält deine Konten, Einstellungen, lokalen Kalender, Aufgaben, Kontakte und Notizen. Es wird verschlüsselt auf deinem Gerät gespeichert und verlässt es nur, wenn du es exportierst oder Sync einrichtest.

Wähle ein starkes Profilpasswort. Es schützt die Verschlüsselungsschlüssel deines Profils. MAICENTA kann es nicht für dich wiederherstellen.

## 3. Mailkonto hinzufügen

Öffne **Konten** und gib deine E-Mail-Adresse ein. MAICENTA versucht, die passenden Einstellungen automatisch zu erkennen:

- **Microsoft 365, Outlook.com**: Anmeldung über Microsoft im Browser. MAICENTA nutzt Microsoft Graph und funktioniert deshalb auch in Tenants, in denen IMAP abgeschaltet ist.
- **Google Workspace, Gmail**: Anmeldung über Google im Browser. Mails werden per IMAP abgerufen und per SMTP mit OAuth 2.0 versendet.
- **Andere Anbieter**: MAICENTA sucht bekannte IMAP- und SMTP-Einstellungen. Wird nichts gefunden, gibst du Host, Port und Verschlüsselung manuell ein.

Wenn OAuth verfügbar ist, speichert MAICENTA dein Anbieter-Passwort nie. Tokens liegen ausschließlich in deinem verschlüsselten Profil.

Details zu den Anbietern und zur Fehlersuche findest du unter [Konten verbinden](/de/docs/accounts/).

## 4. Offline arbeiten

Nachrichten, Ordner und Markierungen werden lokal zwischengespeichert. Du kannst Mails ohne Verbindung lesen, durchsuchen, markieren und Entwürfe schreiben. Änderungen werden in eine Warteschlange gestellt und synchronisiert, sobald du wieder online bist.

## 5. Profil sichern

Erstelle über **Vault → Export** ein verschlüsseltes Backup deines gesamten Profils. Importiere es auf einem anderen Gerät, um deinen Workspace umzuziehen. Verschlüsselter Sync zwischen Geräten ist der nächste Schritt auf der Roadmap.

## Hilfe bekommen

- Fehler und Fragen: [GitHub Issues](https://github.com/Tim0theus/maicenta/issues)
- Sicherheitsmeldungen: siehe [Sicherheitsrichtlinie](https://github.com/Tim0theus/maicenta/blob/main/SECURITY.md)
