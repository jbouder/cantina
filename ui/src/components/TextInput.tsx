import type { InputHTMLAttributes } from "react";
import { Field } from "./Field";

type Props = InputHTMLAttributes<HTMLInputElement> & {
  label: string;
  hint?: string;
};

/** A labeled text field, as a form row. */
export function TextInput({ label, hint, id, className = "", ...rest }: Props) {
  return (
    <Field label={label} hint={hint} htmlFor={id}>
      <input
        id={id}
        type="text"
        className={`surface-field min-h-8 w-full min-w-0 px-2 py-0 ${className}`}
        {...rest}
      />
    </Field>
  );
}
