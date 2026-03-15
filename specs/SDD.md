# SDD: DeepSeek PDF to Markdown

## 1. Systemuebersicht

`pdf-ocr` ist eine Rust-basierte CLI, die PDFs in strukturiertes Markdown umwandelt. Die Anwendung laedt kein OCR-Modell lokal, sondern fungiert als Orchestrator fuer einen laufenden DeepSeek-OCR HTTP-Server, standardmaessig unter `http://localhost:8000/v1`.

Die Pipeline umfasst:

1. PDF-Inspektion und Seitenmetadaten-Extraktion.
2. Rasterisierung einzelner Seiten in Bilder.
3. Umwandlung der Bilder in kompaktes WebP fuer den Transport.
4. OCR-Inferenz ueber einen lokalen HTTP-Endpunkt.
5. Nachbearbeitung und Zusammenbau zu einem Markdown-Dokument mit YAML-Frontmatter.

## 2. Ziele

- Stabiler Rust-Orchestrator ohne direkte Abhaengigkeit von CUDA- oder Python-Inferenzlogik.
- Austauschbarer Backend-Endpunkt durch Konfiguration.
- Effiziente lokale Verarbeitung mit Cache, Fortschrittsanzeige und sauberem Fehlerverhalten.
- Robuste Teilergebnisse bei Abbruch oder Serverproblemen.

## 3. Komponenten

### 3.1 CLI Layer

Verantwortlichkeiten:

- Argumente einlesen via `clap`
- Pre-Flight-Pruefungen ausloesen
- Laufzeitparameter wie Server-URL, Zielpfad, Cache und Parallelitaet an die Pipeline uebergeben

Unterstuetzte Argumente:

- `input`: Eingabe-PDF
- `--output`, `-o`: Zielpfad fuer Markdown
- `--server`: Backend-URL, Default `http://localhost:8000/v1`
- `--model`: Modellname, Default `deepseek-ocr`
- `--concurrent`: Maximale parallele OCR-Requests, Default `1`
- `--no-cache`: Erzwingt Neuverarbeitung

### 3.2 PDF Ingestion und Rasterization

Verantwortlichkeiten:

- PDF laden und Seitenzahl bestimmen
- PDF-Metadaten via `lopdf` lesen
- Seitengroessen ermitteln, um adaptive DPI zu berechnen
- Seiten ueber Poppler (`pdftoppm`) in PNG rendern
- PNG in `image::DynamicImage` laden
- `DynamicImage` in WebP serialisieren und Base64-kodieren

Designentscheidungen:

- Adaptive DPI wird pro Seite berechnet.
- Ziel ist eine maximale Kantenlaenge von `1536px`.
- Rasterisierung erfolgt seitenweise, um Speicher und Fehlerisolierung zu verbessern.

### 3.3 Inference Orchestrator

Verantwortlichkeiten:

- DeepSeek-Server beim Start pruefen
- OCR-Requests seriell oder begrenzt parallel absetzen
- Antworten normalisieren und Fehler sauber klassifizieren
- `tokio::sync::Semaphore` zur Begrenzung gleichzeitiger Requests einsetzen

HTTP-Vertrag der aktuellen Implementierung:

- Health Check: `GET /models`, Fallback auf `GET /health` und `GET /`
- OCR: `POST /chat/completions`
- Request-Body (OpenAI-kompatibel):

```json
{
  "model": "deepseek-ocr",
  "messages": [
    {
      "role": "user",
      "content": [
        {
          "type": "text",
          "text": "Extract the text from page 3 as clean Markdown. Return only Markdown without code fences or commentary."
        },
        {
          "type": "image_url",
          "image_url": {
            "url": "data:image/webp;base64,<base64>"
          }
        }
      ]
    }
  ]
}
```

Antworten duerfen flexibel sein. Die Implementierung akzeptiert OpenAI-`choices[0].message.content` sowie Legacy-Felder wie `markdown`, `text`, `content` oder verschachtelte Varianten unter `data`.

### 3.4 Cache Layer

Verantwortlichkeiten:

- SHA-256 ueber das WebP der Seite bilden
- Treffer im lokalen Cache vor dem OCR-Request auslesen
- OCR-Ergebnisse als Markdown-Fragmente persistieren

Cache-Pfad:

- Plattformabhaengiges Cache-Verzeichnis via `directories`
- Unterordner `pages/`

### 3.5 Post-Processing Engine

Verantwortlichkeiten:

- OCR-Artefakte und Markdown-Codefences entfernen
- Seitenfragmente sortiert zusammensetzen
- YAML-Frontmatter generieren
- Optionales Inhaltsverzeichnis aus erkannten Ueberschriften erzeugen

## 4. Datenfluss

1. Benutzer startet `pdf-ocr input.pdf -o output.md`.
2. CLI prueft Server-Erreichbarkeit.
3. PDF wird analysiert und Seitenbeschreibungen werden aufgebaut.
4. Fuer jede Seite:
   1. adaptive DPI berechnen
   2. Seite rasterisieren
   3. Bild in WebP umwandeln und hashen
   4. Cache pruefen
   5. bei Cache-Miss OCR-Endpunkt aufrufen
   6. Markdown bereinigen und Ergebnis speichern
5. Seiten werden in Originalreihenfolge aggregiert.
6. YAML-Frontmatter und optionales TOC werden hinzugefuegt.
7. Markdown wird an den Zielpfad geschrieben.

## 5. Fehlerbehandlung

Zentrales Fehler-Enum:

```rust
#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("Server nicht erreichbar unter {0}. Laeuft deepseek-ocr.rs im Server-Modus?")]
    ServerUnreachable(String),
    #[error("PDF Fehler: {0}")]
    PdfProcessingError(String),
    #[error("Inferenz fehlgeschlagen: {0}")]
    InferenceError(String),
    #[error("Timeout bei Seite {0}")]
    Timeout(u32),
}
```

Ergaenzend sind Infrastrukturfehler fuer I/O, JSON oder Cache sinnvoll, solange die vier Kernfehler nach aussen erhalten bleiben.

## 6. Graceful Shutdown

- `Ctrl+C` setzt ein Shutdown-Flag.
- Bereits laufende Requests duerfen sauber fertiglaufen.
- Neue OCR-Aufgaben werden danach nicht mehr gestartet.
- Bereits erfolgreiche Seiten werden in einer Datei mit Suffix `~partial.md` persistiert.

## 7. Fortschritt und UX

- `indicatif` zeigt einen globalen Fortschrittsbalken.
- Kurze Statusmeldungen geben den aktuellen Schritt wieder:
  - `Warte auf Server...`
  - `Rasterisiere Seite X`
  - `Generiere Text fuer Seite X`
  - `Schreibe Markdown...`

## 8. Implementierungsstand dieser ersten Lieferung

Die initiale Implementierung deckt bereits den Kernpfad ab:

- CLI und Fehlerenum
- Server-Health-Check
- PDF-Metadaten und Seitendimensionen
- Poppler-basierte Seitenrasterisierung
- WebP-Base64-Transport
- lokaler SHA-256 Cache
- Markdown-Cleaning, Frontmatter und TOC
- Graceful-Shutdown mit Partial-Output

Bewusst als naechste Ausbaustufen vorgesehen:

- Retry-Strategien fuer instabile Serverantworten
- Konfigurierbare Bildzielgroesse (`1024` vs. `1536`)
- Erweiterte Telemetrie und detaillierteres Logging
- Optionaler Batch-Modus fuer ganze Verzeichnisse
