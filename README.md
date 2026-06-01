# Mighty Productivity OS for Windows

Version: v0.4.0

A Windows desktop productivity assistant — dictation, OCR, and screenshot capture in one app. Built with Tauri, React, and Rust.

## Features

- **Push-to-talk dictation** — press Ctrl+Shift+Space, speak, and your words appear wherever you're typing
- **Mighty OCR** — select any region on screen (Ctrl+Shift+2), text is captured and copied to clipboard instantly
- **Mighty Screenshot** — capture, annotate, and save screenshots (Ctrl+Shift+3)
- **Cloud + local OCR** — fast local Windows OCR with Groq cloud fallback for higher accuracy
- **Configurable hotkeys** — change shortcuts to whatever you prefer
- **Clipboard history** — everything you dictate or OCR is saved

## Installation (for developers)

### Prerequisites

1. **Install Rust** — https://www.rust-lang.org/tools/install
2. **Install Node.js** (v18+) — https://nodejs.org
3. Clone the repo and install dependencies:

```bash
git clone https://github.com/jomoteam/Mighty-Productivity-OS-Windows.git
cd Mighty-Productivity-OS-Windows
npm install
```

### Build

```bash
npm run tauri build
```

The installer will be at `src-tauri/target/release/bundle/`.

## Getting a Groq API Key

Voice transcription and cloud OCR need a Groq API key. It's free and the free tier is enough for daily use:

1. Go to https://console.groq.com
2. Sign up for a free account (Google/GitHub login works)
3. Go to **API Keys** → **Create API Key**
4. Copy the key (starts with `gsk_`)
5. Open Mighty Productivity OS → **Settings** → paste your key under **Groq API Key** and click **Save**

No credit card needed. The free tier gives you plenty of requests per day for personal dictation and OCR.

## Contributing

Pull requests are welcome. If you run into any issues or have ideas for improvements, feel free to open an issue on this repo.

## Support

If you need help or want to report a problem, reach out at **[mightytechie.com](https://mightytechie.com)** and I'll help sort it out.
