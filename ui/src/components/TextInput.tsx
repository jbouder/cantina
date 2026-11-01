import type { InputHTMLAttributes } from "react";

type Props = InputHTMLAttributes<HTMLInputElement> & {
  label: string;
  hint?: string;
};

/** A labelled text field in the skin's display material. */
export function TextInput({ label, hint, id, className = "", ...rest }: Props) {
  return (
    <div className="grid grid-cols-[1fr_auto] items-center gap-3">
      <label htmlFor={id} className="text-sm">
        {label}
        {hint ? <small className="block text-ink-dim text-xs">{hint}</small> : null}
      </label>
      <input
        id={id}
        type="text"
        className={`surface-lcd min-h-8 min-w-48 max-w-64 px-2 py-0 font-lcd text-base ${className}`}
        {...rest}
      />
    </div>
  );
}
