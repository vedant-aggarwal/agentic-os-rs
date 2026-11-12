// A progress ring that lights the surface under it in its own colour.
import { motion } from 'motion/react';
import { spring } from './motion';

interface Props {
  size: number;
  stroke?: number;
  value: number; // 0..1
  color: string; // css colour or var()
  color2?: string; // gradient end
  track?: string;
  glow?: boolean;
  children?: React.ReactNode;
  className?: string;
}

let uid = 0;

export function Ring({ size, stroke = 4, value, color, color2, track = 'rgb(255 255 255 / 0.1)', glow = true, children, className = '' }: Props) {
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  const v = Math.max(0, Math.min(1, value || 0));
  const id = `rg${(uid = (uid + 1) % 1e6)}`;
  return (
    <div className={`relative inline-grid place-items-center ${className}`} style={{ width: size, height: size }}>
      <svg width={size} height={size} className="absolute inset-0 -rotate-90 overflow-visible">
        <defs>
          <linearGradient id={id} x1="0" y1="0" x2="1" y2="1">
            <stop offset="0%" stopColor={color} />
            <stop offset="100%" stopColor={color2 || color} />
          </linearGradient>
          {glow && (
            <filter id={id + 'b'} x="-50%" y="-50%" width="200%" height="200%">
              <feGaussianBlur stdDeviation={stroke * 0.9} />
            </filter>
          )}
        </defs>
        <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke={track} strokeWidth={stroke} />
        {glow && v > 0.001 && (
          <motion.circle
            cx={size / 2}
            cy={size / 2 + stroke * 0.25}
            r={r}
            fill="none"
            stroke={`url(#${id})`}
            strokeWidth={stroke}
            strokeLinecap="round"
            strokeDasharray={c}
            filter={`url(#${id}b)`}
            opacity={0.6}
            initial={{ strokeDashoffset: c }}
            animate={{ strokeDashoffset: c * (1 - v) }}
            transition={spring.page}
          />
        )}
        {v > 0.001 && (
          <motion.circle
            cx={size / 2}
            cy={size / 2}
            r={r}
            fill="none"
            stroke={`url(#${id})`}
            strokeWidth={stroke}
            strokeLinecap="round"
            strokeDasharray={c}
            initial={{ strokeDashoffset: c }}
            animate={{ strokeDashoffset: c * (1 - v) }}
            transition={spring.page}
          />
        )}
      </svg>
      <div className="relative">{children}</div>
    </div>
  );
}
