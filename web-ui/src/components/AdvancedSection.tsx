import { useState } from "react";

export default function AdvancedSection({ title, children }: { title: string; children: React.ReactNode }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="advanced-section">
      <button type="button" className="advanced-toggle" onClick={() => setOpen((value) => !value)}>
        {title}
        <span aria-hidden>{open ? "▼" : "▶"}</span>
      </button>
      {open ? <div className="advanced-body">{children}</div> : null}
    </div>
  );
}
