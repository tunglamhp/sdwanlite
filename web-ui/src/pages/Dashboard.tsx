import { useEffect, useState } from "react";
import { fetchStatus, fetchSignals } from "../api";

export default function Dashboard() {
  const [status, setStatus] = useState<unknown>(null);
  const [signals, setSignals] = useState<unknown>(null);

  useEffect(() => {
    fetchStatus().then(setStatus).catch(console.error);
    fetchSignals().then(setSignals).catch(console.error);
  }, []);

  return (
    <div className="page">
      <h1>Dashboard</h1>
      <pre>{JSON.stringify({ status, signals }, null, 2)}</pre>
    </div>
  );
}
