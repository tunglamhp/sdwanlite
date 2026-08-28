import { useState } from "react";
import { useSdwanStore } from "../store";

export default function SettingsPage() {
  const token = useSdwanStore((state) => state.token);
  const setToken = useSdwanStore((state) => state.setToken);
  const [value, setValue] = useState(token);
  const [saved, setSaved] = useState(false);

  const save = () => {
    setToken(value.trim());
    setSaved(true);
  };

  return (
    <div className="page">
      <h1>Settings</h1>
      <section>
        <h2>Controller</h2>
        <label>
          Bootstrap token
          <input
            type="password"
            value={value}
            onChange={(event) => {
              setValue(event.target.value);
              setSaved(false);
            }}
          />
        </label>
        <button type="button" onClick={save}>
          Save
        </button>
        {saved ? <p>Saved.</p> : null}
        <p className="hint">
          Do not paste real production tokens here in shared environments. This value is kept in session state only in this scaffold.
        </p>
      </section>
    </div>
  );
}
