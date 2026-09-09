import type { ReactNode } from "react";

type FormFieldProps = {
  label: string;
  htmlFor?: string;
  children: ReactNode;
  hint?: string;
};

export default function FormField({ label, htmlFor, children, hint }: FormFieldProps) {
  return (
    <label className="form-field" htmlFor={htmlFor}>
      <span className="form-label">{label}</span>
      {children}
      {hint ? <span className="form-hint">{hint}</span> : null}
    </label>
  );
}
