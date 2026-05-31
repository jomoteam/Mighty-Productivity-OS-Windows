import { useEffect, useState, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';

const BAR_COUNT = 18;

function Audiogram({ active }: { active: boolean }) {
  const [bars, setBars] = useState<number[]>(Array(BAR_COUNT).fill(2));
  const animRef = useRef<number | null>(null);

  useEffect(() => {
    if (!active) {
      setBars(Array(BAR_COUNT).fill(2));
      if (animRef.current) cancelAnimationFrame(animRef.current);
      return;
    }
    const animate = () => {
      setBars(prev => prev.map((_, i) => {
        const center = BAR_COUNT / 2;
        const dist = Math.abs(i - center) / center;
        const max = 14 - dist * 6;
        return Math.max(2, Math.random() * max);
      }));
      animRef.current = requestAnimationFrame(animate);
    };
    animRef.current = requestAnimationFrame(animate);
    return () => { if (animRef.current) cancelAnimationFrame(animRef.current); };
  }, [active]);

  return (
    <div className="flex items-center gap-[2px]" style={{ height: '16px' }}>
      {bars.map((h, i) => (
        <div
          key={i}
          style={{ height: `${h}px`, transition: active ? 'height 0.07s ease' : 'none' }}
          className="w-[2px] rounded-full bg-black/70"
        />
      ))}
    </div>
  );
}

export default function FloatingIndicator() {
  const [state, setState] = useState<'idle' | 'started' | 'processing'>('idle');

  useEffect(() => {
    // Rust (show_indicator) handles window positioning — don't reposition here
    const appWindow = getCurrentWindow();
    appWindow.setIgnoreCursorEvents(true).catch(console.error);

    const unsub = listen('recording-state', (event) => {
      const newState = event.payload as 'idle' | 'started' | 'processing';
      setState(newState);
      if (newState === 'idle') {
        appWindow.setIgnoreCursorEvents(true).catch(console.error);
      } else {
        appWindow.setIgnoreCursorEvents(false).catch(console.error);
      }
    });

    return () => { unsub.then(fn => fn()); };
  }, []);

  if (state === 'idle') return null;

  return (
    <div className="w-full h-full flex items-center justify-center bg-transparent">
      <div
        data-tauri-drag-region=""
        className="flex items-center gap-2.5 bg-white/95 backdrop-blur px-4 py-2 rounded-full cursor-move"
        style={{ boxShadow: '0 2px 20px rgba(0,0,0,0.22)', border: '1px solid rgba(0,0,0,0.06)' }}
      >
        {state === 'started' ? (
          <Audiogram active={true} />
        ) : (
          <div className="flex items-center gap-2.5">
            <Audiogram active={false} />
            <div className="w-3 h-3 border-2 border-black/15 border-t-black/60 rounded-full animate-spin flex-shrink-0" />
          </div>
        )}
      </div>
    </div>
  );
}
