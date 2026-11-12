import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClientProvider } from '@tanstack/react-query';
import { MotionConfig } from 'motion/react';
import { App } from './App';
import { qc } from './lib/queries';
import { startOutbox } from './lib/outbox';
import { startLive, startNoraLive } from './lib/live';
import { useUI } from './lib/ui';
import './styles/index.css';

// `?inset=47,34` simulates the notch and home indicator in a desktop browser (top,bottom px).
const inset = new URLSearchParams(location.search).get('inset');
if (inset) {
  const [t, b] = inset.split(',').map((n) => `${Number(n) || 0}px`);
  document.documentElement.style.setProperty('--safe-t', t);
  document.documentElement.style.setProperty('--safe-b', b ?? '0px');
}

// test hook for the screenshot scripts (dev builds, ?demo and ?test only)
const q = new URLSearchParams(location.search);
if (import.meta.env.DEV || q.has('demo') || q.has('test')) (window as unknown as { __aos: unknown }).__aos = { ui: useUI };

startOutbox();
startLive();
startNoraLive();

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QueryClientProvider client={qc}>
      <MotionConfig reducedMotion="user">
        <App />
      </MotionConfig>
    </QueryClientProvider>
  </StrictMode>,
);
