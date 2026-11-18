# Installation

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

Der Build enthält die Profile (`_internal/deckellink/resources/profiles`), Icons und Windows-Versionsinformationen. Nicht benötigte Qt-Module (WebEngine, QML, 3D, Multimedia …) werden ausgeschlossen.
