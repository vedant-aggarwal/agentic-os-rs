// window.aosCall / window.AosOrb are installed by engine.js (side-effect import).
export {};
declare global {
  interface Window {
    aosCall?: {
      state: { phase: string; callId: string | null; startedAt: number; muted: boolean; who: string };
      start(o: { base: string; token: string; reason?: string | null; ringId?: number | null }): Promise<void>;
      end(why?: string): Promise<void>;
      setMuted(m: boolean): void;
      toggleSpeaker(): Promise<void>;
      mountOrb(canvas: HTMLCanvasElement): boolean;
      unmountOrb(): void;
      setOrb(o: { color?: string; thinking?: boolean; muted?: boolean; stress?: boolean }): void;
    };
    AosOrb?: { mount(canvas: HTMLCanvasElement): { set(o: Record<string, unknown>): void; level(v: number): void; destroy(): void } | null };
  }
}
