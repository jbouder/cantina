import type { SelectHTMLAttributes } from "react";
import { Field } from "./Field";

type Props = SelectHTMLAttributes<HTMLSelectElement> & {
  label: string;
  hint?: string;
};

/** A labeled drop-down, as a form row. */
export function Select({ label, hint, id, className = "", children, ...rest }: Props) {
  return (
    <Field label={label} hint={hint} htmlFor={id}>
      <span className="relative block w-full min-w-0">
        <select
          id={id}
          className={`surface-field min-h-8 w-full min-w-0 appearance-none truncate py-0 pr-8 pl-2 ${className}`}
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
    </Field>
  );
}
