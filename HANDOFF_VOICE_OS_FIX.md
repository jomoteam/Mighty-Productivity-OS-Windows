# Mighty Voice OS fix handoff

Date: 2026-05-29

Current status: fixes applied, build/check verified, debug build installed and launched.

User report addressed:
- Push-to-talk started and ended immediately instead of holding until release.
- Exit/cancel button was unreliable for overlay/tools.
- OCR tool behaved like screenshot/native snipping.

Changes completed:

1. Push-to-talk hold/release hardening
- File changed: `src-tauri/src/windows_ptt_hook.rs`
- Added `Instant` and `active_since: Option<Instant>` to the Windows low-level PTT hook state.
- Added watchdog grace period:
  - `stop_if_trigger_released()` ignores release detection for the first 175ms after activation.
  - hook key-up inside the first 75ms is swallowed/ignored.
- Purpose: prevent PTT from immediately sending false after true because watchdog/key-up fires too quickly.
- Existing behavior retained: once active, Space repeats are swallowed even if Ctrl/Shift are released while Space remains physically held.

2. Mighty OCR separated from Mighty Screenshot
- Files changed: `src-tauri/src/lib.rs`, `src/ScreenshotOverlay.tsx`
- `run_ocr_capture_now` now opens the app overlay in OCR mode instead of calling `ocr::run_quick_ocr_capture` / Windows native snip.
- OCR hotkey now opens the app overlay in OCR mode.
- In OCR mode:
  - user drags a text region,
  - mouse release auto-runs OCR,
  - result text is copied by existing OCR pipeline,
  - overlay closes after result,
  - no annotation/save/copy-image toolbar is shown.
- Mighty Screenshot mode remains the separate screenshot/annotation tool with toolbar.

3. Overlay cancel/exit hardened
- File changed: `src/ScreenshotOverlay.tsx`
- `closeOverlay` now force-resets drag/draft/text/busy state.
- It also calls `setFullscreen(false)` and `hide()` defensively.
- Esc and right-click route through hard close.
- Cancel buttons stop propagation/prevent default so clicking X cannot restart selection drag.
- Added always-visible top-right cancel X.

Verification completed:
- `npm run build` passed.
- `cd src-tauri && cargo check --lib` passed.
- `cd src-tauri && cargo test ocr --lib` passed: 3 passed, 0 failed.
- `cd src-tauri && cargo build` passed and produced a fresh debug EXE.

Install/launch completed:
- Fresh debug EXE installed/copied to:
  `C:/Users/Jomo/AppData/Local/Mighty Voice OS/dictationapp.exe`
- Backup of previous installed EXE:
  `C:/Users/Jomo/AppData/Local/Mighty Voice OS/dictationapp.exe.backup-before-voice-os-fix`
- Installed app launched successfully.
- Running process verified:
  - `dictationapp.exe`
  - path: `C:/Users/Jomo/AppData/Local/Mighty Voice OS/dictationapp.exe`

Packaging note:
- Full `npm run tauri build` / fresh NSIS release installer is still blocked by Windows Application Control policy while executing generated Cargo release build scripts.
- Blocking error seen on generated build scripts such as `zerocopy` and `tauri-plugin-autostart` with OS error 4551.
- Workaround used for now: install the freshly built debug EXE directly into the existing installed app path.

Relevant logs:
- `DEBUG_INSTALL_LATEST_LOG.txt` — latest direct debug install/launch log.
- `AUTO_INSTALL_LOG.txt` — previous/release installer automation log.

Next manual verification for user:
1. Test Push-to-talk: hold Ctrl+Shift+Space; recording should stay active until Space release.
2. Test releasing Ctrl/Shift while still holding Space; no typed spaces should leak and recording should stop on Space release.
3. Test Mighty OCR: Ctrl+Shift+2 or Settings button -> drag text region -> OCR runs immediately -> text copied, no screenshot toolbar.
4. Test Mighty Screenshot: Ctrl+Shift+3 or Settings button -> toolbar/annotation screenshot flow.
5. Test overlay cancel: Esc, right-click, top-right X, and toolbar X should all exit cleanly.

Do NOT store this handoff in persistent memory; it is task progress and may become stale.
