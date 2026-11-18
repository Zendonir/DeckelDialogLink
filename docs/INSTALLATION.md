# Installation

## Installer (empfohlen)

1. Von der GitHub-Release-Seite `DeckelLink-<version>-Setup-x64.exe` herunterladen und optional mit `SHA256SUMS.txt` prüfen (`Get-FileHash DeckelLink-<version>-Setup-x64.exe`).
2. Setup starten. Es sind **keine Administratorrechte** nötig; installiert wird nach `%LOCALAPPDATA%\Programs\DeckelLink` (bei Ausführung als Administrator wahlweise für alle Benutzer).
3. Startmenü: „DeckelLink" und „DeckelLink (Simulationsmodus)".
4. Aktualisieren: neuen Installer einfach darüber installieren. Deinstallieren: Windows-Einstellungen → Apps. Ihre Daten unter `%LOCALAPPDATA%\DeckelLink` bleiben in beiden Fällen erhalten.

Hinweis: Der Installer ist nicht code-signiert. Windows SmartScreen kann deshalb warnen („Weitere Informationen → Trotzdem ausführen"). Prüfen Sie im Zweifel die SHA-256-Prüfsumme.

Stille Installation (z. B. Softwareverteilung): `DeckelLink-<version>-Setup-x64.exe /VERYSILENT /CURRENTUSER`.

## Portable Windows-Anwendung (Anwender)

1. ZIP `DeckelLink-<version>-windows-x64.zip` aus den GitHub-Actions-Artefakten bzw. dem Release herunterladen.
2. Vollständig in einen beschreibbaren Ordner entpacken (z. B. `C:\Tools\DeckelLink`). Administratorrechte sind nicht nötig.
3. `DeckelLink\DeckelLink.exe` starten. Es wird keine Python-Installation benötigt.
4. Optionaler Selbsttest: `DeckelLink.exe --smoke-test --smoke-report %TEMP%\dl.json` (Exit-Code 0 = OK).

Die Anwendung startet im **Simulationsmodus**. Für den Realbetrieb USB-RS-232-Adapter anschließen, unter *Einstellungen* „Realbetrieb über COM-Port" und den COM-Port wählen.

### Datenverzeichnisse (platformdirs)

| Inhalt | Windows (Standard) |
|---|---|
| Einstellungen, Profile, empfangene Programme, Backups, Rohdaten, Exporte | `%LOCALAPPDATA%\DeckelLink\` (`config`, `profiles`, `received`, `failed`, `raw`, `backups`, `exports`, `simulation`) |
| Logs | `%LOCALAPPDATA%\DeckelLink\Logs\` |

Abweichend: Parameter `--data-dir <Ordner>` oder Umgebungsvariable `DECKELLINK_HOME` (z. B. für einen USB-Stick). In das Programmverzeichnis wird nie geschrieben.

## Entwicklungsumgebung

```bash
python -m venv .venv
.venv\Scripts\activate            # Linux: source .venv/bin/activate
python -m pip install -e ".[dev]"
python -m deckellink
```

### Linux (headless/CI)
PySide6 benötigt Systembibliotheken, u. a. `libegl1`, `libgl1`, `libxkbcommon0`, `libfontconfig1`, `libdbus-1-3`. Für Tests ohne Display: `export QT_QPA_PLATFORM=offscreen`.

## Windows-Build selbst erstellen

```bash
python -m pip install -e ".[dev,build]"
pyinstaller --noconfirm --clean deckellink.spec
dist\DeckelLink\DeckelLink.exe --smoke-test
```

### Warum Onedir statt Onefile?
Ein Onefile-Build entpackt bei jedem Start ca. 100–200 MB PySide6-Bibliotheken in ein temporäres Verzeichnis. Das verlängert den Start deutlich, wird häufig von Virenscannern beanstandet und scheitert auf Rechnern mit restriktiven Temp-Richtlinien. Der Onedir-Build (`dist/DeckelLink/`) ist zuverlässiger und trotzdem portabel (Ordner kopieren genügt).

Installer selbst bauen (Inno Setup 6 installiert):

```bash
iscc /DAppVersion=0.4.1 installer\deckellink.iss
```

Ergebnis: `dist\installer\DeckelLink-0.4.1-Setup-x64.exe`.

### Release erstellen
Version in `src/deckellink/version.py` und `CHANGELOG.md` pflegen, dann Tag `v<version>` pushen. Der Workflow `build-windows.yml` baut, testet (auch eine stille Installation mit Selbsttest und Deinstallation) und veröffentlicht das GitHub-Release mit Installer, ZIP und Prüfsummen. Versionen `0.x` werden als Vorabversion markiert.

Der Build enthält die Profile (`_internal/deckellink/resources/profiles`), Icons und Windows-Versionsinformationen. Nicht benötigte Qt-Module (WebEngine, QML, 3D, Multimedia …) werden ausgeschlossen.
