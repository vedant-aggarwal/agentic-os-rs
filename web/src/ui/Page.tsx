// A pushed page: slides in from the right over the tab, the tab below dims and shifts 30%.
// Swipe from the left edge to go back (commits past a third of the width or on a flick).
import { motion, useDragControls, useMotionValue, useTransform, type PanInfo } from 'motion/react';
import { ChevronLeft } from 'lucide-react';
import { useRef, useState } from 'react';
import { spring } from './motion';

interface Props {
  onBack: () => void;
  title?: string;
  /** colour bleeding in from the top */
  tone?: string;
  /** element behind the scrolling content (hero art) */
  backdrop?: React.ReactNode;
  trailing?: React.ReactNode;
  children: React.ReactNode;
  /** enter by rising from below (goal page) instead of sliding in */
  rise?: boolean;
  /** no bottom padding (a page with its own sticky composer) */
  flush?: boolean;
}

export function Page({ onBack, title, tone, backdrop, trailing, children, rise, flush }: Props) {
  const x = useMotionValue(0);
  const drag = useDragControls();
  const scroller = useRef<HTMLDivElement>(null);
  const [scrolled, setScrolled] = useState(false);
  const shade = useTransform(x, [0, 400], [1, 0.6]);

  const end = (_: unknown, info: PanInfo) => {
    if (info.offset.x > window.innerWidth / 3 || info.velocity.x > 600) onBack();
  };

  return (
    <motion.div
      className="fixed inset-0 z-30 flex flex-col overflow-hidden bg-ground md:left-[84px]"
      style={{ x, opacity: shade, background: 'var(--tex-ground) 0 0 / 180px 180px, var(--color-ground)', boxShadow: '-20px 0 60px rgb(0 0 0 / 0.6)' }}
      initial={rise ? { y: 40, opacity: 0 } : { x: '100%' }}
      animate={rise ? { y: 0, opacity: 1 } : { x: 0 }}
      exit={rise ? { y: 40, opacity: 0 } : { x: '100%' }}
      transition={spring.page}
      drag="x"
      dragListener={false}
      dragControls={drag}
      dragConstraints={{ left: 0, right: 0 }}
      dragElastic={{ left: 0, right: 1 }}
      onDragEnd={end}
    >
      {tone && (
        <div
          aria-hidden
          className="pointer-events-none absolute inset-x-0 top-0 h-[420px]"
          style={{ background: `radial-gradient(90% 70% at 50% 0%, color-mix(in oklab, ${tone} 22%, transparent), transparent 75%)` }}
        />
      )}
      {backdrop}
      {/* the left edge: 24 px that start the back swipe */}
      <div className="absolute inset-y-0 left-0 z-20 w-6 touch-none" onPointerDown={(e) => drag.start(e)} />
      <header className="absolute inset-x-0 top-0 z-10 pt-safe">
        <div className={`absolute inset-0 -bottom-6 transition-opacity duration-300 ${scrolled ? 'opacity-100' : 'opacity-0'} veil-top`} />
        <div className="relative mx-auto flex h-14 w-full max-w-[600px] items-center gap-2 px-3">
          <button aria-label="Back" onClick={onBack} className="glass press grid size-11 place-items-center rounded-full">
            <ChevronLeft size={22} strokeWidth={2.2} className="-ml-0.5" />
          </button>
          <div className={`min-w-0 flex-1 truncate text-center text-headline font-semibold transition-opacity duration-200 ${scrolled ? 'opacity-100' : 'opacity-0'}`}>{title}</div>
          <div className="flex min-w-11 justify-end">{trailing}</div>
        </div>
      </header>
      <div
        ref={scroller}
        onScroll={(e) => setScrolled((e.target as HTMLDivElement).scrollTop > 40)}
        className={`no-scrollbar relative min-h-0 flex-1 overflow-y-auto overscroll-contain ${flush ? '' : 'pb-[calc(var(--safe-b)+40px)]'}`}
      >
        <div className="mx-auto w-full max-w-[600px]">{children}</div>
      </div>
    </motion.div>
  );
}
