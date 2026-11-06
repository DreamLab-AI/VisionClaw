/**
 * MacroDial — radial dial control for the L1 macro row (Density, Luminosity,
 * Motion, Focus, Atmosphere). design-spec.md §1.1, §5, §6.2.
 *
 * Pointer drag (vertical delta), keyboard arrows (± step), full ARIA slider
 * semantics. Fires onChange continuously during drag (immediate, no
 * transition on the indicator per §5.3) and onCommit once on release/keyup
 * so callers can pulse an echo without spamming it per-tick.
 */

import React, { useCallback, useRef, useState } from 'react';
import type { LucideIcon } from 'lucide-react';
import { Users } from 'lucide-react';
import { cn } from '../../../utils/classNameUtils';
import '../styles/control-center.css';

export interface MacroDialProps {
  id: string;
  label: string;
  icon: LucideIcon;
  /** Normalised dial position, 0..1. */
  value: number;
  onChange: (t: number) => void;
  /** Fired once on drag release / keyup — not on every tick. */
  onCommit?: (t: number) => void;
  step?: number;
  disabled?: boolean;
  size?: number;
  accentColor?: string;
  /** Plain-language readout of what the dial currently sets (announced as aria-valuetext). */
  valueText?: string;
  /** The dial writes the shared layout document: show the "affects everyone" marker. */
  shared?: boolean;
}

/** Text appended to a shared dial's tooltip and accessible value (ADR-2189 §2). */
export const SHARED_DIAL_TEXT = 'Changes the layout for everyone';

const CLAMP = (n: number) => Math.min(1, Math.max(0, n));
// Drag distance (px) that spans the full 0..1 range.
const DRAG_RANGE_PX = 140;
// Arc sweep: 270° gauge, starting at -135° (pointing up-left) through +135°.
const ARC_START_DEG = -135;
const ARC_SWEEP_DEG = 270;

export const MacroDial: React.FC<MacroDialProps> = ({
  id,
  label,
  icon: Icon,
  value,
  onChange,
  onCommit,
  step = 0.05,
  disabled = false,
  size = 48,
  accentColor,
  valueText,
  shared = false,
}) => {
  const pct = Math.round(CLAMP(value) * 100);
  const ariaValueText = `${pct}%${valueText ? ` — ${valueText}` : ''}${shared ? `. ${SHARED_DIAL_TEXT}` : ''}`;
  const title = `${label}: ${valueText ?? `${pct}%`}${shared ? `\n${SHARED_DIAL_TEXT}` : ''}`;
  const [dragging, setDragging] = useState(false);
  const dragStartRef = useRef<{ y: number; startValue: number } | null>(null);

  const radius = size / 2 - 4;
  const circumference = (ARC_SWEEP_DEG / 360) * 2 * Math.PI * radius;
  const dashOffset = circumference * (1 - CLAMP(value));

  const handlePointerDown = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      if (disabled) return;
      (e.target as Element).setPointerCapture?.(e.pointerId);
      dragStartRef.current = { y: e.clientY, startValue: value };
      setDragging(true);
    },
    [disabled, value]
  );

  const handlePointerMove = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      if (!dragging || !dragStartRef.current) return;
      const dy = dragStartRef.current.y - e.clientY;
      const next = CLAMP(dragStartRef.current.startValue + dy / DRAG_RANGE_PX);
      onChange(next);
    },
    [dragging, onChange]
  );

  const endDrag = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      if (!dragging) return;
      (e.target as Element).releasePointerCapture?.(e.pointerId);
      setDragging(false);
      dragStartRef.current = null;
      onCommit?.(value);
    },
    [dragging, onCommit, value]
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLDivElement>) => {
      if (disabled) return;
      let next: number | null = null;
      switch (e.key) {
        case 'ArrowUp':
        case 'ArrowRight':
          next = CLAMP(value + step);
          break;
        case 'ArrowDown':
        case 'ArrowLeft':
          next = CLAMP(value - step);
          break;
        case 'Home':
          next = 0;
          break;
        case 'End':
          next = 1;
          break;
        default:
          return;
      }
      e.preventDefault();
      onChange(next);
      onCommit?.(next);
    },
    [disabled, onChange, onCommit, step, value]
  );

  return (
    <div
      className="flex flex-col items-center gap-1"
      style={{ width: size + 16 }}
    >
      <div
        id={id}
        data-testid={`macro-${id}`}
        data-dragging={dragging}
        role="slider"
        tabIndex={disabled ? -1 : 0}
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={1}
        aria-valuenow={Number(value.toFixed(3))}
        aria-valuetext={ariaValueText}
        title={title}
        data-scope={shared ? 'shared' : 'browser'}
        aria-disabled={disabled}
        aria-orientation="vertical"
        className={cn('cc-macro-dial relative', disabled && 'opacity-50 pointer-events-none')}
        style={{ width: size, height: size }}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
        onKeyDown={handleKeyDown}
      >
        <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} className="pointer-events-none">
          <circle
            cx={size / 2}
            cy={size / 2}
            r={radius}
            fill="none"
            stroke="hsl(var(--secondary))"
            strokeWidth={3}
            strokeDasharray={`${circumference} ${circumference}`}
            strokeDashoffset={0}
            strokeLinecap="round"
            transform={`rotate(${ARC_START_DEG} ${size / 2} ${size / 2})`}
            opacity={0.35}
          />
          <circle
            cx={size / 2}
            cy={size / 2}
            r={radius}
            fill="none"
            stroke={accentColor ?? 'hsl(var(--primary))'}
            strokeWidth={3}
            strokeDasharray={`${circumference} ${circumference}`}
            strokeDashoffset={dashOffset}
            strokeLinecap="round"
            transform={`rotate(${ARC_START_DEG} ${size / 2} ${size / 2})`}
            className="cc-macro-dial-indicator"
          />
        </svg>
        <Icon
          size={size * 0.32}
          className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 text-foreground pointer-events-none"
        />
        {shared && (
          <span
            aria-hidden="true"
            data-testid={`macro-${id}-shared`}
            className="absolute -top-0.5 -right-0.5 flex items-center justify-center h-4 w-4 rounded-full bg-amber-500/90 text-black pointer-events-none"
          >
            <Users size={9} />
          </span>
        )}
      </div>
      <span className="cc-field-label select-none">{label}</span>
    </div>
  );
};

MacroDial.displayName = 'MacroDial';

export default MacroDial;
