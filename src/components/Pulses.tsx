import { CENTER_DIAMETER, pulseScale } from "../lib/radar";

/**
 * Three rings leaving the centre circle, staggered so one is always on its
 * way out. Only `transform` and `opacity` animate, which keeps this cheap.
 */
export function Pulses({ size }: { size: number }) {
  return (
    <svg
      className="pointer-events-none absolute inset-0 h-full w-full"
      style={{ ["--pulse-scale" as string]: pulseScale(size) }}
      aria-hidden
    >
      {[0, 0.8, 1.6].map((delay) => (
        <circle
          key={delay}
          cx="50%"
          cy="50%"
          r={CENTER_DIAMETER / 2}
          className="pulse"
          style={{ animationDelay: `${delay}s` }}
        />
      ))}
    </svg>
  );
}
