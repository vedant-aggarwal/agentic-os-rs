// Core components: buttons, chips, segmented control, bars, section headers, rows, empty states.
import { motion } from 'motion/react';
import { forwardRef, useId } from 'react';
import { ChevronRight, type LucideIcon } from 'lucide-react';
import { spring } from './motion';

/* ═══ buttons ═══ */

type BtnProps = React.ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: 'primary' | 'secondary' | 'ghost' | 'tint';
  tint?: string;
  size?: 'md' | 'lg';
  icon?: LucideIcon;
};

export const Button = forwardRef<HTMLButtonElement, BtnProps>(function Button(
  { variant = 'secondary', tint, size = 'md', icon: Icon, className = '', children, style, ...rest },
  ref,
) {
  const base = 'press inline-flex items-center justify-center gap-2 rounded-full font-semibold disabled:opacity-40';
  const sz = size === 'lg' ? 'h-[52px] px-6 text-headline' : 'h-11 px-5 text-subhead';
  const look =
    variant === 'primary'
      ? 'bg-ink text-ground'
      : variant === 'ghost'
        ? 'text-ink-2 hover:text-ink'
        : variant === 'tint'
          ? ''
          : 'bg-fill-2 text-ink hover:bg-fill-3';
  const tintStyle =
    variant === 'tint' && tint
      ? { background: `color-mix(in oklab, ${tint} 18%, transparent)`, color: `color-mix(in oklab, ${tint} 70%, white)` }
      : {};
  return (
    <button ref={ref} className={`${base} ${sz} ${look} ${className}`} style={{ ...tintStyle, ...style }} {...rest}>
      {Icon && <Icon size={size === 'lg' ? 20 : 18} strokeWidth={2.2} />}
      {children}
    </button>
  );
});

/** Round glass button for the top bar: 44 px target. */
export function IconButton({
  icon: Icon,
  label,
  onClick,
  className = '',
  size = 44,
}: {
  icon: LucideIcon;
  label: string;
  onClick?: () => void;
  className?: string;
  size?: number;
}) {
  return (
    <button aria-label={label} onClick={onClick} className={`glass press grid shrink-0 place-items-center rounded-full ${className}`} style={{ width: size, height: size }}>
      <Icon size={20} strokeWidth={2} />
    </button>
  );
}

/* ═══ chips ═══ */

export function Chip({
  children,
  active,
  onClick,
  tint,
  icon: Icon,
}: {
  children: React.ReactNode;
  active?: boolean;
  onClick?: () => void;
  tint?: string;
  icon?: LucideIcon;
}) {
  return (
    <button
      onClick={onClick}
      className={`press inline-flex h-9 shrink-0 items-center gap-1.5 rounded-full px-3.5 text-subhead font-medium ${active ? 'text-ink' : 'text-ink-2'}`}
      style={{
        background: active ? (tint ? `color-mix(in oklab, ${tint} 26%, rgb(255 255 255 / 0.06))` : 'rgb(255 255 255 / 0.17)') : 'var(--color-fill-2)',
        boxShadow: active ? 'inset 0 1px 0 rgb(255 255 255 / 0.18)' : undefined,
      }}
    >
      {Icon && <Icon size={15} strokeWidth={2.2} />}
      {children}
    </button>
  );
}

/* ═══ segmented control with a sliding lens ═══ */

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  className = '',
}: {
  value: T;
  options: { value: T; label: string; count?: number }[];
  onChange: (v: T) => void;
  className?: string;
}) {
  const id = useId();
  return (
    <div className={`flex h-10 rounded-full bg-fill-2 p-[3px] ${className}`} role="tablist">
      {options.map((o) => (
        <button
          key={o.value}
          role="tab"
          aria-selected={o.value === value}
          onClick={() => onChange(o.value)}
          className={`relative flex-1 rounded-full text-footnote font-semibold transition-colors ${o.value === value ? 'text-ink' : 'text-ink-2'}`}
        >
          {o.value === value && (
            <motion.span
              layoutId={id}
              className="absolute inset-0 rounded-full"
              style={{ background: 'rgb(255 255 255 / 0.15)', boxShadow: 'inset 0 1px 0 rgb(255 255 255 / 0.2)' }}
              transition={spring.snappy}
            />
          )}
          <span className="relative">
            {o.label}
            {o.count ? <span className="ml-1 text-ink-3 tnum">{o.count}</span> : null}
          </span>
        </button>
      ))}
    </div>
  );
}

/* ═══ bars ═══ */

export function Bar({ value, color, height = 4, glow = true, className = '' }: { value: number; color: string; height?: number; glow?: boolean; className?: string }) {
  const v = Math.max(0, Math.min(1, value || 0));
  return (
    <div className={`relative overflow-visible rounded-full ${className}`} style={{ height, background: 'rgb(255 255 255 / 0.08)' }}>
      <motion.div
        className="absolute inset-y-0 left-0 rounded-full"
        style={{ background: color, boxShadow: glow && v > 0 ? `0 0 ${height * 3}px color-mix(in oklab, ${color} 70%, transparent)` : undefined }}
        initial={{ width: 0 }}
        animate={{ width: `${v * 100}%` }}
        transition={spring.page}
      />
    </div>
  );
}

/* ═══ section header ═══ */

export function Section({
  title,
  meta,
  action,
  onAction,
  children,
  className = '',
}: {
  title: React.ReactNode;
  meta?: React.ReactNode;
  action?: string;
  onAction?: () => void;
  children?: React.ReactNode;
  className?: string;
}) {
  return (
    <section className={`mt-8 ${className}`}>
      <div className="mb-3 flex items-baseline gap-2 px-gutter">
        <h2 className="text-title3 font-bold">{title}</h2>
        {meta != null && <span className="text-subhead text-ink-2 tnum">{meta}</span>}
        <span className="flex-1" />
        {action && (
          <button onClick={onAction} className="press -my-2 inline-flex h-11 items-center gap-0.5 text-subhead font-medium text-ink-2">
            {action}
            <ChevronRight size={16} strokeWidth={2.2} />
          </button>
        )}
      </div>
      {children}
    </section>
  );
}

/* ═══ rows and cards ═══ */

export function Card({ children, className = '', style }: { children: React.ReactNode; className?: string; style?: React.CSSProperties }) {
  return (
    <div className={`stone mx-gutter rounded-xl ${className}`} style={style}>
      {children}
    </div>
  );
}

export function Row({
  icon: Icon,
  tint,
  title,
  subtitle,
  trailing,
  onClick,
  last,
}: {
  icon?: LucideIcon;
  tint?: string;
  title: React.ReactNode;
  subtitle?: React.ReactNode;
  trailing?: React.ReactNode;
  onClick?: () => void;
  last?: boolean;
}) {
  const Tag = onClick ? 'button' : 'div';
  return (
    <Tag onClick={onClick} className={`flex min-h-[56px] w-full items-center gap-3 px-4 py-2.5 text-left ${onClick ? 'active:bg-fill-1' : ''}`}>
      {Icon && (
        <span
          className="grid size-9 shrink-0 place-items-center rounded-[11px]"
          style={{ background: tint ? `color-mix(in oklab, ${tint} 20%, transparent)` : 'var(--color-fill-2)', color: tint ? `color-mix(in oklab, ${tint} 80%, white)` : undefined }}
        >
          <Icon size={18} strokeWidth={2.1} />
        </span>
      )}
      <span className={`min-w-0 flex-1 self-stretch py-1 ${last ? '' : 'hair'}`} style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
        <span className="min-w-0 flex-1">
          <span className="block truncate text-body">{title}</span>
          {subtitle && <span className="mt-0.5 block truncate text-footnote text-ink-2">{subtitle}</span>}
        </span>
        {trailing}
        {onClick && !trailing && <ChevronRight size={18} className="shrink-0 text-ink-3" />}
      </span>
    </Tag>
  );
}

export function Empty({ icon: Icon, title, body, action, onAction }: { icon: LucideIcon; title: string; body?: string; action?: string; onAction?: () => void }) {
  return (
    <div className="flex flex-col items-center px-8 py-8 text-center">
      <span className="grid size-12 place-items-center rounded-full bg-fill-2 text-ink-2">
        <Icon size={22} strokeWidth={2} />
      </span>
      <p className="mt-3 text-headline font-semibold">{title}</p>
      {body && <p className="mt-1 max-w-[30ch] text-subhead text-ink-2">{body}</p>}
      {action && (
        <Button className="mt-4" onClick={onAction}>
          {action}
        </Button>
      )}
    </div>
  );
}

/** A big tabular figure with a small unit, like the Opal stats row. */
export function Stat({ label, value, unit, color, onClick }: { label: string; value: React.ReactNode; unit?: string; color?: string; onClick?: () => void }) {
  return (
    <div onClick={onClick} role={onClick ? 'button' : undefined} tabIndex={onClick ? 0 : undefined} className={`flex min-w-0 flex-1 flex-col items-center ${onClick ? 'press cursor-pointer' : ''}`}>
      <span className="eyebrow">{label}</span>
      <span className="fig mt-1 text-[30px] leading-[34px]" style={{ color: color || 'var(--color-ink)' }}>
        {value}
        {unit && <span className="ml-0.5 text-[17px] font-semibold text-ink-2">{unit}</span>}
      </span>
    </div>
  );
}
