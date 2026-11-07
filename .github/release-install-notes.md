
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
App ist noch nicht von Apple beglaubigt; beim ersten Start mit Rechtsklick →
„Öffnen“ bestätigen. MAICENTA meldet neue Versionen, die Installation erfolgt
über die neue DMG.

Profile werden beim ersten Start einer neuen Version übernommen. Vor einem
Beta-Update empfiehlt sich ein passwortgeschützter Export.

Prüfsummen stehen in `SHA256SUMS.txt`. Die Dateien `releases.*.json`,
`assets.*.json`, `RELEASES` und `*.nupkg` sind für das In-App-Update bestimmt.
