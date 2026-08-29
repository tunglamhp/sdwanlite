import { useState } from "react";
import { useSdwanStore } from "../store";

const TOKEN_KEY = "sdwan.token";

function readSessionToken(): string {
  try {
    return sessionStorage.getItem(TOKEN_KEY) ?? "";
  } catch {
    return "";
  }
}

export default function SettingsPage() {
  const token = useSdwanStore((state) => state.token);
  const setToken = useSdwanStore((state) => state.setToken);
  const [value, setValue] = useState(token || readSessionToken());
  const [saved, setSaved] = useState(false);
  const save = () => {
    const trimmed = value.trim();
    setToken(trimmed);
    try {
      if (trimmed) {
        sessionStorage.setItem(TOKEN_KEY, trimmed);
      } else {
        sessionStorage.removeItem(TOKEN_KEY);
      }
    } catch {
      // storage unavailable — token lives in memory for this session only
    }
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
            autoComplete="off"
            value={value}
            onChange={(event) => {
              setValue(event.target.value);
              setSaved(false);
            }}
          />
        </label>{" "}
        <button type="button" className="btn" onClick={save}>
          Save
        </button>
        {saved ? <p>Saved.</p> : null}
        <p className="hint">This value is kept in this browser session only.</p>
      </section>
    </div>
  );
}
