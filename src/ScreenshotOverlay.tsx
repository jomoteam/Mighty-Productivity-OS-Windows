import { useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';

type Point = { x: number; y: number };
type Rect = { x: number; y: number; width: number; height: number };
type OverlayMode = 'screenshot' | 'ocr';
type Action = 'copy' | 'ocr';
type Tool = 'select' | 'pen' | 'rect' | 'arrow' | 'text' | 'blur' | 'pixelate';
type Annotation =
  | { type: 'pen'; points: Point[] }
  | { type: 'rect' | 'arrow' | 'blur' | 'pixelate'; start: Point; end: Point }
  | { type: 'text'; point: Point; text: string };

const TOOL_ICONS: Record<Tool, { icon: string; label: string }> = {
  select: { icon: '✥', label: 'Select area' },
  pen: { icon: '✎', label: 'Draw pen' },
  rect: { icon: '▭', label: 'Rectangle' },
  arrow: { icon: '↗', label: 'Arrow' },
  text: { icon: 'T', label: 'Text' },
  blur: { icon: '◍', label: 'Blur area' },
  pixelate: { icon: '▦', label: 'Pixelate area' },
};

function normalizeRect(a: Point, b: Point): Rect {
  const x = Math.min(a.x, b.x);
  const y = Math.min(a.y, b.y);
  return { x, y, width: Math.abs(a.x - b.x), height: Math.abs(a.y - b.y) };
}

  const usePhysicalScreenCoords = () => (window.devicePixelRatio || 1) > 1.05;

  const toScreenRect = (rect: Rect): Rect => {
    // In this app the overlay covers the full physical desktop, while React sees
    // CSS pixels on scaled Windows displays. CopyFromScreen needs physical pixels.
    const scale = usePhysicalScreenCoords() ? (window.devicePixelRatio || 1) : 1;
    return {
      x: rect.x * scale,
      y: rect.y * scale,
      width: rect.width * scale,
      height: rect.height * scale,
    };
  };

function scalePoint(point: Point, scale: number): Point {
  return { x: point.x * scale, y: point.y * scale };
}

function scaleAnnotation(annotation: Annotation, scale: number): Annotation {
  if (annotation.type === 'pen') return { ...annotation, points: annotation.points.map((p) => scalePoint(p, scale)) };
  if (annotation.type === 'text') return { ...annotation, point: scalePoint(annotation.point, scale) };
  return { ...annotation, start: scalePoint(annotation.start, scale), end: scalePoint(annotation.end, scale) };
}

type ScreenshotOverlayProps = { initialMode?: OverlayMode };

declare global {
  interface Window { __MIGHTY_OVERLAY_MODE?: OverlayMode }
}

export default function ScreenshotOverlay({ initialMode = 'screenshot' }: ScreenshotOverlayProps) {
  const [mode, setMode] = useState<OverlayMode>(initialMode);
  const [start, setStart] = useState<Point | null>(null);
  const [end, setEnd] = useState<Point | null>(null);
  const [dragging, setDragging] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState('Drag to select. Esc or right-click cancels.');
  const [tool, setTool] = useState<Tool>('select');
  const [annotations, setAnnotations] = useState<Annotation[]>([]);
  const [draft, setDraft] = useState<Annotation | null>(null);
  const [editingTextIndex, setEditingTextIndex] = useState<number | null>(null);
  const dragKind = useRef<'selection' | 'annotation' | 'text' | null>(null);
  const draggingTextIndex = useRef<number | null>(null);
  const textDragOffset = useRef<Point>({ x: 0, y: 0 });

  const rect = useMemo(() => start && end ? normalizeRect(start, end) : null, [start, end]);
  const validRect = rect && rect.width >= 8 && rect.height >= 8 ? rect : null;

  const resetOverlay = (nextMode = mode) => {
    setStart(null); setEnd(null); setDragging(false); setBusy(false); setTool('select');
    setAnnotations([]); setDraft(null); setEditingTextIndex(null);
    dragKind.current = null; draggingTextIndex.current = null;
    setMessage(nextMode === 'ocr' ? 'OCR: drag text area. Esc cancels.' : 'Drag to select. Esc or right-click cancels.');
  };

  const closeOverlay = async () => {
    resetOverlay();
    setDragging(false);
    setBusy(false);
    setDraft(null);
    setEditingTextIndex(null);
    dragKind.current = null;
    draggingTextIndex.current = null;
    const win = getCurrentWindow();
    // Hide first. On Windows fullscreen transitions can hang or steal focus,
    // which made Esc / the red X look dead until Copy closed the overlay.
    await win.hide().catch(() => {});
    win.setFullscreen(false).catch(() => {});
  };

  useEffect(() => {
    const injectedMode = window.__MIGHTY_OVERLAY_MODE;
    const fromTitle: OverlayMode = document.title.toLowerCase().includes('ocr') ? 'ocr' : 'screenshot';
    const startingMode: OverlayMode = injectedMode === 'ocr' || injectedMode === 'screenshot' ? injectedMode : (initialMode === 'ocr' ? 'ocr' : fromTitle);
    setMode(startingMode);
    resetOverlay(startingMode);

    const applyInjectedMode = (event: Event) => {
      const detail = (event as CustomEvent<OverlayMode>).detail;
      const nextMode = detail === 'ocr' ? 'ocr' : 'screenshot';
      window.__MIGHTY_OVERLAY_MODE = nextMode;
      setMode(nextMode);
      resetOverlay(nextMode);
      setTimeout(() => window.focus(), 0);
    };
    window.addEventListener('mighty-overlay-mode', applyInjectedMode as EventListener);

    const unlistenPromise = listen<OverlayMode>('screenshot-overlay-mode', (event) => {
      const nextMode = event.payload === 'ocr' ? 'ocr' : 'screenshot';
      window.__MIGHTY_OVERLAY_MODE = nextMode;
      setMode(nextMode);
      resetOverlay(nextMode);
      setTimeout(() => window.focus(), 0);
    });
    return () => {
      window.removeEventListener('mighty-overlay-mode', applyInjectedMode as EventListener);
      unlistenPromise.then((unlisten) => unlisten()).catch(() => {});
    };
  }, []);

  useEffect(() => {
    const win = getCurrentWindow();
    win.setFocus().catch(() => {});
    const focusOverlay = () => window.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); closeOverlay(); return; }
      if (e.key === 'Enter' && validRect && !busy && editingTextIndex === null) {
        e.preventDefault();
        runAction(mode === 'ocr' ? 'ocr' : 'copy');
        return;
      }
      if (e.key.toLowerCase() === 'z' && (e.ctrlKey || e.metaKey)) {
        e.preventDefault(); setAnnotations(prev => prev.slice(0, -1));
      }
    };
    const endDrag = () => { setDragging(false); dragKind.current = null; draggingTextIndex.current = null; };
    window.addEventListener('keydown', onKey, true);
    window.addEventListener('keyup', onKey, true);
    window.addEventListener('pointerdown', focusOverlay, true);
    window.addEventListener('blur', endDrag);
    window.addEventListener('mouseup', endDrag);
    return () => {
      window.removeEventListener('keydown', onKey, true);
      window.removeEventListener('keyup', onKey, true);
      window.removeEventListener('pointerdown', focusOverlay, true);
      window.removeEventListener('blur', endDrag);
      window.removeEventListener('mouseup', endDrag);
    };
  }, [validRect, busy, editingTextIndex, mode]);

  const runAction = async (action: Action, rectOverride?: Rect) => {
    const actionRect = rectOverride ?? validRect;
    if (!actionRect || busy) return;
    const scale = usePhysicalScreenCoords() ? (window.devicePixelRatio || 1) : 1;
    const screenRect = toScreenRect(actionRect);
    setBusy(true);
    setDragging(false);
    dragKind.current = null;
    setMessage(action === 'ocr' ? 'Reading text...' : 'Copying image...');
    try {
      const result = await invoke<string>('screenshot_overlay_action', {
        action,
        rect: screenRect,
        annotations: action === 'copy' ? annotations.map((a) => scaleAnnotation(a, scale)) : [],
      });
      setMessage(result);
      setTimeout(closeOverlay, 700);
    } catch (e) {
      setMessage(String(e));
      setBusy(false);
    }
  };

  const toolbarLeft = validRect ? Math.min(window.innerWidth - 430, Math.max(12, validRect.x)) : 12;
  const toolbarTop = validRect ? Math.min(window.innerHeight - 62, validRect.y + validRect.height + 10) : 12;

  const toLocal = (p: Point): Point => validRect ? { x: p.x - validRect.x, y: p.y - validRect.y } : p;

  const beginPointer = (e: React.PointerEvent<HTMLDivElement>) => {
    if (busy || e.button !== 0) return;
    const p = { x: e.clientX, y: e.clientY };

    if (validRect && tool === 'text') {
      const local = toLocal(p);
      const hitIndex = annotations.findIndex((a) => a.type === 'text' && Math.abs(a.point.x - local.x) < 90 && Math.abs(a.point.y - local.y) < 28);
      if (hitIndex >= 0 && annotations[hitIndex].type === 'text') {
        const a = annotations[hitIndex] as Extract<Annotation, { type: 'text' }>;
        draggingTextIndex.current = hitIndex;
        textDragOffset.current = { x: local.x - a.point.x, y: local.y - a.point.y };
        dragKind.current = 'text';
        setDragging(true);
        return;
      }
      const newIndex = annotations.length;
      setAnnotations(prev => [...prev, { type: 'text', point: local, text: '' }]);
      setEditingTextIndex(newIndex);
      return;
    }

    if (validRect && tool !== 'select' && mode === 'screenshot') {
      dragKind.current = 'annotation';
      setDragging(true);
      const local = toLocal(p);
      if (tool === 'pen') setDraft({ type: 'pen', points: [local] });
      else if (tool === 'rect' || tool === 'arrow' || tool === 'blur' || tool === 'pixelate') setDraft({ type: tool, start: local, end: local });
      return;
    }

    dragKind.current = 'selection';
    setStart(p); setEnd(p); setDragging(true); setAnnotations([]); setDraft(null); setEditingTextIndex(null);
    setMessage('Release to confirm region. Esc cancels.');
  };

  const movePointer = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!dragging || busy) return;
    const p = { x: e.clientX, y: e.clientY };
    if (dragKind.current === 'selection') setEnd(p);
    if (dragKind.current === 'text' && validRect && draggingTextIndex.current !== null) {
      const local = toLocal(p);
      const index = draggingTextIndex.current;
      setAnnotations(prev => prev.map((a, i) => i === index && a.type === 'text'
        ? { ...a, point: { x: local.x - textDragOffset.current.x, y: local.y - textDragOffset.current.y } }
        : a));
    }
    if (dragKind.current === 'annotation' && validRect && draft) {
      const local = toLocal(p);
      if (draft.type === 'pen') setDraft({ ...draft, points: [...draft.points, local] });
      else if (draft.type === 'rect' || draft.type === 'arrow' || draft.type === 'blur' || draft.type === 'pixelate') setDraft({ ...draft, end: local });
    }
  };

  const finishPointer = (e: React.PointerEvent<HTMLDivElement>) => {
    const p = { x: e.clientX, y: e.clientY };
    let finishedRect = validRect;
    if (dragKind.current === 'selection' && start) {
      setEnd(p);
      const candidate = normalizeRect(start, p);
      finishedRect = candidate.width >= 8 && candidate.height >= 8 ? candidate : null;
      if (mode === 'ocr' && finishedRect) {
        setDraft(null); setDragging(false); dragKind.current = null; draggingTextIndex.current = null;
        runAction('ocr', finishedRect);
        return;
      }
    }
    if (dragKind.current === 'selection' && !finishedRect) setMessage('Selection too small — drag a larger area, or press Esc to exit.');
    if (dragKind.current === 'annotation' && draft) setAnnotations(prev => [...prev, draft]);
    setDraft(null); setDragging(false); dragKind.current = null; draggingTextIndex.current = null;
  };

  const updateText = (index: number, text: string) => {
    setAnnotations(prev => prev.map((a, i) => i === index && a.type === 'text' ? { ...a, text } : a));
  };

  const allAnnotations = draft ? [...annotations, draft] : annotations;
  const visibleTools: Tool[] = mode === 'ocr' ? ['select'] : ['select', 'pen', 'rect', 'arrow', 'text', 'blur', 'pixelate'];

  const cancelPointer = (e: React.PointerEvent | React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    closeOverlay();
  };

  return (
    <div
      tabIndex={0}
      className="fixed inset-0 select-none cursor-crosshair text-white outline-none"
      style={{ background: 'rgba(0,0,0,0.38)' }}
      onPointerDown={beginPointer}
      onPointerMove={movePointer}
      onPointerUp={finishPointer}
      onContextMenu={(e) => { e.preventDefault(); e.stopPropagation(); closeOverlay(); }}
    >
      {validRect && (
        <>
          <div
            className="absolute border-2 border-emerald-400 bg-transparent shadow-[0_0_0_9999px_rgba(0,0,0,0.28)]"
            style={{ left: validRect.x, top: validRect.y, width: validRect.width, height: validRect.height }}
          >
            {['-top-1 -left-1','-top-1 -right-1','-bottom-1 -left-1','-bottom-1 -right-1'].map((pos) => (
              <div key={pos} className={`absolute ${pos} w-3 h-3 rounded-full bg-emerald-300 border border-black/40`} />
            ))}
            <div className="absolute -top-7 left-0 rounded bg-black/80 px-2 py-1 text-[11px] font-mono text-white/90">
              {Math.round(validRect.width)} × {Math.round(validRect.height)}
            </div>
            <svg className="absolute inset-0 w-full h-full pointer-events-none overflow-visible">
              {allAnnotations.map((a, i) => {
                if (a.type === 'pen') return <polyline key={i} points={a.points.map(p => `${p.x},${p.y}`).join(' ')} fill="none" stroke="#22c55e" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round" />;
                if (a.type === 'rect') return <rect key={i} x={Math.min(a.start.x, a.end.x)} y={Math.min(a.start.y, a.end.y)} width={Math.abs(a.end.x - a.start.x)} height={Math.abs(a.end.y - a.start.y)} fill="none" stroke="#22c55e" strokeWidth="3" />;
                if (a.type === 'arrow') return <line key={i} x1={a.start.x} y1={a.start.y} x2={a.end.x} y2={a.end.y} stroke="#22c55e" strokeWidth="3" strokeLinecap="round" markerEnd="url(#arrowhead)" />;
                if (a.type === 'blur') return <rect key={i} x={Math.min(a.start.x, a.end.x)} y={Math.min(a.start.y, a.end.y)} width={Math.abs(a.end.x - a.start.x)} height={Math.abs(a.end.y - a.start.y)} fill="rgba(56,189,248,0.20)" stroke="#38bdf8" strokeDasharray="8 5" strokeWidth="2" />;
                if (a.type === 'pixelate') return <rect key={i} x={Math.min(a.start.x, a.end.x)} y={Math.min(a.start.y, a.end.y)} width={Math.abs(a.end.x - a.start.x)} height={Math.abs(a.end.y - a.start.y)} fill="rgba(251,191,36,0.20)" stroke="#fbbf24" strokeDasharray="8 5" strokeWidth="2" />;
                return null;
              })}
              <defs><marker id="arrowhead" markerWidth="10" markerHeight="7" refX="9" refY="3.5" orient="auto"><polygon points="0 0, 10 3.5, 0 7" fill="#22c55e" /></marker></defs>
            </svg>
            {annotations.map((a, i) => a.type === 'text' && (
              editingTextIndex === i ? (
                <input
                  key={i}
                  autoFocus
                  value={a.text}
                  onChange={(e) => updateText(i, e.target.value)}
                  onBlur={() => setEditingTextIndex(null)}
                  onKeyDown={(e) => { if (e.key === 'Enter') setEditingTextIndex(null); if (e.key === 'Escape') { e.stopPropagation(); setEditingTextIndex(null); } }}
                  className="absolute min-w-24 rounded bg-black/45 border border-emerald-300 px-2 py-1 text-emerald-300 font-semibold text-sm outline-none"
                  style={{ left: a.point.x, top: a.point.y }}
                />
              ) : (
                <div
                  key={i}
                  className="absolute cursor-move text-emerald-300 font-semibold text-sm drop-shadow px-1 py-0.5 rounded hover:bg-black/25"
                  style={{ left: a.point.x, top: a.point.y }}
                  onDoubleClick={(e) => { e.stopPropagation(); setEditingTextIndex(i); }}
                >{a.text || 'Type text'}</div>
              )
            ))}
          </div>
          {mode === 'screenshot' && <div
            className="absolute flex items-center gap-2 rounded-2xl bg-[#151515]/95 border border-white/15 shadow-2xl px-3 py-2 backdrop-blur cursor-default"
            style={{ left: toolbarLeft, top: toolbarTop }}
            onPointerDown={(e) => { e.preventDefault(); e.stopPropagation(); setDragging(false); dragKind.current = null; }}
            onPointerMove={(e) => e.stopPropagation()}
            onPointerUp={(e) => { e.preventDefault(); e.stopPropagation(); }}
          >
            {visibleTools.map(t => (
              <button
                key={t}
                disabled={busy}
                title={TOOL_ICONS[t].label}
                aria-label={TOOL_ICONS[t].label}
                onClick={() => setTool(t)}
                className={`min-w-9 px-2.5 py-1.5 rounded-xl text-base disabled:opacity-50 ${tool === t ? 'bg-emerald-400 text-black font-semibold' : 'bg-white/10 hover:bg-white/20'}`}
              >{TOOL_ICONS[t].icon}</button>
            ))}
            {mode === 'screenshot' && <button disabled={busy} title="Undo" onClick={() => setAnnotations(prev => prev.slice(0, -1))} className="px-2.5 py-1.5 rounded-xl bg-white/10 hover:bg-white/20 text-sm disabled:opacity-50">↶</button>}
            <span className="h-5 w-px bg-white/15" />
            <button disabled={busy} onClick={() => runAction('copy')} className="px-3 py-1.5 rounded-xl bg-emerald-400 text-black text-sm font-semibold disabled:opacity-50">Copy</button>
            <button disabled={busy} onPointerDown={cancelPointer} onPointerUp={(e) => { e.preventDefault(); e.stopPropagation(); }} onClick={cancelPointer} title="Cancel" className="px-3 py-1.5 rounded-xl bg-red-500/20 hover:bg-red-500/35 text-sm disabled:opacity-50">✕</button>
          </div>}
        </>
      )}
      <button
        disabled={busy}
        onPointerDown={cancelPointer}
        onPointerUp={(e) => { e.preventDefault(); e.stopPropagation(); }}
        onClick={cancelPointer}
        title="Cancel"
        className="absolute right-5 top-5 rounded-full bg-red-500/25 hover:bg-red-500/45 border border-red-200/20 px-3 py-1.5 text-sm disabled:opacity-50"
      >✕</button>
      <div className="absolute left-1/2 top-5 -translate-x-1/2 rounded-full bg-black/75 border border-white/10 px-4 py-2 text-xs text-white/80 shadow-xl">
        {message} {mode === 'screenshot' && tool !== 'select' && validRect ? `Tool: ${TOOL_ICONS[tool].icon}` : ''}
      </div>
    </div>
  );
}
