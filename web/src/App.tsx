// The shell: unlock -> pairing -> tabs, with pushed pages and sheets on top.
// Desktop (md+): rail on the left, the phone layout centred in a floating panel.
import { AnimatePresence, motion } from 'motion/react';
import { useEffect, useState } from 'react';
import { CloudOff, RefreshCw } from 'lucide-react';
import { useOutbox } from '@/lib/outbox';
import { useSession } from '@/lib/session';
import { useUI, type Page as PageT } from '@/lib/ui';
import { spring } from '@/ui/motion';
import { TabBar } from '@/ui/TabBar';
import { XpFloatLayer } from '@/ui/XpFloat';
import { CallLayer } from '@/call/CallScreen';
import { AreaPage } from './screens/AreaPage';
import { GoalPage } from './screens/GoalPage';
import { NoraPage } from './screens/NoraPage';
import { Pair } from './screens/Pair';
import { PillarPage } from './screens/PillarPage';
import { Sheets } from './screens/Sheets';
import { InboxScreen, LifeScreen, MissionScreen, WorkScreen } from './screens/Tabs';
import { TodayScreen } from './screens/today/Today';
import { Unlock } from './screens/Unlock';

export function App() {
  const unlocked = useUI((s) => s.unlocked);
  const token = useSession((s) => s.token);
  const needsRepair = useSession((s) => s.needsRepair);
  const [repairOpen, setRepairOpen] = useState(false);

  useEffect(() => {
    if (needsRepair) setRepairOpen(false);
  }, [needsRepair]);

  // a write the server refused (bad args, missing tool): say so once; the refetch shows the truth
  useEffect(
    () =>
      useOutbox.subscribe((s, prev) => {
        if (s.lastError && s.lastError !== prev.lastError) useUI.getState().say(`Couldn’t save: ${s.lastError}`, 'warn');
      }),
    [],
  );

  return (
    <>
      {token && unlocked && <Shell />}
      <AnimatePresence>{unlocked && !token && <Pair key="pair" />}</AnimatePresence>
      {unlocked && token && needsRepair && repairOpen && <Pair repair />}
      {token && needsRepair && !repairOpen && <RepairBanner onOpen={() => setRepairOpen(true)} />}
      <AnimatePresence>{!unlocked && <Unlock key="unlock" />}</AnimatePresence>
      <Toast />
      <XpFloatLayer />
    </>
  );
}

function Shell() {
  const { tab, pages, pop } = useUI();
  const [scrolled, setScrolled] = useState(false);

  useEffect(() => {
    const el = document.getElementById('scroller');
    el?.scrollTo({ top: 0 });
    setScrolled(false);
  }, [tab]);

  return (
    <div className="relative h-full md:pl-[84px]">
      {/* desktop: the hero's colour bleeds past the centre panel */}
      <div aria-hidden className="pointer-events-none fixed inset-0 hidden md:block">
        <img src="./img/cave.webp" alt="" className="h-full w-full scale-110 object-cover opacity-25 blur-3xl" />
        <div className="absolute inset-0" style={{ background: 'radial-gradient(60% 80% at 50% 30%, transparent, #050507 85%)' }} />
      </div>
      <motion.main
        id="scroller"
        onScroll={(e) => setScrolled((e.target as HTMLElement).scrollTop > 24)}
        className="no-scrollbar relative h-full overflow-y-auto overscroll-y-contain pb-[calc(var(--tabbar-h)+var(--safe-b)+40px)] md:pb-12"
        animate={{ scale: pages.length ? 0.96 : 1, opacity: pages.length ? 0.5 : 1, x: pages.length ? -40 : 0 }}
        transition={spring.page}
        style={{ transformOrigin: '50% 30%' }}
      >
        <div className="mx-auto w-full max-w-[560px] md:my-6 md:overflow-hidden md:rounded-[36px] md:shadow-[0_30px_80px_rgb(0_0_0/0.6),inset_0_0_0_1px_rgb(255_255_255/0.06)]" style={{ background: 'var(--color-ground)' }}>
          <AnimatePresence mode="wait" initial={false}>
            <motion.div key={tab} initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0 }} transition={{ duration: 0.2, ease: [0.22, 1, 0.36, 1] }}>
              {tab === 'today' && <TodayScreen scrolled={scrolled} />}
              {tab === 'inbox' && <InboxScreen />}
              {tab === 'life' && <LifeScreen />}
              {tab === 'work' && <WorkScreen />}
              {tab === 'mission' && <MissionScreen />}
            </motion.div>
          </AnimatePresence>
        </div>
      </motion.main>
      <SyncPill />
      <TabBar />
      <AnimatePresence>
        {pages.map((p, i) => (
          <PageFor key={`${i}-${p.kind}-${'slot' in p ? p.slot : 'id' in p ? p.id : ''}`} page={p} onBack={pop} />
        ))}
      </AnimatePresence>
      <Sheets />
      <CallLayer />
    </div>
  );
}

function PageFor({ page }: { page: PageT; onBack: () => void }) {
  switch (page.kind) {
    case 'goal':
      return <GoalPage slot={page.slot} />;
    case 'pillar':
      return <PillarPage id={page.id} />;
    case 'area':
      return <AreaPage id={page.id} />;
    case 'nora':
      return <NoraPage />;
  }
}

/** Says "offline" only when it is true, and how many changes wait. Never logs out. */
function SyncPill() {
  const offline = useSession((s) => s.offline);
  const pending = useOutbox((s) => s.pending);
  const show = offline || pending > 2;
  return (
    <AnimatePresence>
      {show && (
        <motion.div initial={{ y: -20, opacity: 0 }} animate={{ y: 0, opacity: 1 }} exit={{ y: -20, opacity: 0 }} className="pointer-events-none fixed inset-x-0 top-[calc(var(--safe-t)+64px)] z-40 flex justify-center">
          <span className="glass flex h-9 items-center gap-2 rounded-full px-4 text-footnote font-medium">
            {offline ? <CloudOff size={15} className="text-needs" /> : <RefreshCw size={15} className="animate-spin text-ink-2" />}
            {offline ? (pending ? `Offline · ${pending} change${pending > 1 ? 's' : ''} saved on this device` : 'Offline · showing the last copy') : `Syncing ${pending} changes`}
          </span>
        </motion.div>
      )}
    </AnimatePresence>
  );
}

function RepairBanner({ onOpen }: { onOpen: () => void }) {
  return (
    <button onClick={onOpen} className="glass fixed inset-x-4 top-[calc(var(--safe-t)+64px)] z-40 mx-auto flex max-w-[420px] items-center gap-3 rounded-[20px] p-3 text-left">
      <CloudOff size={18} className="shrink-0 text-needs" />
      <span className="flex-1 text-footnote">The server no longer knows this device. Your data here is kept. Tap to pair again.</span>
    </button>
  );
}

function Toast() {
  const toast = useUI((s) => s.toast);
  return (
    <div className="pointer-events-none fixed inset-x-0 bottom-[calc(var(--tabbar-h)+var(--safe-b)+28px)] z-[65] flex justify-center">
      <AnimatePresence>
        {toast && (
          <motion.div key={toast.id} initial={{ y: 16, opacity: 0, scale: 0.96 }} animate={{ y: 0, opacity: 1, scale: 1 }} exit={{ y: 8, opacity: 0 }} transition={spring.snappy} className="glass rounded-full px-4 py-2.5 text-subhead font-medium">
            {toast.text}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
