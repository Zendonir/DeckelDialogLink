# Mitwirken an DeckelLink

Danke für Ihr Interesse! Bitte lesen Sie zuerst [CLAUDE.md](CLAUDE.md) (verbindliche Projektregeln) und [docs/SAFETY.md](docs/SAFETY.md).

## Entwicklungsumgebung

```bash
python -m venv .venv && source .venv/bin/activate   # Windows: .venv\Scripts\activate
python -m pip install -e ".[dev]"
export QT_QPA_PLATFORM=offscreen                     # für GUI-Tests ohne Display
```

## Vor jedem Pull Request

```bash
ruff check src tests
ruff format --check src tests
mypy
pytest --cov=deckellink
```

Alle Prüfungen müssen grün sein; die Coverage-Grenze liegt bei 80 %.

## Richtlinien

- UI-Texte auf Deutsch, Code und Kommentare auf Englisch.
- Neue Funktionen mit Tests; Transferlogik mit `FakeClock` und `SimulatedTransport` testen, niemals mit echten COM-Ports.
- Keine Funktionen, die Maschinenausführung, Achsen oder Modi beeinflussen.
- Erkenntnisse von realen Maschinen bitte mit Steuerungsversion, Parametern, Kabelbelegung und Ergebnis in `docs/VERIFICATION_CHECKLIST.md` dokumentieren. Behaupten Sie keine Eigenschaften, die nicht geprüft wurden.

## Fehler melden

Bitte ein Diagnosepaket (Diagnose → Diagnosepaket exportieren, Pfade maskiert) anhängen. NC-Programme nur beifügen, wenn Sie dazu berechtigt sind.
