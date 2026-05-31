#!/usr/bin/env bash
set -euo pipefail
ROOT='/c/Users/Jomo/work/AudioToText/Mighty Voice/DictationApp'
LOG="$ROOT/AUTO_INSTALL_LOG.txt"
START_EPOCH=$(python - <<'PY'
import time
print(time.time())
PY
)
cd "$ROOT"
{
  echo "== Mighty Voice current fix rebuild/install =="
  date
  echo "Build start epoch: $START_EPOCH"
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
  export MSVC_DIR="$(find "$MSVC_BASE" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | sort -V | tail -1 || true)"
  export SDK_BASE='/c/Program Files (x86)/Windows Kits/10/Lib'
  export SDK_DIR="$(find "$SDK_BASE" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | sort -V | tail -1 || true)"
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

  echo "== find fresh NSIS installer =="
  INSTALLER_LINE="$(find src-tauri/target/release/bundle/nsis -maxdepth 1 -type f -name '*.exe' -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -1 || true)"
  if [ -z "$INSTALLER_LINE" ]; then
    echo "No NSIS installer found after build" >&2
    exit 2
  fi
  INSTALLER_MTIME="${INSTALLER_LINE%% *}"
  INSTALLER="${INSTALLER_LINE#* }"
  python - "$START_EPOCH" "$INSTALLER_MTIME" <<'PY'
import sys
start=float(sys.argv[1]); mtime=float(sys.argv[2])
if mtime < start:
    raise SystemExit(f"Installer is stale: mtime={mtime}, build_start={start}")
PY
  echo "Fresh installer: $INSTALLER"
  echo "Installer mtime: $INSTALLER_MTIME"
  echo

  echo "== kill old app processes =="
  /c/Windows/System32/taskkill.exe /IM 'dictationapp.exe' /F >/dev/null 2>&1 || true
  /c/Windows/System32/taskkill.exe /IM 'Mighty Voice OS.exe' /F >/dev/null 2>&1 || true
  sleep 1
  echo

  echo "== silent install =="
  "$INSTALLER" /S
  echo "Installed from: $INSTALLER"
  echo

  echo "== locate installed EXE =="
  EXE=""
  for candidate in \
    '/c/Users/Jomo/AppData/Local/Mighty Voice OS/dictationapp.exe' \
    '/c/Users/Jomo/AppData/Local/Programs/Mighty Voice OS/dictationapp.exe' \
    '/c/Users/Jomo/AppData/Local/Programs/dictationapp/dictationapp.exe' \
    '/c/Program Files/Mighty Voice OS/dictationapp.exe' \
    '/c/Program Files (x86)/Mighty Voice OS/dictationapp.exe'; do
    if [ -f "$candidate" ]; then EXE="$candidate"; break; fi
  done
  if [ -z "$EXE" ]; then
    EXE="$(find '/c/Users/Jomo/AppData/Local' '/c/Program Files' '/c/Program Files (x86)' -maxdepth 4 -type f -iname 'dictationapp.exe' 2>/dev/null | head -1 || true)"
  fi
  if [ -z "$EXE" ]; then
    echo "Installed EXE not found" >&2
    exit 3
  fi
  echo "Installed EXE: $EXE"
  echo

  echo "== launch installed app =="
  WIN_EXE="$(cygpath -w "$EXE")"
  /c/Windows/System32/cmd.exe /c start "" "$WIN_EXE"
  sleep 4
  powershell.exe -NoProfile -Command "Get-Process dictationapp -ErrorAction SilentlyContinue | Select-Object Id,ProcessName,Path | Format-List" | tee /tmp/mightyvoice_tasklist.txt
  if ! grep -qi 'dictationapp' /tmp/mightyvoice_tasklist.txt; then
    echo "dictationapp.exe not visible in process list after launch" >&2
    exit 4
  fi
  echo

  echo "== update handoff =="
  python - <<'PY'
from pathlib import Path
p=Path(r'C:/Users/Jomo/work/AudioToText/Mighty Voice/DictationApp/NEXT_OCR_UPGRADES.md')
s=p.read_text(encoding='utf-8') if p.exists() else ''
marker='''\n## Auto status — Current API/OCR fix\n'''
status='''\n## Auto status — Current API/OCR fix\n- Removed API key auto-save-on-keystroke.\n- Added explicit Groq API key Save button with xAI compatibility warning.\n- Set OCR default to local in frontend and backend.\n- Rebuilt frontend, ran Rust OCR tests/checks, built fresh Tauri installer, installed it, and launched the installed app.\n'''
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
  echo "Mighty Voice current fix rebuild/install completed successfully. Log: $LOG"
else
  echo "Mighty Voice current fix rebuild/install failed with exit code $code. Log: $LOG"
fi
exit $code
