# pdf-ocr

Rust-CLI fuer PDF-zu-Markdown ueber einen laufenden DeepSeek-OCR-HTTP-Server.

Der Orchestrator in diesem Repo:

- rendert PDF-Seiten mit `pdftoppm`
- codiert Seiten fuer den Transport zum Server
- ruft einen OpenAI-kompatiblen OCR-Server unter `/v1/chat/completions` auf
- schreibt ein Markdown-Dokument mit YAML-Frontmatter und TOC

Die Server-Seite kommt aus dem separaten Repo `deepseek-ocr.rs`, das bei dir aktuell unter `/Users/fre/RustroverProjects/deepseek-ocr.rs` liegt.

## Voraussetzungen

- Rust / Cargo
- Poppler mit `pdftoppm`
- ein laufender `deepseek-ocr-server`

macOS:

```bash
brew install poppler
```

Build und Tests fuer dieses Repo:

```bash
cargo test
```

## Lokaler Start auf diesem Mac

### Empfohlener, stabil validierter Pfad: CPU + `deepseek-ocr`

Der folgende Server-Start wurde auf diesem Rechner erfolgreich gegen das CLI getestet.

```bash
cd /Users/fre/RustroverProjects/deepseek-ocr.rs

cargo run -p deepseek-ocr-server --release -- \
  --device cpu --dtype f32 \
  --host 0.0.0.0 --port 8003 \
  --model deepseek-ocr \
  --max-new-tokens 512
```

Danach das CLI in diesem Repo:

```bash
cd /Users/fre/dev/unfold

cargo run -- /pfad/zur/datei.pdf \
  -o /pfad/zum/output.md \
  --server http://127.0.0.1:8003/v1 \
  --model deepseek-ocr \
  --concurrent 1
```

Beispiel:

```bash
cargo run -- /tmp/pdf-ocr-smoke.pdf \
  -o /tmp/pdf-ocr-smoke-out.md \
  --server http://127.0.0.1:8003/v1 \
  --model deepseek-ocr \
  --concurrent 1 \
  --no-cache
```

### Optionaler Mac-GPU-Pfad: Metal

Wichtig: Fuer Metal muss der Server mit `--features metal` gebaut werden.

```bash
cd /Users/fre/RustroverProjects/deepseek-ocr.rs

cargo run -p deepseek-ocr-server --release --features metal -- \
  --device metal --dtype f16 \
  --host 0.0.0.0 --port 8000 \
  --model deepseek-ocr-q4k \
  --max-new-tokens 512
```

CLI dazu:

```bash
cd /Users/fre/dev/unfold

cargo run -- /pfad/zur/datei.pdf \
  -o /pfad/zum/output.md \
  --server http://127.0.0.1:8000/v1 \
  --model deepseek-ocr-q4k \
  --concurrent 1
```

Hinweis:
Der Metal-Start wurde kompiliert und gebootet, aber auf diesem Rechner war der erste echte OCR-Request mit `deepseek-ocr-q4k` am 15. Maerz 2026 noch instabil. Fuer lokale Validierung ist deshalb der CPU-Pfad oben aktuell die sichere Wahl.

## Server auf einer GPU-VM starten

Fuer eine Linux-VM mit NVIDIA-GPU:

```bash
cd /pfad/zu/deepseek-ocr.rs

cargo run -p deepseek-ocr-server --release --features cuda -- \
  --device cuda --dtype f16 \
  --host 0.0.0.0 --port 8000 \
  --model deepseek-ocr-q4k \
  --max-new-tokens 512
```

Danach dieses CLI gegen die VM richten:

```bash
cd /Users/fre/dev/unfold

cargo run -- /pfad/zur/datei.pdf \
  -o /pfad/zum/output.md \
  --server http://<vm-ip>:8000/v1 \
  --model deepseek-ocr-q4k \
  --concurrent 1
```

Wenn du die VM von aussen erreichen willst, oeffne Port `8000` in Firewall / Security Group und pruefe:

```bash
curl http://<vm-ip>:8000/v1/models
```

## Wichtige CLI-Optionen

```text
pdf-ocr <input.pdf> -o <output.md>
```

Zusaetzliche Optionen:

- `--server`: Server-URL, Default `http://localhost:8000/v1`
- `--model`: Modellname, Default `deepseek-ocr`
- `--concurrent`: maximale parallele OCR-Requests, Default `1`
- `--no-cache`: Cache fuer diesen Lauf ignorieren

## Aktueller Stand

Verifiziert wurden in diesem Repo:

- End-to-end gegen `deepseek-ocr-server` auf CPU
- OpenAI-kompatibler `/v1/chat/completions`-Pfad
- Markdown-Output mit YAML-Frontmatter, TOC, Cache und Partial-Output

Die aktuelle Architektur ist im SDD dokumentiert:

- [specs/SDD.md](/Users/fre/dev/unfold/specs/SDD.md)
