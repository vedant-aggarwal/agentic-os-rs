// Bottom sheet: scrim, grabber, drag down to dismiss (velocity aware), safe-area padding.
// On wide screens it becomes a centred floating panel.
import { motion, useDragControls, type PanInfo } from 'motion/react';
import { useEffect } from 'react';
import { X } from 'lucide-react';
import { spring } from './motion';

interface Props {
  onClose: () => void;
  title?: React.ReactNode;
  subtitle?: React.ReactNode;
  children: React.ReactNode;
  /** full = up to the top safe area; auto = content height */
  size?: 'auto' | 'full';
  tone?: string; // a pillar hue to tint the top glow
  hideClose?: boolean;
}

export function Sheet({ onClose, title, subtitle, children, size = 'auto', tone, hideClose }: Props) {
  const drag = useDragControls();
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    addEventListener('keydown', onKey);
    return () => removeEventListener('keydown', onKey);
  }, [onClose]);

  const end = (_: unknown, info: PanInfo) => {
    if (info.offset.y > 110 || info.velocity.y > 650) onClose();
  };

  return (
    <div className="fixed inset-0 z-50 flex items-end justify-center md:items-center" role="dialog" aria-modal="true">
      <motion.div
        className="absolute inset-0"
        style={{ background: 'var(--scrim)' }}
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        exit={{ opacity: 0 }}
        transition={{ duration: 0.24 }}
        onClick={onClose}
      />
      <motion.div
        className={`sheet relative flex w-full flex-col overflow-hidden rounded-t-[var(--radius-sheet)] md:mb-0 md:max-w-[440px] md:rounded-[32px] ${
          size === 'full' ? 'h-[calc(100dvh-var(--safe-t)-12px)] md:h-[min(760px,88dvh)]' : 'max-h-[calc(100dvh-var(--safe-t)-12px)] md:max-h-[88dvh]'
        }`}
        initial={{ y: '100%' }}
        animate={{ y: 0 }}
        exit={{ y: '100%' }}
        transition={spring.sheet}
        drag="y"
        dragListener={false}
        dragControls={drag}
        dragConstraints={{ top: 0, bottom: 0 }}
        dragElastic={{ top: 0.04, bottom: 0.9 }}
        onDragEnd={end}
      >
        {tone && (
          <div
            aria-hidden
            className="pointer-events-none absolute inset-x-0 top-0 h-48 opacity-60"
            style={{ background: `radial-gradient(70% 100% at 50% 0%, color-mix(in oklab, ${tone} 35%, transparent), transparent 70%)` }}
          />
        )}
        <div className="relative shrink-0 touch-none pt-2" onPointerDown={(e) => drag.start(e)}>
          <div className="mx-auto h-[5px] w-9 rounded-full bg-white/25" />
          {(title || !hideClose) && (
            <div className="flex items-start gap-3 px-5 pt-3 pb-2">
              <div className="min-w-0 flex-1">
                {title && <h2 className="text-title2 font-bold">{title}</h2>}
                {subtitle && <p className="mt-0.5 text-subhead text-ink-2">{subtitle}</p>}
              </div>
              {!hideClose && (
                <button
                  aria-label="Close"
                  onClick={onClose}
                  className="press -mt-1 -mr-1 grid size-11 shrink-0 place-items-center rounded-full"
                >
                  <span className="grid size-8 place-items-center rounded-full bg-fill-2">
                    <X size={16} strokeWidth={2.4} className="text-ink-2" />
                  </span>
                </button>
              )}
            </div>
          )}
        </div>
        <div className="relative min-h-0 flex-1 overflow-y-auto overscroll-contain px-5 pb-[calc(var(--safe-b)+20px)]">{children}</div>
      </motion.div>
    </div>
  );
}
