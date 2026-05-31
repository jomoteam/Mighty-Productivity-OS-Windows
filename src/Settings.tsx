import { useState, useEffect } from 'react';
import { Key, Languages, Keyboard, Mic, ChevronRight, Copy, Check, Trash2 } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window';
import { listen } from '@tauri-apps/api/event';
import { getVersion } from '@tauri-apps/api/app';

type RecordingMode = 'push_to_talk' | 'hands_free' | 'command';

type OcrCopyMode = 'original' | 'translated' | 'both';
type OcrImageFormat = 'png' | 'jpeg';

interface OcrStatus {
  path: string;
  label: string;
  elapsedMs: number;
}

interface OcrSettings {
  ocrHotkey: string;
  ocrLanguage: string;
  translateEnabled: boolean;
  translateTarget: string;
  copyMode: OcrCopyMode;
  imageFormat: OcrImageFormat;
  jpegQuality: number;
  speedMode: 'local' | 'fast' | 'balanced' | 'accuracy';
}

const MODES: { id: RecordingMode; label: string; sub: string; icon: string }[] = [
  { id: 'push_to_talk', label: 'Push to Talk', sub: 'Hold to record, release to transcribe', icon: '🎙' },
  { id: 'hands_free',   label: 'Hands Free',  sub: 'Press once to start, press again to stop', icon: '🤲' },
  { id: 'command',      label: 'Command',     sub: 'Speech → structured AI prompt', icon: '✦' },
];


interface HistoryItem {
  id: string;
  text: string;
  ts: string;
}

interface ApiKeyStatus {
  hasKey: boolean;
  source: 'environment' | 'settings_file' | 'current_session' | 'none' | string;
  path: string;
}

export default function Settings() {
  const [apiKey, setApiKey]               = useState('');
  const [apiKeySaved, setApiKeySaved]     = useState(false);
  const [apiKeyStatus, setApiKeyStatus] = useState<ApiKeyStatus | null>(null);
  const [recordingMode, setRecordingMode] = useState<RecordingMode>('push_to_talk');
  const [language, setLanguage]           = useState('auto');
  const [hotkey, setHotkey]               = useState('CmdOrCtrl+Shift+Space');
  const [capturingHotkey, setCapturingHotkey] = useState(false);
  const [appError, setAppError]           = useState<string | null>(null);
  const [logs, setLogs]                   = useState<string[]>([]);
  const [version, setVersion]             = useState('');
  const [inputDevices, setInputDevices]   = useState<string[]>([]);
  const [inputDevice, setInputDevice]     = useState('');
  const [history, setHistory]             = useState<HistoryItem[]>([]);
  const [historyOpen, setHistoryOpen]     = useState(false);
  const [copiedId, setCopiedId]           = useState<string | null>(null);
  const [expandedId, setExpandedId]       = useState<string | null>(null);
  const [selectedHistoryIds, setSelectedHistoryIds] = useState<string[]>([]);
  const [autostart, setAutostart]         = useState(false);
  const [ocrCapturingHotkey, setOcrCapturingHotkey] = useState(false);
  const [screenshotHotkey, setScreenshotHotkey] = useState('CmdOrCtrl+Shift+3');
  const [screenshotCapturingHotkey, setScreenshotCapturingHotkey] = useState(false);
  const [ocrStatus, setOcrStatus] = useState<OcrStatus | null>(null);
  const [ocrSettings, setOcrSettings] = useState<OcrSettings>({
    ocrHotkey: 'CmdOrCtrl+Shift+2',
    ocrLanguage: 'auto',
    translateEnabled: false,
    translateTarget: 'en',
    copyMode: 'original',
    imageFormat: 'png',
    jpegQuality: 90,
    speedMode: 'local',
  });

  // Load persisted data
  useEffect(() => {
    getVersion().then(setVersion).catch(() => {});
    invoke<string[]>('list_input_devices').then(setInputDevices).catch(console.error);
    invoke<ApiKeyStatus>('get_api_key_status').then(setApiKeyStatus).catch(console.error);

    const storedApiKey    = localStorage.getItem('mightyvoice.apiKey') ?? '';
    const storedMode      = localStorage.getItem('mightyvoice.recordingMode') as RecordingMode | null;
    const storedLanguage  = localStorage.getItem('mightyvoice.language') ?? 'auto';
    const savedHotkey     = localStorage.getItem('mightyvoice.hotkey') ?? 'CmdOrCtrl+Shift+Space';
    const storedHotkey    = ['CmdOrCtrl+Alt+V', 'Ctrl+Alt+V'].includes(savedHotkey) ? 'CmdOrCtrl+Shift+Space' : savedHotkey;
    const storedDevice    = localStorage.getItem('mightyvoice.inputDevice') ?? '';
    const storedHistory   = localStorage.getItem('mightyvoice.history');
    const storedOcrSettings = localStorage.getItem('mightyvoice.ocrSettings');
    const storedScreenshotHotkey = localStorage.getItem('mightyvoice.screenshotHotkey') ?? 'CmdOrCtrl+Shift+3';

    const trimmedStoredApiKey = storedApiKey.trim();
    if (trimmedStoredApiKey) {
      if (trimmedStoredApiKey.startsWith('gsk_')) {
        setApiKey(trimmedStoredApiKey);
        invoke('set_api_key', { apiKey: trimmedStoredApiKey }).catch(console.error);
        setApiKeySaved(true);
      } else {
        localStorage.removeItem('mightyvoice.apiKey');
        setApiKey('');
        setApiKeySaved(false);
        setAppError('Removed an incompatible saved API key. Mighty Productivity OS needs a Groq key starting with gsk_; xAI keys are not sent to Groq.');
      }
    }
    if (storedMode && ['push_to_talk', 'hands_free', 'command'].includes(storedMode)) setRecordingMode(storedMode);
    setLanguage(storedLanguage);
    setHotkey(storedHotkey);
    setInputDevice(storedDevice);
    setScreenshotHotkey(storedScreenshotHotkey);
    if (storedHistory) setHistory(JSON.parse(storedHistory));
    if (storedOcrSettings) {
      try {
        const parsed = JSON.parse(storedOcrSettings) as OcrSettings;
        const savedKey = storedApiKey.trim();
        setOcrSettings({ ...parsed, speedMode: savedKey.startsWith('gsk_') ? parsed.speedMode : 'local' });
      } catch {}
    }

    invoke('set_input_device', { deviceName: storedDevice }).catch(console.error);
    invoke<boolean>('plugin:autostart|is_enabled').then(setAutostart).catch(console.error);
  }, []);

  // Sync state to backend
  useEffect(() => {
    invoke('set_app_state', { recordingMode, language }).catch(console.error);
    localStorage.setItem('mightyvoice.recordingMode', recordingMode);
    localStorage.setItem('mightyvoice.language', language);
  }, [recordingMode, language]);

  useEffect(() => {
    invoke('update_hotkey', { newHotkey: hotkey }).catch(console.error);
    localStorage.setItem('mightyvoice.hotkey', hotkey);
  }, [hotkey]);

  useEffect(() => {
    invoke('update_ocr_settings', {
      settings: {
        ocrHotkey: ocrSettings.ocrHotkey,
        ocrLanguage: ocrSettings.ocrLanguage,
        translateEnabled: ocrSettings.translateEnabled,
        translateTarget: ocrSettings.translateTarget,
        copyMode: ocrSettings.copyMode,
        imageFormat: ocrSettings.imageFormat,
        jpegQuality: ocrSettings.jpegQuality,
        speedMode: ocrSettings.speedMode,
      }
    }).catch(console.error);

    invoke('update_ocr_hotkey', { newHotkey: ocrSettings.ocrHotkey }).catch(console.error);
    localStorage.setItem('mightyvoice.ocrSettings', JSON.stringify(ocrSettings));
  }, [ocrSettings]);

  useEffect(() => {
    invoke('update_screenshot_hotkey', { newHotkey: screenshotHotkey }).catch(console.error);
    localStorage.setItem('mightyvoice.screenshotHotkey', screenshotHotkey);
  }, [screenshotHotkey]);

  // Events
  useEffect(() => {
    let timeout: ReturnType<typeof setTimeout> | null = null;

    const unlistenError = listen<string>('app-error', (event) => {
      setAppError(event.payload);
      if (timeout) clearTimeout(timeout);
      timeout = setTimeout(() => setAppError(null), 5000);
    });

    const unlistenLog = listen<string>('app-log', (event) => {
      const ts = new Date().toLocaleTimeString();
      setLogs(prev => [`[${ts}] ${event.payload}`, ...prev].slice(0, 20));
    });

    const unlistenResult = listen<string>('dictation-result', (event) => {
      const item: HistoryItem = {
        id: Date.now().toString(),
        text: event.payload,
        ts: new Date().toLocaleTimeString(),
      };
      setHistory(prev => {
        const next = [item, ...prev].slice(0, 50);
        localStorage.setItem('mightyvoice.history', JSON.stringify(next));
        return next;
      });
    });

    const unlistenOcrStatus = listen<OcrStatus>('ocr-status', (event) => {
      setOcrStatus(event.payload);
    });

    return () => {
      unlistenError.then(u => u());
      unlistenLog.then(u => u());
      unlistenResult.then(u => u());
      unlistenOcrStatus.then(u => u());
      if (timeout) clearTimeout(timeout);
    };
  }, []);


  // Hotkey capture
  useEffect(() => {
    if (!capturingHotkey) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      if (e.key === 'Escape') { setCapturingHotkey(false); invoke('update_hotkey', { newHotkey: hotkey }).catch(console.error); return; }
      if (['Meta', 'Control', 'Alt', 'Shift'].includes(e.key)) return;
      const mods: string[] = [];
      if (e.metaKey || e.ctrlKey) mods.push('CmdOrCtrl');
      if (e.altKey) mods.push('Alt');
      if (e.shiftKey) mods.push('Shift');

      // Require at least one modifier to avoid plain-key hotkeys like "V"
      // which can cause accidental repeats and conflicts while typing.
      if (mods.length === 0) {
        setAppError('Hotkey must include at least one modifier (Ctrl/Cmd/Alt/Shift).');
        return;
      }
      let key = e.code.replace('Key', '').replace('Digit', '');
      if (e.code === 'Space') key = 'Space';
      if (e.code === 'Slash') key = '/';
      if (e.code === 'Period') key = '.';
      if (e.code === 'Comma') key = ',';
      setHotkey([...mods, key].join('+'));
      setCapturingHotkey(false);
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [capturingHotkey, hotkey]);

  useEffect(() => {
    if (!ocrCapturingHotkey) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      if (e.key === 'Escape') { setOcrCapturingHotkey(false); return; }
      if (['Meta', 'Control', 'Alt', 'Shift'].includes(e.key)) return;
      const mods: string[] = [];
      if (e.metaKey || e.ctrlKey) mods.push('CmdOrCtrl');
      if (e.altKey) mods.push('Alt');
      if (e.shiftKey) mods.push('Shift');
      if (mods.length === 0) {
        setAppError('OCR hotkey must include at least one modifier.');
        return;
      }
      let key = e.code.replace('Key', '').replace('Digit', '');
      if (e.code === 'Space') key = 'Space';
      if (e.code === 'Slash') key = '/';
      if (e.code === 'Period') key = '.';
      if (e.code === 'Comma') key = ',';
      setOcrSettings(prev => ({ ...prev, ocrHotkey: [...mods, key].join('+') }));
      setOcrCapturingHotkey(false);
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [ocrCapturingHotkey]);

  useEffect(() => {
    if (!screenshotCapturingHotkey) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      if (e.key === 'Escape') { setScreenshotCapturingHotkey(false); return; }
      if (['Meta', 'Control', 'Alt', 'Shift'].includes(e.key)) return;
      const mods: string[] = [];
      if (e.metaKey || e.ctrlKey) mods.push('CmdOrCtrl');
      if (e.altKey) mods.push('Alt');
      if (e.shiftKey) mods.push('Shift');
      if (mods.length === 0) {
        setAppError('Mighty Screenshot hotkey must include at least one modifier.');
        return;
      }
      let key = e.code.replace('Key', '').replace('Digit', '');
      if (e.code === 'Space') key = 'Space';
      if (e.code === 'Slash') key = '/';
      if (e.code === 'Period') key = '.';
      if (e.code === 'Comma') key = ',';
      setScreenshotHotkey([...mods, key].join('+'));
      setScreenshotCapturingHotkey(false);
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [screenshotCapturingHotkey]);

  const isMac = navigator.platform.toLowerCase().includes('mac');
  const formatHotkey = (value: string) => {
    const replacements: Record<string, string> = isMac
      ? { CmdOrCtrl: '⌘', Ctrl: '⌃', Alt: '⌥', Shift: '⇧', Space: 'Space' }
      : { CmdOrCtrl: 'Ctrl', Ctrl: 'Ctrl', Alt: 'Alt', Shift: 'Shift', Space: 'Space' };
    return value.split('+').map(part => replacements[part] ?? part).join(isMac ? ' ' : '+');
  };
  const displayHotkey = formatHotkey(hotkey);
  const displayOcrHotkey = formatHotkey(ocrSettings.ocrHotkey);
  const displayScreenshotHotkey = formatHotkey(screenshotHotkey);

  const copyItem = (item: HistoryItem) => {
    navigator.clipboard.writeText(item.text).then(() => {
      setCopiedId(item.id);
      setTimeout(() => setCopiedId(null), 1500);
    });
  };

  const deleteItem = (id: string) => {
    setHistory(prev => {
      const next = prev.filter(i => i.id !== id);
      localStorage.setItem('mightyvoice.history', JSON.stringify(next));
      return next;
    });
    setSelectedHistoryIds(prev => prev.filter(x => x !== id));
  };

  const toggleHistorySelection = (id: string) => {
    setSelectedHistoryIds(prev => prev.includes(id) ? prev.filter(x => x !== id) : [...prev, id]);
  };

  const selectAllHistory = () => {
    setSelectedHistoryIds(history.map(h => h.id));
  };

  const clearHistorySelection = () => {
    setSelectedHistoryIds([]);
  };

  const copySelectedHistory = async () => {
    const selectedItems = history
      .filter(h => selectedHistoryIds.includes(h.id))
      .sort((a, b) => Number(a.id) - Number(b.id));
    if (selectedItems.length === 0) return;
    const bulkText = selectedItems.map(i => i.text).join('\n\n');
    await navigator.clipboard.writeText(bulkText);
    setAppError(`Copied ${selectedItems.length} selected items oldest → newest.`);
  };

  const applyApiKey = async () => {
    const trimmed = apiKey.trim();
    if (trimmed && !trimmed.startsWith('gsk_')) {
      setApiKeySaved(false);
      setAppError('This app currently expects a Groq key starting with gsk_. xAI keys are not supported yet. Local OCR still works without a key.');
      return;
    }

    try {
      await invoke('set_api_key', { apiKey: trimmed });
      localStorage.setItem('mightyvoice.apiKey', trimmed);
      setApiKey(trimmed);
      setApiKeySaved(true);
      invoke<ApiKeyStatus>('get_api_key_status').then(setApiKeyStatus).catch(console.error);
      setAppError(trimmed ? 'Groq API key saved.' : 'Groq API key cleared. Local OCR still works without a key.');
    } catch (e) {
      setApiKeySaved(false);
      setAppError(`Failed to save API key: ${String(e)}`);
    }
  };

  return (
    <div className="flex h-screen bg-[#0f0f10] overflow-hidden font-sans">

      {/* ── Settings panel ── */}
      <div className="flex-1 min-w-0 overflow-y-auto p-6 space-y-4">

        {/* Header */}
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <div className="w-9 h-9 rounded-xl bg-white/10 flex items-center justify-center text-lg">🎙</div>
            <div>
              <h1 className="text-white font-semibold text-lg leading-tight">Mighty Productivity OS for Windows</h1>
              <p className="text-white/40 text-xs">Settings {version && <span className="text-white/20">v{version}</span>}</p>
            </div>
          </div>
          {/* History toggle */}
          <button
            onClick={async () => {
              const next = !historyOpen;
              setHistoryOpen(next);
              try {
                const win = getCurrentWindow();
                const s = await win.innerSize();
                const f = await win.scaleFactor();
                await win.setSize(new LogicalSize(next ? 840 : 560, Math.round(s.height / f)));
              } catch {}
            }}
            title="Dictation history"
            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-xl border text-xs font-medium transition-all ${
              historyOpen
                ? 'bg-white text-black border-white'
                : 'bg-white/5 text-white/50 border-white/8 hover:bg-white/10 hover:text-white/80'
            }`}
          >
            History
            <ChevronRight size={12} className={`transition-transform ${historyOpen ? 'rotate-180' : ''}`} />
          </button>
        </div>

        {appError && (
          <div className="rounded-xl border border-red-400/30 bg-red-500/10 px-3 py-2 text-xs text-red-100">{appError}</div>
        )}

        {/* Recording Mode */}
        <section>
          <label className="text-white/40 text-xs font-medium uppercase tracking-wider mb-2 block">Mode</label>
          <div className="grid grid-cols-3 gap-2">
            {MODES.map(m => {
              const active = recordingMode === m.id;
              const modeHotkey = m.id === 'hands_free' ? `Press ${displayHotkey}` : `Hold ${displayHotkey}`;
              return (
                <button key={m.id} onClick={() => setRecordingMode(m.id)}
                  className={`flex flex-col items-center gap-1.5 p-3 rounded-2xl border text-center transition-all ${
                    active ? 'bg-white text-black border-white' : 'bg-white/5 text-white/60 border-white/8 hover:bg-white/10 hover:text-white/80'
                  }`}
                >
                  <span className="text-xl">{m.icon}</span>
                  <span className="text-xs font-semibold leading-tight">{m.label}</span>
                  <span className={`text-[10px] font-mono mt-0.5 ${active ? 'text-black/50' : 'text-white/30'}`}>{modeHotkey}</span>
                </button>
              );
            })}
          </div>
          <p className="text-white/30 text-xs mt-2 text-center">{MODES.find(m => m.id === recordingMode)?.sub}</p>
        </section>

        {/* Hotkey */}
        <section>
          <label className="text-white/40 text-xs font-medium uppercase tracking-wider mb-2 flex items-center gap-1.5">
            <Keyboard size={12} /> Hotkey
          </label>
          <button
            onClick={() => { invoke('unregister_hotkey').catch(console.error); setCapturingHotkey(true); }}
            className={`w-full rounded-2xl border px-4 py-3 text-left flex justify-between items-center transition-all ${
              capturingHotkey ? 'bg-white/10 border-white/30 text-white animate-pulse' : 'bg-white/5 border-white/8 text-white/80 hover:bg-white/8'
            }`}
          >
            <span className="font-mono text-sm">{capturingHotkey ? 'Press keys...' : displayHotkey}</span>
            <span className="text-white/30 text-xs">{capturingHotkey ? 'ESC to cancel' : 'click to change'}</span>
          </button>
        </section>

        {/* Mighty OCR */}
        <section className="rounded-2xl border border-white/8 bg-white/5 p-3 space-y-3">
          <label className="text-white/40 text-xs font-medium uppercase tracking-wider flex items-center gap-1.5">
            ⚡ Mighty OCR
          </label>
          <p className="text-white/35 text-[11px] leading-relaxed">
            One-step text capture: drag a region and the recognized text is copied immediately. No annotation, save, or copy-image toolbar.
          </p>

          {ocrStatus && (
            <div className="rounded-xl border border-emerald-400/20 bg-emerald-500/10 px-3 py-2 text-xs text-emerald-100 flex items-center justify-between">
              <span>{ocrStatus.label}</span>
              <span className="font-mono text-emerald-100/70">{ocrStatus.elapsedMs > 0 ? `${ocrStatus.elapsedMs}ms` : 'ready'}</span>
            </div>
          )}

          <button
            onClick={() => { invoke('unregister_ocr_hotkey').catch(console.error); setOcrCapturingHotkey(true); }}
            className={`w-full rounded-2xl border px-4 py-3 text-left flex justify-between items-center transition-all ${
              ocrCapturingHotkey ? 'bg-white/10 border-white/30 text-white animate-pulse' : 'bg-white/5 border-white/8 text-white/80 hover:bg-white/8'
            }`}
          >
            <span className="font-mono text-sm">{ocrCapturingHotkey ? 'Press keys...' : displayOcrHotkey}</span>
            <span className="text-white/30 text-xs">Mighty OCR hotkey</span>
          </button>

          <div className="grid grid-cols-2 gap-2">
            <select value={ocrSettings.ocrLanguage} onChange={e => setOcrSettings(prev => ({ ...prev, ocrLanguage: e.target.value }))}
              className="bg-white/5 border border-white/8 rounded-xl px-3 py-2 text-white/80 text-xs focus:outline-none">
              <option value="auto" className="bg-[#1a1a1a]">OCR: Auto</option>
              <option value="en" className="bg-[#1a1a1a]">OCR: English</option>
              <option value="zh" className="bg-[#1a1a1a]">OCR: Chinese</option>
              <option value="yue" className="bg-[#1a1a1a]">OCR: Cantonese</option>
              <option value="ja" className="bg-[#1a1a1a]">OCR: Japanese</option>
            </select>

            <select value={ocrSettings.imageFormat} onChange={e => setOcrSettings(prev => ({ ...prev, imageFormat: e.target.value as OcrImageFormat }))}
              className="bg-white/5 border border-white/8 rounded-xl px-3 py-2 text-white/80 text-xs focus:outline-none">
              <option value="png" className="bg-[#1a1a1a]">OCR upload: PNG</option>
              <option value="jpeg" className="bg-[#1a1a1a]">OCR upload: JPEG</option>
            </select>
          </div>

          <select value={ocrSettings.speedMode} onChange={e => setOcrSettings(prev => ({ ...prev, speedMode: e.target.value as OcrSettings['speedMode'] }))}
            className="w-full bg-white/5 border border-white/8 rounded-xl px-3 py-2 text-white/80 text-xs focus:outline-none">
            <option value="local" className="bg-[#1a1a1a]">Speed: Ultra Fast Local</option>
            <option value="fast" className="bg-[#1a1a1a]">Speed: Fast</option>
            <option value="balanced" className="bg-[#1a1a1a]">Speed: Balanced</option>
            <option value="accuracy" className="bg-[#1a1a1a]">Speed: Max Accuracy</option>
          </select>

          {ocrSettings.imageFormat === 'jpeg' && (
            <div>
              <label className="text-white/40 text-[10px]">JPEG quality: {ocrSettings.jpegQuality}</label>
              <input
                type="range"
                min={30}
                max={100}
                value={ocrSettings.jpegQuality}
                onChange={e => setOcrSettings(prev => ({ ...prev, jpegQuality: Number(e.target.value) }))}
                className="w-full"
              />
            </div>
          )}

          <div className="flex items-center justify-between">
            <label className="text-white/60 text-xs">Translate OCR result</label>
            <input type="checkbox" checked={ocrSettings.translateEnabled}
              onChange={e => setOcrSettings(prev => ({ ...prev, translateEnabled: e.target.checked }))}
              className="accent-white" />
          </div>

          {ocrSettings.translateEnabled && (
            <div className="grid grid-cols-2 gap-2">
              <select value={ocrSettings.translateTarget} onChange={e => setOcrSettings(prev => ({ ...prev, translateTarget: e.target.value }))}
                className="bg-white/5 border border-white/8 rounded-xl px-3 py-2 text-white/80 text-xs focus:outline-none">
                <option value="en" className="bg-[#1a1a1a]">To English</option>
                <option value="zh" className="bg-[#1a1a1a]">To Chinese</option>
                <option value="yue" className="bg-[#1a1a1a]">To Cantonese</option>
                <option value="ja" className="bg-[#1a1a1a]">To Japanese</option>
              </select>
              <select value={ocrSettings.copyMode} onChange={e => setOcrSettings(prev => ({ ...prev, copyMode: e.target.value as OcrCopyMode }))}
                className="bg-white/5 border border-white/8 rounded-xl px-3 py-2 text-white/80 text-xs focus:outline-none">
                <option value="original" className="bg-[#1a1a1a]">Copy original</option>
                <option value="translated" className="bg-[#1a1a1a]">Copy translated</option>
                <option value="both" className="bg-[#1a1a1a]">Copy both</option>
              </select>
            </div>
          )}

          <button
            onClick={() => invoke('run_ocr_capture_now').catch((e) => setAppError(String(e)))}
            className="w-full rounded-xl border border-white/20 text-white/80 hover:text-white hover:border-white/40 px-3 py-2 text-xs"
          >
            Test Mighty OCR now
          </button>

          <div className="rounded-xl border border-emerald-400/20 bg-emerald-500/5 p-2 space-y-2">
            <button
              onClick={() => { invoke('unregister_screenshot_hotkey').catch(console.error); setScreenshotCapturingHotkey(true); }}
              className={`w-full rounded-xl border px-3 py-2 text-left flex justify-between items-center transition-all ${
                screenshotCapturingHotkey ? 'bg-emerald-400/20 border-emerald-300/60 text-emerald-50 animate-pulse' : 'bg-white/5 border-white/8 text-white/80 hover:bg-white/8'
              }`}
            >
              <span className="font-mono text-xs">{screenshotCapturingHotkey ? 'Press keys...' : displayScreenshotHotkey}</span>
              <span className="text-white/30 text-xs">Mighty Screenshot hotkey</span>
            </button>
            <button
              onClick={() => invoke('open_screenshot_overlay', { mode: 'screenshot' }).catch((e) => setAppError(String(e)))}
              className="w-full rounded-xl border border-emerald-400/30 bg-emerald-500/10 text-emerald-100 hover:border-emerald-300/60 px-3 py-2 text-xs"
            >
              Open Mighty Screenshot
            </button>
          </div>
        </section>

        {/* Microphone */}
        <section>
          <label className="text-white/40 text-xs font-medium uppercase tracking-wider mb-2 flex items-center gap-1.5">
            <Mic size={12} /> Microphone
          </label>
          <select value={inputDevice}
            onChange={e => {
              setInputDevice(e.target.value);
              localStorage.setItem('mightyvoice.inputDevice', e.target.value);
              invoke('set_input_device', { deviceName: e.target.value }).catch(console.error);
            }}
            className="w-full bg-white/5 border border-white/8 rounded-2xl px-4 py-3 text-white/80 text-sm focus:outline-none focus:border-white/20 appearance-none"
          >
            <option value="" className="bg-[#1a1a1a]">System Default</option>
            {inputDevices.map(d => <option key={d} value={d} className="bg-[#1a1a1a]">{d}</option>)}
          </select>
        </section>

        {/* Language */}
        <section>
          <label className="text-white/40 text-xs font-medium uppercase tracking-wider mb-2 flex items-center gap-1.5">
            <Languages size={12} /> Language
          </label>
          <select value={language} onChange={e => setLanguage(e.target.value)}
            className="w-full bg-white/5 border border-white/8 rounded-2xl px-4 py-3 text-white/80 text-sm focus:outline-none focus:border-white/20 appearance-none"
          >
            <option value="auto" className="bg-[#1a1a1a]">Auto-detect / Mixed</option>
            <option value="yue"  className="bg-[#1a1a1a]">Cantonese (廣東話)</option>
            <option value="zh"   className="bg-[#1a1a1a]">Mandarin (普通話)</option>
            <option value="tl"   className="bg-[#1a1a1a]">Tagalog / Filipino</option>
            <option value="en"   className="bg-[#1a1a1a]">English</option>
          </select>
        </section>

        {/* API Key */}
        <section>
          <label className="text-white/40 text-xs font-medium uppercase tracking-wider mb-2 flex items-center gap-1.5">
            <Key size={12} /> Groq API Key
          </label>
          <div className="flex gap-2">
            <input
              type="password"
              value={apiKey}
              onChange={e => { setApiKey(e.target.value); setApiKeySaved(false); }}
              placeholder="gsk_..."
              className="min-w-0 flex-1 bg-white/5 border border-white/8 rounded-2xl px-4 py-3 text-white/80 text-sm placeholder:text-white/20 focus:outline-none focus:border-white/20"
            />
            <button
              onClick={applyApiKey}
              className={`rounded-2xl border px-4 py-3 text-xs font-semibold transition-all ${apiKeySaved ? 'bg-emerald-400/15 border-emerald-300/30 text-emerald-100' : 'bg-white/10 border-white/15 text-white/80 hover:bg-white/15 hover:text-white'}`}
            >
              {apiKeySaved ? 'Saved' : 'Save'}
            </button>
          </div>
          {apiKeyStatus && apiKeyStatus.hasKey && (
            <p className="mt-1.5 text-[10px] text-white/25 flex items-center gap-1">
              <span className="w-1.5 h-1.5 rounded-full bg-emerald-400/60 inline-block" />
              Key loaded from {apiKeyStatus.source.replace('_', ' ')}
            </p>
          )}
          {apiKeyStatus && !apiKeyStatus.hasKey && (
            <p className="mt-1.5 text-[10px] text-white/20 flex items-center gap-1">
              <span className="w-1.5 h-1.5 rounded-full bg-white/15 inline-block" />
              No Groq key detected
            </p>
          )}
          <p className="mt-2 text-[11px] leading-relaxed text-white/35">
            Voice transcription and cloud OCR use Groq keys that start with gsk_. xAI keys are not supported yet. Local OCR works without a key.
          </p>
        </section>

        {/* Launch at Login */}
        <section>
          <button
            onClick={() => {
              const next = !autostart;
              setAutostart(next);
              invoke(next ? 'plugin:autostart|enable' : 'plugin:autostart|disable').catch(console.error);
            }}
            className={`w-full rounded-2xl border px-4 py-3 flex justify-between items-center transition-all ${
              autostart ? 'bg-white/10 border-white/20 text-white' : 'bg-white/5 border-white/8 text-white/50 hover:bg-white/8'
            }`}
          >
            <span className="text-sm">Launch at Login</span>
            <span className={`text-xs font-mono ${autostart ? 'text-white/60' : 'text-white/20'}`}>{autostart ? 'ON' : 'OFF'}</span>
          </button>
        </section>

        {/* Live Log */}
        {logs.length > 0 && (
          <section>
            <label className="text-white/40 text-xs font-medium uppercase tracking-wider mb-2 block">Live Log</label>
            <div className="bg-black/40 border border-white/8 rounded-2xl p-3 space-y-1 max-h-36 overflow-y-auto">
              {logs.map((log, i) => <div key={i} className="text-xs font-mono text-white/60 leading-relaxed">{log}</div>)}
            </div>
          </section>
        )}
      </div>

      {/* ── History panel ── */}
      {historyOpen && (
        <div className="w-[280px] flex-shrink-0 border-l border-white/8 flex flex-col">
          <div className="flex items-center justify-between px-4 py-3 border-b border-white/8">
            <span className="text-white/60 text-xs font-medium uppercase tracking-wider">History</span>
            <div className="flex items-center gap-2">
              {history.length > 0 && (
                <button onClick={() => { setHistory([]); setSelectedHistoryIds([]); localStorage.removeItem('mightyvoice.history'); }}
                  className="text-white/20 hover:text-red-400 transition-colors" title="Clear all">
                  <Trash2 size={12} />
                </button>
              )}
              <button
                onClick={async () => {
                  setHistoryOpen(false);
                  try {
                    const { getCurrentWindow, LogicalSize } = await import('@tauri-apps/api/window');
                    const win = getCurrentWindow();
                    const s = await win.innerSize();
                    const f = await win.scaleFactor();
                    await win.setSize(new LogicalSize(560, Math.round(s.height / f)));
                  } catch {}
                }}
                className="text-white/20 hover:text-white transition-colors" title="Close history">
                <ChevronRight size={12} />
              </button>
            </div>
          </div>

          <div className="flex-1 overflow-y-auto p-3 space-y-2">
            {history.length > 0 && (
              <div className="sticky top-0 z-10 mb-2 bg-[#0f0f10]/95 backdrop-blur rounded-lg border border-white/8 p-2 flex items-center gap-2">
                <button onClick={selectAllHistory} className="text-[10px] px-2 py-1 rounded border border-white/15 text-white/60 hover:text-white hover:border-white/30">Select all</button>
                <button onClick={clearHistorySelection} className="text-[10px] px-2 py-1 rounded border border-white/15 text-white/60 hover:text-white hover:border-white/30">Clear</button>
                <button
                  onClick={copySelectedHistory}
                  disabled={selectedHistoryIds.length === 0}
                  className={`text-[10px] px-2 py-1 rounded border ${selectedHistoryIds.length === 0 ? 'border-white/10 text-white/20 cursor-not-allowed' : 'border-white/20 text-white/70 hover:text-white hover:border-white/40'}`}
                >
                  Copy selected ({selectedHistoryIds.length}) oldest → newest
                </button>
              </div>
            )}
            {history.length === 0 ? (
              <p className="text-white/20 text-xs text-center mt-8">No dictations yet</p>
            ) : (
              history.map(item => {
                const expanded = expandedId === item.id;
                const selected = selectedHistoryIds.includes(item.id);
                return (
                <div key={item.id}
                  className={`group rounded-xl p-3 transition-all border ${selected ? 'bg-white/12 border-white/35' : 'bg-white/5 border-white/8 hover:bg-white/8'}`}
                >
                  <div className="flex items-start gap-2">
                    <input
                      type="checkbox"
                      checked={selected}
                      onChange={() => toggleHistorySelection(item.id)}
                      className="mt-0.5 accent-white"
                    />
                    <p
                      onClick={() => setExpandedId(expanded ? null : item.id)}
                      className={`text-white/80 text-xs leading-relaxed cursor-pointer flex-1 ${expanded ? '' : 'line-clamp-3'}`}
                    >{item.text}</p>
                  </div>
                  <div className="flex items-center justify-between mt-2">
                    <span className="text-white/20 text-[10px] font-mono">{item.ts}</span>
                    <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                      <button onClick={() => deleteItem(item.id)}
                        className="p-1 rounded-lg text-white/30 hover:text-red-400 hover:bg-red-400/10 transition-all">
                        <Trash2 size={10} />
                      </button>
                      <button onClick={() => copyItem(item)}
                        className="p-1 rounded-lg text-white/30 hover:text-white hover:bg-white/10 transition-all">
                        {copiedId === item.id ? <Check size={10} className="text-green-400" /> : <Copy size={10} />}
                      </button>
                    </div>
                  </div>
                </div>
                );
              })
            )}
          </div>
        </div>
      )}
    </div>
  );
}
