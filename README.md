# Oleh Groovy Studio (Web)

Modern browser-based editor for diaLIMS Groovy templates and `.docx` template files.

## What It Does

- VS-Code-style workspace UI with Monaco editor and smooth animated design.
- Opens `.docx`, `.groovy`, `.txt` directly from the workspace tree.
- Groovy/Text mode switch for quick template logic editing vs plain text editing.
- Session storage in SQLite (`data/oleh_groovy_studio.sqlite`).
- Timeline of edits per session (open/save/manual snapshots/reverts).
- Safe path handling constrained to workspace root.

## DOCX Editing Behavior

- Opening `.docx` reads text from `word/document.xml`.
- Saving `.docx` writes updated text back into `word/document.xml` while preserving other ZIP parts.
- This is optimized for template-content editing workflows.

## Run Locally

```powershell
cargo run --release
```

On startup, the app opens your browser at `http://127.0.0.1:8787`.

## Portable Windows Launch

Use:

- `oleh-groovy-editor.exe`
- `OlehGroovyEditor.bat`

in the same folder, then run `OlehGroovyEditor.bat`.

## GitHub Release Packaging

Workflow:

- `.github/workflows/oleh-groovy-editor-portable.yml`

Build output zip:

- `OlehGroovyEditor-windows-portable.zip`

The workflow verifies that the zip contains both:

- `oleh-groovy-editor.exe`
- `OlehGroovyEditor.bat`

before publishing the release.
