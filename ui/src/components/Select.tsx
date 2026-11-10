import type { SelectHTMLAttributes } from "react";

type Props = SelectHTMLAttributes<HTMLSelectElement> & {
  label: string;
  hint?: string;
};

/** A labelled drop-down in the skin's display material. */
export function Select({ label, hint, id, className = "", children, ...rest }: Props) {
  return (
    <div className="grid grid-cols-[1fr_auto] items-center gap-3">
      <label htmlFor={id} className="text-body">
        {label}
        {hint ? <small className="block text-ink-dim text-label">{hint}</small> : null}
      </label>
      <span className="relative">
        <select
          id={id}
          className={`surface-field min-h-8 min-w-48 max-w-64 appearance-none truncate py-0 pr-8 pl-2 ${className}`}
          {...rest}
        >
          {children}
        </select>
        <svg
          aria-hidden="true"
          viewBox="0 0 16 16"
          className="pointer-events-none absolute top-1/2 right-2 size-3.5 -translate-y-1/2 fill-field-fg"
        >
          <path d="M2 5l6 6 6-6z" />
        </svg>
      </span>
    </div>
  );
}
