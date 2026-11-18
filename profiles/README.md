# Mitgelieferte Profile

Die mitgelieferten DIALOG-11-Profile liegen als Paketdaten unter
[`src/deckellink/resources/profiles/`](../src/deckellink/resources/profiles/), damit sie in
Python-Paket und Windows-Build enthalten sind (siehe `docs/ARCHITECTURE.md`):

- `dialog11_7n1_rtscts.json` – DIALOG 11 – 4800 7N1 RTS/CTS (nicht verifiziert)
- `dialog11_7e1_none.json` – DIALOG 11 – 4800 7E1 ohne Handshake (nicht verifiziert)
- `dialog11_lsv2_7e1.json` – DIALOG 11 – LSV2 4800 7E1 (Nachladebetrieb) (nicht verifiziert)

Benutzerprofile speichert DeckelLink im Ordner `profiles/` des Datenverzeichnisses
(Windows: `%LOCALAPPDATA%\DeckelLink\profiles`). Mitgelieferte Profile werden nie überschrieben.
