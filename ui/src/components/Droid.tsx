/** What the droid is doing, which its face shows. */
export type DroidState = "idle" | "thinking" | "speaking" | "error";

type Props = {
  state?: DroidState;
  /** Pixels across; it is square. */
  size?: number;
  className?: string;
};

/**
 * Tapster, Cantina's house droid: a domed service unit with one lens in a
 * visor, a speaker grille, an antenna light and a bow tie. Its body is
 * drawn in the current text color, so it reads in every skin and inside a
 * button; the visor, lens and lights are the skin's display and LED
 * colors. Idle, it blinks; thinking, its lens scans; speaking, its
 * grille moves; on an error its lens and light go red. The motion stops
 * under the system's Reduce Motion (see `.droid-*` in styles.css).
 */
export function Droid({ state = "idle", size = 48, className = "" }: Props) {
  const fault = state === "error";
  return (
    <svg
      aria-hidden="true"
      viewBox="0 0 64 64"
      width={size}
      height={size}
      className={`droid droid-${state} shrink-0 ${className}`}
    >
      {/* Antenna and its light */}
      <rect x="31" y="5" width="2" height="10" className="fill-current opacity-70" />
      <circle
        cx="32"
        cy="5"
        r="3"
        className={`droid-light ${fault ? "fill-led-rec" : "fill-led-on"}`}
      />
      {/* Side bolts */}
      <circle cx="11" cy="30" r="3.5" className="fill-current opacity-70" />
      <circle cx="53" cy="30" r="3.5" className="fill-current opacity-70" />
      {/* Dome */}
      <path d="M12 44V30a20 16 0 0 1 40 0v14z" className="fill-current" />
      <path
        d="M17 26a15 11 0 0 1 12-8"
        className="fill-none stroke-lcd-bg opacity-30"
        strokeWidth="2"
      />
      {/* Visor and lens */}
      <rect x="16" y="22" width="32" height="12" rx="6" className="fill-lcd-bg" />
      <g className="droid-eye">
        <circle cx="32" cy="28" r="4.5" className={fault ? "fill-led-rec" : "fill-lcd-fg"} />
        <circle cx="33.5" cy="26.5" r="1.2" className="fill-lcd-bg opacity-60" />
      </g>
      {/* Speaker grille */}
      <g className="droid-grille fill-lcd-bg">
        <rect x="23" y="37" width="2" height="5" rx="1" />
        <rect x="27" y="37" width="2" height="5" rx="1" />
        <rect x="31" y="37" width="2" height="5" rx="1" />
        <rect x="35" y="37" width="2" height="5" rx="1" />
        <rect x="39" y="37" width="2" height="5" rx="1" />
      </g>
      {/* Collar, shoulders and bow tie */}
      <rect x="22" y="44" width="20" height="4" className="fill-current opacity-70" />
      <path d="M10 62l6-14h32l6 14z" className="fill-current opacity-45" />
      <path d="M26 46l6 3-6 3zM38 46l-6 3 6 3z" className="fill-led-rec" />
      <circle cx="32" cy="49" r="1.5" className="fill-led-rec" />
    </svg>
  );
}
