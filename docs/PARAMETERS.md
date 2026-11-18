# Maschinenparameter auslesen und schreiben

> **Status:** Nicht an einer realen DIALOG 11 geprüft. Das Format der Parameterdateien, die Bedienung an der Steuerung und der Schutz der Parametereingabe (Schlüsselschalter/Schlüsselzahl) sind **nicht verifiziert**. Alle Tests liefen gegen den Simulator mit einer erkennbar künstlichen Parameterliste.

## Grundsatz

DeckelLink behandelt Maschinenparameter ausschließlich als **Datendatei**:

| Vorgang | Wer löst aus? | Was DeckelLink tut |
|---|---|---|
| Auslesen | Bediener startet die **Parameterausgabe an der CNC** | empfängt die Daten (Klartext oder LSV2) und legt sie als Sicherung ab |
| Schreiben | Berechtigte Person **gibt die Parametereingabe an der CNC frei** | sendet eine Parameterdatei in diese Eingabe |

Es werden **keine Fernbefehle** an die Steuerung gesendet (insbesondere keine LSV2-Kommandos zum Lesen/Ändern einzelner Parameter): Deren Verwendung durch die DIALOG 11 ist nicht dokumentiert und würde eine Steuerungseigenschaft erfinden. DeckelLink kann eine Parametereingabe weder freischalten noch erzwingen.

## Auslesen (Sicherung)

1. *Maschinenparameter* → **Parameter auslesen (Sicherung)**.
2. An der CNC die Parameterausgabe starten.
3. Das Ende wird über **Inaktivität** erkannt (Standard 5 s; bei LSV2 über `EOT`). Ein Programmende (`M30`) wird bei Parametern nicht erwartet.
4. Ablage: `<Datenordner>/parameters/PARAM_JJJJMMTT_HHMMSS.par` plus Rohdaten (`parameters/raw/`) und Metadaten (JSON mit SHA-256, Profil, Port, Zeit). Dateien werden nie überschrieben.

Die Daten werden **unverändert** übernommen – nur Nullbytes werden entfernt und Zeilenenden vereinheitlicht. Leerzeichen, Leerzeilen, Tabulatoren, Semikolon- und Kommentarzeilen bleiben erhalten.

## Vergleichen

- **Mit aktueller Sicherung vergleichen**: zeilenweiser Vergleich (Zeilenenden werden ignoriert) mit Anzahl geänderter/hinzugefügter/entfernter Zeilen und Diff-Ansicht.

## Schreiben – Schutzmechanismen

Die Schaltfläche **Parameter an Maschine schreiben …** wird nur aktiv, wenn **alle** Bedingungen erfüllt sind:

1. Das Profil erlaubt das Schreiben (`parameters.allow_write`, Reiter *Maschinenparameter* im Profil-Editor). **Standard: aus** – auch in allen mitgelieferten Profilen. Das Ändern erzeugt eine Profilkopie.
2. Die Schreibfunktion ist **für diese Sitzung freigeschaltet** (Checkbox + Warnhinweis). Die Freischaltung wird nicht gespeichert und endet beim Beenden.
3. Es gibt eine **aktuelle Sicherung**: in dieser Sitzung mit demselben Profil und Anschluss ausgelesen, vollständig, höchstens `parameters.max_backup_age_min` (Standard 60) Minuten alt.
4. Eine Parameterdatei ist gewählt (aus dem Archiv oder extern), sie hat **keine Prüffehler** (7-Bit-ASCII, keine Steuerzeichen, optional Erkennungsmuster `parameters.identification_pattern`).
5. Die Datei **unterscheidet sich** von der aktuellen Sicherung (sonst gibt es nichts zu schreiben).
6. COM-Port gewählt bzw. Simulator aktiv, kein anderer Vorgang läuft.

Danach folgt ein Bestätigungsdialog mit Diff, Warnhinweis, Pflicht-Checkbox („Parametereingabe an der CNC durch eine berechtigte Person freigegeben …") und der **getippten Bestätigung `PARAMETER SCHREIBEN`**.

Geschrieben wird immer als Speicherübertragung (nie im Nachladebetrieb). Jeder Schreibvorgang wird mit Dateiname, SHA-256, Sicherung und Anzahl der Änderungen im Log protokolliert (ohne Parameterinhalte).

## Nach dem Schreiben – Kontrolle

Nach dem Senden ist die bisherige Sicherung ungültig. Lesen Sie die Parameter **erneut aus**: DeckelLink vergleicht automatisch die ausgelesenen mit den geschriebenen Daten und meldet „stimmen überein" oder „ABWEICHUNG" mit Diff. Die Maschine erst verfahren, wenn die Kontrolle passt und die Parameter nach Herstellervorgaben geprüft sind.

## Offene Prüfpunkte

Siehe [VERIFICATION_CHECKLIST.md](VERIFICATION_CHECKLIST.md), Punkte 34–40.
