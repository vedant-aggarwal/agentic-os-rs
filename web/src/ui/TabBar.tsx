// Floating glass tab bar with a lit lens that slides to the active tab. On wide screens the same
// tabs become a slim left rail.
import { motion } from 'motion/react';
import { CalendarCheck, Inbox, Sprout, SquareTerminal, Mountain, type LucideIcon } from 'lucide-react';
import { useInbox } from '@/lib/queries';
import { haptic, useUI, type Tab } from '@/lib/ui';
import { spring } from './motion';

const ITEMS: { id: Tab; label: string; icon: LucideIcon }[] = [
  { id: 'today', label: 'Today', icon: CalendarCheck },
  { id: 'inbox', label: 'Inbox', icon: Inbox },
  { id: 'life', label: 'Life', icon: Sprout },
  { id: 'work', label: 'Work', icon: SquareTerminal },
  { id: 'mission', label: 'Mission', icon: Mountain },
];

export function TabBar() {
  const { tab, setTab, pages } = useUI();
  const { data: inbox } = useInbox();
  const unread = (inbox || []).filter((i) => !i.handled && i.triage_yes !== false).length;

  const go = (t: Tab) => {
    if (t === tab && !pages.length) {
      document.getElementById('scroller')?.scrollTo({ top: 0, behavior: 'smooth' });
      return;
    }
    haptic('light');
    setTab(t);
  };

  return (
    <>
      {/* phone: bottom capsule */}
      <div className="pointer-events-none fixed inset-x-0 bottom-0 z-20 pb-[max(var(--safe-b),12px)] md:hidden">
        <div aria-hidden className="absolute inset-x-0 bottom-0 h-28" style={{ background: 'linear-gradient(180deg, transparent, rgb(5 5 7 / 0.85))' }} />
        <nav aria-label="Sections" className="glass pointer-events-auto relative mx-auto flex h-[var(--tabbar-h)] w-[calc(100%-32px)] max-w-[420px] items-stretch rounded-full p-1">
          {ITEMS.map(({ id, label, icon: Icon }) => {
            const on = tab === id;
            return (
              <button key={id} onClick={() => go(id)} aria-current={on ? 'page' : undefined} className="relative flex flex-1 flex-col items-center justify-center gap-0.5 rounded-full">
                {on && <motion.span layoutId="tab-lens" className="absolute inset-0 rounded-full" style={{ background: 'rgb(255 255 255 / 0.13)', boxShadow: 'inset 0 1px 0 rgb(255 255 255 / 0.2), inset 0 0 0 0.5px rgb(255 255 255 / 0.08)' }} transition={spring.snappy} />}
                <motion.span className="relative" animate={{ scale: on ? 1 : 0.94 }} transition={spring.snappy}>
                  <Icon size={22} strokeWidth={on ? 2.3 : 1.9} className={on ? 'text-ink' : 'text-ink-2'} />
                  {id === 'inbox' && unread > 0 && (
                    <span className="absolute -top-1.5 -right-2.5 grid h-[17px] min-w-[17px] place-items-center rounded-full bg-needs px-1 text-[11px] font-bold text-ground tnum">{unread > 99 ? '99+' : unread}</span>
                  )}
                </motion.span>
                <span className={`relative text-[10.5px] leading-[13px] font-semibold ${on ? 'text-ink' : 'text-ink-2'}`}>{label}</span>
              </button>
            );
          })}
        </nav>
      </div>

      {/* desktop: slim rail */}
      <nav aria-label="Sections" className="fixed inset-y-0 left-0 z-20 hidden w-[84px] flex-col items-center gap-2 pt-8 md:flex">
        <span className="mb-6 grid size-10 place-items-center rounded-[13px] bg-fill-2">
          <span className="size-4 rounded-full border-[2.5px] border-mind" />
        </span>
        {ITEMS.map(({ id, label, icon: Icon }) => {
          const on = tab === id;
          return (
            <button key={id} onClick={() => go(id)} className="press relative flex w-16 flex-col items-center gap-1 rounded-[18px] py-2.5">
              {on && <motion.span layoutId="rail-lens" className="absolute inset-0 rounded-[18px]" style={{ background: 'rgb(255 255 255 / 0.1)', boxShadow: 'inset 0 1px 0 rgb(255 255 255 / 0.14)' }} transition={spring.snappy} />}
              <Icon size={22} strokeWidth={on ? 2.3 : 1.9} className={`relative ${on ? 'text-ink' : 'text-ink-2'}`} />
              <span className={`relative text-caption font-medium ${on ? 'text-ink' : 'text-ink-3'}`}>{label}</span>
              {id === 'inbox' && unread > 0 && <span className="absolute top-1.5 right-3 size-2 rounded-full bg-needs" />}
            </button>
          );
        })}
      </nav>
    </>
  );
}
