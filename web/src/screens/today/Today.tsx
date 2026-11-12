// Today: the hero and its stat line, mood and energy, shortcuts, then the day (timeline, tasks
// by area, habits). The hero's colour glows into the background and the panels under it.
import { useCallback, useState } from 'react';
import { useToday } from '@/lib/queries';

import { Section } from '@/ui/kit';
import { NeedsYou, NowCard, ToPlace } from './Cards';
import { HabitsSection } from './Habits';
import { MoodCard } from './Mood';
import { Shortcuts } from './Shortcuts';
import { Stage } from './Stage';
import { TasksSection } from './Tasks';
import { Timeline } from './Timeline';
import { TopBar } from './TopBar';

export function TodayScreen({ scrolled }: { scrolled: boolean }) {
  const [amb, setAmb] = useState('#8f9bff');
  const onAmbient = useCallback((c: string) => setAmb(c), []);
  const { data: today } = useToday();

  return (
    <div className="relative" style={{ ['--amb' as string]: amb, transition: '--amb 900ms ease' }}>
      {/* the cave and the ambient light from the hero */}
      <div aria-hidden className="pointer-events-none absolute inset-x-0 top-0 h-[min(92vh,780px)] overflow-hidden">
        <img src="./img/cave.webp" alt="" className="absolute inset-0 h-full w-full object-cover object-top opacity-90" fetchPriority="high" />
        <div className="absolute inset-0" style={{ background: 'radial-gradient(70% 45% at 50% 52%, color-mix(in oklab, var(--amb) 20%, transparent), transparent 70%)', mixBlendMode: 'screen' }} />
        <div className="absolute inset-0" style={{ background: 'linear-gradient(180deg, rgb(5 5 7 / 0.25) 0%, rgb(5 5 7 / 0) 18%, rgb(5 5 7 / 0) 50%, rgb(5 5 7 / 0.85) 80%, #050507 100%)' }} />
      </div>

      <TopBar solid={scrolled} />
      <div className="relative" style={{ paddingTop: 'calc(var(--safe-t) + 18px)' }}>
        <Stage onAmbient={onAmbient} />
        <div className="mt-2">
          <NowCard />
        </div>
        <NeedsYou />
        <ToPlace />
        <div className="mt-6">
          <MoodCard />
        </div>
        <Shortcuts />
        <TasksSection />
        <HabitsSection />
        <Section title="Timeline" meta={today ? new Date().toLocaleDateString('en-GB', { weekday: 'short', day: 'numeric', month: 'short' }) : undefined}>
          <Timeline />
        </Section>
        <p className="mt-10 px-8 text-center text-footnote text-ink-3">Hold a task to skip it with a reason. A miss gets a reason, never a punishment.</p>
      </div>
    </div>
  );
}
