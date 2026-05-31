# Mighty Voice — Next OCR Upgrades (Saved for next session)

Date saved: 2026-05-24

## Priority order

1. Path indicator + latency toast (high impact, low risk)
   - Show OCR path used: `Local OCR` or `Cloud Fallback`
   - Show end-to-end capture time (e.g., `320ms`)

2. Silent auto-retry for weak local OCR
   - If local OCR returns empty/weak text, retry once with:
     - increased contrast
     - slight upscale

3. Better region UX
   - `Esc` to cancel cleanly
   - tiny selection warning for too-small capture area
   - optional drag helpers

4. Smart language detection
   - detect likely script/language and auto-select best OCR language path

5. Clipboard dual-mode
   - copy extracted text + source image payload when supported

6. History quality upgrades
   - pin/favorite entries
   - bulk copy formats: plain text / markdown / CSV

7. Advanced performance tuning panel
   - expose max-edge, JPEG quality, preprocessing toggles

8. Optional stronger offline/local pack
   - improve local-only reliability without cloud dependency

## Next implementation batch (recommended)
- Batch A (fastest UX win): #1 + #2 + #3

## Add for tomorrow — History bulk copy order (Forest feature)
Problem:
- History list is shown newest-first (descending) for browsing.
- During bulk copy, output should read oldest-to-newest (ascending) so pasted text flows naturally.

Decision:
- Keep UI list order as-is (newest on top).
- Change only bulk-copy output order to ascending by timestamp.

Implementation notes:
- On `Copy selected`, gather selected items from history.
- Sort selected items by `createdAt` ascending (oldest -> newest).
- Join with chosen separator/newline and copy to clipboard.
- Optional setting toggle (default ON): `Bulk copy in chronological order`.

Acceptance checks:
- Selecting top 3 newest entries pastes in chronological order (oldest of the 3 first).
- Single-item copy unchanged.
- Select-all copy pastes as full conversation timeline, oldest -> newest.

## New request — WeChat-style screenshot tool
Current state:
- Mighty Voice Quick OCR currently uses the native Windows Snipping overlay (`ms-screenclip:`) for region selection.
- That gives the fast screenshot-to-OCR-to-clipboard flow, but it is NOT a full WeChat-style custom screenshot editor/toolbar.

User target:
- A WeChat-style screenshot mode inside Mighty Voice:
  - hotkey opens full-screen dim overlay
  - drag to select region
  - selection border with resize handles
  - floating toolbar under selection
  - actions: OCR text, copy image, save image, cancel/done
  - optional annotation tools later: arrow, rectangle, text, blur/mosaic, pen

Recommended implementation phases:
1. Phase A — Custom capture overlay MVP
   - Tauri transparent always-on-top fullscreen window per monitor
   - dimmed background
   - drag-to-select rectangle
   - Esc cancels, Enter confirms
   - toolbar: OCR / Copy image / Save / Cancel
2. Phase B — WeChat polish
   - resize handles
   - magnifier/crosshair coordinates
   - selection size label
   - better toolbar positioning near selected region
3. Phase C — Annotation tools
   - pen, rectangle, arrow, text, blur/mosaic
   - copy annotated image to clipboard

Acceptance checks:
- Hotkey opens custom overlay without needing Windows Snipping Tool.
- Drag selection feels like WeChat screenshot.
- OCR button extracts selected region text and copies it.
- Copy image button puts the selected screenshot on clipboard.
- Esc cancels cleanly.
## Auto status — WeChat-style screenshot tool
- Implemented custom fullscreen dim screenshot overlay with drag selection.
- Added floating toolbar actions: OCR, Copy image, Save, Cancel.
- Added Esc cancel and Enter-to-OCR.
- Added backend commands for opening overlay and capturing selected region.
- Added shared OCR image-bytes path so overlay OCR uses existing OCR settings/fallback.
- Verified npm build, Rust OCR tests, Cargo check, and Tauri release package build.
- Installed latest NSIS build and launched app.
## Auto status — Current API/OCR fix
- Removed API key auto-save-on-keystroke.
- Added explicit Groq API key Save button with xAI compatibility warning.
- Set OCR default to local in frontend and backend.
- Rebuilt frontend, ran Rust OCR tests/checks, built fresh Tauri installer, installed it, and launched the installed app.
