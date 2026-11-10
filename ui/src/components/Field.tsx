import type { ReactNode } from "react";

type FieldProps = {
  label: string;
  /** A line under the label: what the setting does, or what it is now. */
  hint?: ReactNode | undefined;
  /** The id of the control, so the label names it. */
  htmlFor?: string | undefined;
  /** The id to give the label, for a control named with aria-labelledby. */
  labelId?: string | undefined;
  children: ReactNode;
};

/**
 * One row of a form, the same in every sheet: the label with its helper
 * text under it at the left, the control at the right. The columns are
 * fixed fractions, so rows line up down a sheet and helper text wraps
 * the same way in each.
 */
export function Field({ label, hint, htmlFor, labelId, children }: FieldProps) {
  return (
    <div className="grid grid-cols-[minmax(0,5fr)_minmax(0,6fr)] items-center gap-x-4">
      <div className="min-w-0">
        <label id={labelId} htmlFor={htmlFor} className="block text-body">
          {label}
        </label>
        {hint ? <small className="block text-ink-dim text-label">{hint}</small> : null}
      </div>
      <div className="flex min-w-0 items-center justify-end">{children}</div>
    </div>
  );
}

type ToggleProps = {
  id: string;
  label: string;
  hint?: ReactNode | undefined;
  checked: boolean;
  disabled?: boolean | undefined;
  onChange: (checked: boolean) => void;
};

/**
 * A setting that is on or off, as a form row with a switch. A key that
 * does something stays a `Button`; a setting is never drawn as one.
 */
export function Toggle({ id, label, hint, checked, disabled, onChange }: ToggleProps) {
  return (
    <Field label={label} hint={hint} labelId={`${id}-label`}>
      <button
        id={id}
        type="button"
        role="switch"
        aria-checked={checked}
        aria-labelledby={`${id}-label`}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className="switch"
      />
    </Field>
  );
}
