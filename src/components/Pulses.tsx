import { CENTER_DIAMETER, PULSE_SECONDS, pulseScale } from "../lib/radar";

/**
 * Three rings leaving the centre circle, staggered so one is always on its
 * way out. Only `transform` and `opacity` animate, which keeps this cheap.
 *
 * Plain elements rather than SVG circles. The SVG version scaled about a
 * percentage origin, which the Windows webview resolved differently, and the
 * rings either sat still in a corner or never showed up at all. A scaled box
 * has the same origin everywhere, and its border thickens as it grows, which
 * is what a wave front does anyway.
 */
export function Pulses({ size }: { size: number }) {
  return (
    <div className="pointer-events-none absolute inset-0 overflow-hidden" aria-hidden>
      {[0, 1, 2].map((step) => (
        <span
          key={step}
          className="pulse"
          style={{
            width: CENTER_DIAMETER,
            height: CENTER_DIAMETER,
            ["--pulse-scale" as string]: pulseScale(size),
            animationDuration: `${PULSE_SECONDS}s`,
            animationDelay: `${(step * PULSE_SECONDS) / 3}s`,
          }}
        />
      ))}
    </div>
  );
}
