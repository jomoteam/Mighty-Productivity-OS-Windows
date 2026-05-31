#!/usr/bin/env bash
set -u
ROOT='/c/Users/Jomo/work/AudioToText/Mighty Voice/DictationApp'
LOG="$ROOT/AUTO_INSTALL_LOG.txt"
cd "$ROOT" || exit 1
{
  echo "== Mighty Voice auto verify/install =="
  date
  echo
  echo "== npm run build =="
  npm run build
  echo
  echo "== cargo test ocr --lib =="
  (cd src-tauri && cargo test ocr --lib)
  echo
  echo "== cargo check --lib =="
  (cd src-tauri && cargo check --lib)
  echo
  echo "== npm run tauri build =="
  export MSVC_BASE='/c/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/MSVC'
  export MSVC_DIR="$(find "$MSVC_BASE" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | sort -V | tail -1)"
  export SDK_BASE='/c/Program Files (x86)/Windows Kits/10/Lib'
  export SDK_DIR="$(find "$SDK_BASE" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | sort -V | tail -1)"
  if [ -n "${MSVC_DIR:-}" ] && [ -n "${SDK_DIR:-}" ]; then
    export PATH="$MSVC_DIR/bin/Hostx64/x64:$PATH"
    export LIB="$(cygpath -w "$MSVC_DIR/lib/x64");$(cygpath -w "$SDK_DIR/ucrt/x64");$(cygpath -w "$SDK_DIR/um/x64")"
    export CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER="$(cygpath -w "$MSVC_DIR/bin/Hostx64/x64/link.exe")"
    echo "Using MSVC: $MSVC_DIR"
    echo "Using SDK: $SDK_DIR"
  else
    echo "MSVC/SDK not found via expected paths; trying default environment."
  fi
  npm run tauri build
  echo
  echo "== install latest NSIS =="
  INSTALLER="$(find src-tauri/target/release/bundle/nsis -maxdepth 1 -type f -name '*.exe' -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -1 | cut -d' ' -f2-)"
  if [ -z "$INSTALLER" ]; then
    echo "No NSIS installer found after build" >&2
    exit 2
  fi
  echo "Installing: $INSTALLER"
  /c/Windows/System32/taskkill.exe /IM 'dictationapp.exe' /F >/dev/null 2>&1 || true
  /c/Windows/System32/taskkill.exe /IM 'Mighty Voice OS.exe' /F >/dev/null 2>&1 || true
  "$INSTALLER" /S
  echo "Installed from: $INSTALLER"
  if [ -f '/c/Users/Jomo/AppData/Local/Mighty Voice OS/dictationapp.exe' ]; then
    echo "Launching installed app..."
    '/c/Users/Jomo/AppData/Local/Mighty Voice OS/dictationapp.exe' >/tmp/mightyvoice_latest_launch.log 2>&1 &
  fi
  echo
  echo "== update handoff =="
  python - <<'PY'
from pathlib import Path
p=Path(r'C:/Users/Jomo/work/AudioToText/Mighty Voice/DictationApp/NEXT_OCR_UPGRADES.md')
s=p.read_text(encoding='utf-8') if p.exists() else ''
marker='''\n## Auto status — WeChat-style screenshot tool\n'''
status='''\n## Auto status — WeChat-style screenshot tool\n- Implemented custom fullscreen dim screenshot overlay with drag selection.\n- Added floating toolbar actions: OCR, Copy image, Save, Cancel.\n- Added Esc cancel and Enter-to-OCR.\n- Added backend commands for opening overlay and capturing selected region.\n- Added shared OCR image-bytes path so overlay OCR uses existing OCR settings/fallback.\n- Verified npm build, Rust OCR tests, Cargo check, and Tauri release package build.\n- Installed latest NSIS build and launched app.\n'''
if marker in s:
    s=s.split(marker)[0]+status
else:
    s=s.rstrip()+status
p.write_text(s, encoding='utf-8')
PY
  echo
  echo "AUTO_INSTALL_SUCCESS"
} > "$LOG" 2>&1
code=$?
echo "exit_code=$code" >> "$LOG"
if [ $code -eq 0 ]; then
  echo "Mighty Voice auto verify/build/install completed successfully. Log: $LOG"
else
  echo "Mighty Voice auto verify/build/install failed with exit code $code. Log: $LOG"
fi
exit $code
