import { CENTER_DIAMETER, PULSE_SECONDS, pulseScale } from "../lib/radar";

/**
 * Three rings leaving the centre circle, staggered so one is always on its
 * way out. Only `transform` and `opacity` animate, which keeps this cheap.
 */
export function Pulses({ size }: { size: number }) {
  return (
    <svg
      className="pointer-events-none absolute inset-0 h-full w-full"
      style={{
        ["--pulse-scale" as string]: pulseScale(size),
        ["--pulse-duration" as string]: `${PULSE_SECONDS}s`,
      }}
      aria-hidden
    >
      {[0, 1, 2].map((step) => (
        <circle
          key={step}
          cx="50%"
          cy="50%"
          r={CENTER_DIAMETER / 2}
          className="pulse"
          style={{ animationDelay: `${(step * PULSE_SECONDS) / 3}s` }}
        />
      ))}
    </svg>
  );
}
