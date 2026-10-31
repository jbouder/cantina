type Props = {
  on: boolean;
  /** Recording lights are red in every skin. */
  tone?: "power" | "record";
  label: string;
};

/** An indicator light with its state read out to assistive tech. */
export function Led({ on, tone = "power", label }: Props) {
  const color = on ? (tone === "record" ? "bg-led-rec" : "bg-led-on") : "bg-led-off";
  return (
    <span
      role="img"
      aria-label={`${label}: ${on ? "on" : "off"}`}
      className={`inline-block size-3 rounded-full ${color} ${on ? "shadow-[0_0_6px_currentColor]" : ""} ${on ? (tone === "record" ? "text-led-rec" : "text-led-on") : ""}`}
    />
  );
}
