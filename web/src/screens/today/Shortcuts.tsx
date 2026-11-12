// Shortcuts: Call Nora, Wave, Capture, Print today's plan, Launch work.
import { Mic, Phone, Printer, Rocket, Waves, type LucideIcon } from 'lucide-react';
import { haptic, useUI } from '@/lib/ui';
import { startCall } from '@/call/store';

export function Shortcuts() {
  const { open, setTab } = useUI();
  const items: { icon: LucideIcon; label: string; tint: string; on: () => void }[] = [
    { icon: Phone, label: 'Call Nora', tint: '#9b8cff', on: () => startCall('Tapped Call Nora') },
    { icon: Waves, label: 'Wave', tint: '#3fd0ff', on: () => open({ kind: 'wave' }) },
    { icon: Mic, label: 'Capture', tint: '#ff8a80', on: () => open({ kind: 'capture' }) },
    { icon: Printer, label: 'Print', tint: '#ffc24b', on: () => open({ kind: 'print' }) },
    { icon: Rocket, label: 'Launch', tint: '#7fe0ff', on: () => setTab('work') },
  ];
  return (
    <div className="mx-gutter mt-4 grid grid-cols-5 gap-1">
      {items.map(({ icon: Icon, label, tint, on }) => (
        <button
          key={label}
          onClick={() => {
            haptic('light');
            on();
          }}
          className="press group flex flex-col items-center gap-2 py-1"
        >
          <span className="glass relative grid size-[58px] place-items-center rounded-full">
            <span aria-hidden className="absolute inset-0 rounded-full opacity-0 transition-opacity group-active:opacity-100" style={{ boxShadow: `0 0 24px ${tint}55, inset 0 0 12px ${tint}33` }} />
            <Icon size={22} strokeWidth={1.9} style={{ color: `color-mix(in oklab, ${tint} 75%, white)` }} />
          </span>
          <span className="text-caption font-medium text-ink-2">{label}</span>
        </button>
      ))}
    </div>
  );
}
