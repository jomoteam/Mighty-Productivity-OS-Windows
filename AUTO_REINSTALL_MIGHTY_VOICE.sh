#!/usr/bin/env bash
set -u
ROOT='/c/Users/Jomo/work/AudioToText/Mighty Voice/DictationApp'
LOG="$ROOT/AUTO_INSTALL_LOG.txt"
APP_EXE1='/c/Users/Jomo/AppData/Local/Programs/Mighty Voice OS/Mighty Voice OS.exe'
APP_EXE2='/c/Users/Jomo/AppData/Local/Mighty Voice OS/Mighty Voice OS.exe'
cd "$ROOT" || exit 1
{
  echo '=== Mighty Voice OS auto reinstall ==='
  date
  echo
  echo '--- git status ---'
  git status --short || true
  echo
  echo '--- npm run build ---'
  npm run build
  echo
  echo '--- cargo test ocr --lib ---'
  (cd src-tauri && cargo test ocr --lib)
  echo
  echo '--- cargo check --lib ---'
  (cd src-tauri && cargo check --lib)
  echo
  echo '--- npm run tauri build ---'
  npm run tauri build
  echo
  echo '--- locating NSIS installer ---'
  INSTALLER=$(python - <<'PY'
import glob, os
paths = glob.glob(r'C:/Users/Jomo/work/AudioToText/Mighty Voice/DictationApp/src-tauri/target/release/bundle/nsis/*setup.exe')
if not paths:
    raise SystemExit('No NSIS setup.exe found')
print(max(paths, key=os.path.getmtime))
PY
)
  echo "Installer: $INSTALLER"
  echo
  echo '--- stopping running app ---'
  taskkill //IM 'Mighty Voice OS.exe' //F || true
  taskkill //IM 'Mighty Voice.exe' //F || true
  sleep 2
  echo
  echo '--- silent install ---'
  "$INSTALLER" //S
  echo
  echo '--- launch app ---'
  if [ -f "$APP_EXE1" ]; then
    nohup "$APP_EXE1" >/dev/null 2>&1 &
    echo "Launched: $APP_EXE1"
  elif [ -f "$APP_EXE2" ]; then
    nohup "$APP_EXE2" >/dev/null 2>&1 &
    echo "Launched: $APP_EXE2"
  else
    echo 'Installed EXE not found in expected locations; installer may use a different path.'
  fi
  echo
  echo '=== completed ==='
  date
} > "$LOG" 2>&1
