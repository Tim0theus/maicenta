---
title: Konten verbinden
description: Wie MAICENTA sich bei Microsoft, Google und klassischen IMAP/SMTP-Anbietern anmeldet, und was zu tun ist, wenn es nicht klappt.
order: 2
---

## So funktioniert die Anmeldung

MAICENTA ist als offizieller Client bei Microsoft, Google und Apple registriert. Wo ein Anbieter OAuth 2.0 anbietet, öffnet MAICENTA deinen Browser, du meldest dich direkt beim Anbieter an, und MAICENTA erhält nur ein Zugriffstoken. Dein Passwort wird nie in MAICENTA eingegeben oder gespeichert.

Alle Tokens und Zugangsdaten liegen in deinem verschlüsselten Profil auf deinem Gerät.

## Microsoft 365 und Outlook.com

MAICENTA spricht mit Microsoft über die Microsoft-Graph-API. Das funktioniert für private Outlook.com-Konten ebenso wie für Geschäfts- und Schulkonten, auch in Tenants, in denen IMAP und SMTP AUTH per Richtlinie deaktiviert sind.

Heute unterstützt: Delta-Synchronisation pro Ordner, Lesen und Durchsuchen von Mails, Anhänge, Gelesen- und Markierungsänderungen, Verschieben von Nachrichten, serverseitige Entwürfe und Versand.

Wenn deine Organisation Drittanbieter-Apps einschränkt, muss ein Administrator MAICENTA möglicherweise einmalig freigeben.

## Google Workspace und Gmail

MAICENTA meldet sich über Google mit OAuth 2.0 an und nutzt danach IMAP zum Lesen und SMTP zum Senden. IMAP muss in deinen Gmail-Einstellungen aktiviert sein; Google-Workspace-Administratoren können es auch zentral abschalten.

## Andere IMAP/SMTP-Anbieter

Gib deine E-Mail-Adresse ein, und MAICENTA sucht bekannte Servereinstellungen. Wird dein Anbieter nicht erkannt, trägst du ein:

- IMAP-Host, Port und Verschlüsselung (meist Port 993 mit TLS)
- SMTP-Host, Port und Verschlüsselung (meist Port 465 mit TLS oder 587 mit STARTTLS)
- Deinen Benutzernamen und ein App-Passwort, falls dein Anbieter eines anbietet

Anbieter, die OAuth verlangen, aber noch nicht nativ unterstützt werden, findest du in der [Roadmap](https://github.com/Tim0theus/maicenta/blob/main/ROADMAP.md).

## Fehlersuche

**Das Browserfenster schließt sich, aber MAICENTA macht nicht weiter.**
Stelle sicher, dass MAICENTA noch läuft und keine Firewall die lokale Weiterleitung blockiert. Versuche die Anmeldung erneut.

**Ordner erscheinen, aber es werden keine Nachrichten geladen.**
Die Synchronisation ist in der Alpha bewusst begrenzt. Öffne einen Ordner, um seine aktuellen Nachrichten zu laden. Ältere Nachrichten werden bei Bedarf nachgeladen.

**Ich habe mein Passwort beim Anbieter geändert.**
Entferne das Konto und füge es erneut hinzu. Vorhandene lokale Daten bleiben erhalten.

Alles andere: Eröffne ein Issue auf [GitHub](https://github.com/Tim0theus/maicenta/issues) mit dem Namen des Anbieters und der genauen Fehlermeldung. Bitte niemals Tokens oder Passwörter einfügen.
