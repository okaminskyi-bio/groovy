# Oleh Groovy Editor

Portable desktop editor for diaLIMS Groovy-based report templates, including Word `.docx` template text extraction.

## Key Features

- Monaco-style coding UX in a native desktop app (tabs, syntax highlighting, diagnostics panel).
- Groovy + diaLIMS marker highlighting:
  - `{|: ... {:|}`
  - `{||: ... {:||}`
  - `{! ... !}`
- Built-in template linting:
  - marker mismatch detection
  - old class-path detection (`Biolab...Controller`)
  - suspicious patterns (e.g. `returnpe.` or inline `return null; !}`)
- Placeholder index for quick navigation of template variables.
- Snippet tray for quick testing fragments:
  - add, copy, insert, delete snippets
  - folders + tags + filters
  - capture clipboard into tray
- Drag-and-drop file opening.
- DOCX import mode:
  - reads `word/document.xml` text from `.docx`
  - edits as text and saves to `.groovy/.txt` files
- Split-view diff:
  - compare current tab vs another tab or external file
  - side-by-side highlighting for added/removed/replaced rows
- Quick template test harness:
  - JSON input variables
  - real-time render preview before saving
  - unresolved placeholder tracking
- Built-in Git flow panel:
  - status / fetch / pull-rebase / push
  - commit+push
  - merge branch
- Local-network collaboration:
  - SQLite message board in shared folder
  - quote code in messages
  - suggestion decision flow (approve / deny / reset)
  - realtime sync via local WebSocket host/client mode

## Shortcuts

- `Ctrl+N`: New tab
- `Ctrl+O`: Open file
- `Ctrl+S`: Save
- `Ctrl+Shift+S`: Save As
- `F7`: Run lint

## Local Run

```powershell
cargo run
```

## Local Release Build

```powershell
cargo build --release
```

## Portable Launcher

Use `portable/OlehGroovyEditor.bat` together with `oleh-groovy-editor.exe` in the same folder.

## Collaboration Setup

1. Pick a shared SQLite file path in the Collab panel (for example on a network share).
2. On one machine: start `Host` on an address (for example `0.0.0.0:9002`).
3. On other machines: connect as `Client` using `ws://host-ip:9002`.
4. Post messages, quote code, and approve/deny suggestions in realtime.

The app uses machine identity by default (`COMPUTERNAME\USERNAME (ip)`), so no login is required.

## GitHub Release (Portable Zip)

The workflow `.github/workflows/oleh-groovy-editor-portable.yml` will:

- build on `windows-latest`
- package `OlehGroovyEditor-windows-portable.zip`
- upload workflow artifact
- create a GitHub release asset when you push a tag like `v1.0.0`

### Download latest release without login (public repo)

```powershell
.\scripts\download-latest-portable.ps1 -Owner <owner> -Repo <repo>
```
