# Mighty-Productivity-OS-Windows

Version: v0.4.0

A Windows desktop productivity assistant built with Tauri, React, and Rust.

## Features

- Push-to-talk dictation
- Mighty OCR: quick region-to-text capture and clipboard copy
- Mighty Screenshot: screenshot capture workflow
- Local-first Windows OCR support with cloud fallback paths
- Configurable desktop hotkeys
- History and clipboard-focused productivity workflows

## Development

```bash
npm install
npm run build
npm run tauri build
```

## Windows

This app targets Windows and packages through Tauri.
## API keys

Do not commit real API keys to this repository.

For local development, copy `.env.example` to `.env` and set:

```bash
GROQ_API_KEY=your_local_key_here
```

For the installed Windows app, the Groq key is stored locally by the app settings flow, not in git.

