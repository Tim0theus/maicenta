
## Installation

**Windows:** `MAICENTA-…-windows-x64-Setup.exe` ausführen. MAICENTA wird für
den aktuellen Benutzer installiert und aktualisiert sich danach selbst. Die
Vorabversion ist noch nicht signiert: Bei „Der Computer wurde durch Windows
geschützt“ auf „Weitere Informationen“ und dann „Trotzdem ausführen“ klicken.
Alternativ lässt sich die portable ZIP entpacken und `maicenta.exe` starten.

**Linux (x86_64):** `MAICENTA-…-linux-x64.AppImage` ausführbar machen
(`chmod +x`) und starten. Benötigt FUSE 2 (Paket `libfuse2`), GTK 3,
WebKitGTK 4.1, libsecret und einen laufenden Secret Service wie GNOME Keyring
oder KWallet. Das AppImage aktualisiert sich selbst.

**macOS:** Die DMG öffnen und MAICENTA in den Ordner „Programme“ ziehen. Die
App ist noch nicht von Apple beglaubigt, und seit macOS 15 hilft der
Rechtsklick auf „Öffnen“ nicht mehr. So startest du sie:

1. MAICENTA einmal starten und den Hinweis „‚MAICENTA‘ nicht geöffnet“ mit
   **Fertig** schließen (nicht „In den Papierkorb legen“).
2. **Systemeinstellungen → Datenschutz & Sicherheit** öffnen, nach unten zu
   „Sicherheit“ scrollen und bei „‚MAICENTA‘ wurde blockiert“ auf **Dennoch
   öffnen** klicken; mit Passwort oder Touch ID bestätigen. Der Knopf
   erscheint nur etwa eine Stunde nach dem Startversuch.
3. MAICENTA erneut starten und **Dennoch öffnen** wählen.

Alternativ im Terminal:
`xattr -dr com.apple.quarantine /Applications/MAICENTA.app`

Nach einem Update über eine neue DMG ist die Freigabe erneut nötig. MAICENTA
meldet neue Versionen selbst; installiert wird über die neue DMG.

Profile werden beim ersten Start einer neuen Version übernommen. Vor einem
Beta-Update empfiehlt sich ein passwortgeschützter Export.

Prüfsummen stehen in `SHA256SUMS.txt`. Die Dateien `releases.*.json`,
`RELEASES*` und `*.nupkg` sind für das In-App-Update bestimmt.

## Installation (English)

**Windows:** run `MAICENTA-…-windows-x64-Setup.exe`. MAICENTA is installed for
the current user and updates itself from then on. The pre-release is not
signed yet: at "Windows protected your PC", click "More info" and then "Run
anyway". Alternatively, unpack the portable ZIP and start `maicenta.exe`.

**Linux (x86_64):** make `MAICENTA-…-linux-x64.AppImage` executable
(`chmod +x`) and start it. Requires FUSE 2 (package `libfuse2`), GTK 3,
WebKitGTK 4.1, libsecret, and a running Secret Service such as GNOME Keyring
or KWallet. The AppImage updates itself.

**macOS:** open the DMG and drag MAICENTA into the Applications folder. The app
is not notarized by Apple yet, and since macOS 15 right-click → "Open" no
longer helps. To start it:

1. Launch MAICENTA once and close the "“MAICENTA” Not Opened" message with
   **Done** (not "Move to Trash").
2. Open **System Settings → Privacy & Security**, scroll down to "Security",
   and next to "“MAICENTA” was blocked" click **Open Anyway**; confirm with
   your password or Touch ID. The button only appears for about an hour after
   the launch attempt.
3. Launch MAICENTA again and choose **Open Anyway**.

Alternatively, in Terminal:
`xattr -dr com.apple.quarantine /Applications/MAICENTA.app`

After updating from a new DMG, the approval is needed again. MAICENTA reports
new versions itself; you install them from the new DMG.

Profiles are carried over on the first start of a new version. Before a beta
update, a password-protected export is recommended.

Checksums are in `SHA256SUMS.txt`. The files `releases.*.json`, `RELEASES*`,
and `*.nupkg` are used by the in-app update.
