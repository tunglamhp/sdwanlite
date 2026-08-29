import { useEffect, useMemo } from "react";
import { useSdwanStore } from "../store";
import { formatUptime } from "../format";

export default function Dashboard() {
  const deviceSummaries = useSdwanStore((state) => state.deviceSummaries);
  const loadDevices = useSdwanStore((state) => state.loadDevices);
  const devicesLoading = useSdwanStore((state) => state.devicesLoading);
  const devicesError = useSdwanStore((state) => state.devicesError);
  const telemetryByDeviceId = useSdwanStore((state) => state.telemetryByDeviceId);

  useEffect(() => {
    loadDevices().catch(() => undefined);
  }, [loadDevices]);

  const signals = useMemo(() => {
    let uptimeSecs = 0;
    let linkDown = 0;
    let degraded = 0;
    for (const frame of Object.values(telemetryByDeviceId)) {
      uptimeSecs += frame.uptime_secs;
      for (const flag of frame.flags) {
        if (flag.kind === "link_down") linkDown += 1;
        if (flag.kind === "degraded") degraded += 1;
      }
    }
    return { uptimeSecs, linkDown, degraded };
  }, [telemetryByDeviceId]);

  const devices = deviceSummaries.length;

  return (
    <div className="page">
      <h1>Dashboard</h1>
      {devicesError ? <div className="alert">{devicesError}</div> : null}
      <div className="stats">
        <div className="card">
          <div className="card-label">Devices</div>
          <div className={`card-value ${devices > 0 ? "ok" : ""}`}>{devices}</div>
        </div>
        <div className="card">
          <div className="card-label">Links down</div>
          <div className={`card-value ${signals.linkDown > 0 ? "err" : ""}`}>{signals.linkDown}</div>
        </div>
        <div className="card">
          <div className="card-label">Degraded</div>
          <div className={`card-value ${signals.degraded > 0 ? "warn" : ""}`}>{signals.degraded}</div>
        </div>
        <div className="card">
          <div className="card-label">Total uptime</div>
          <div className="card-value">{formatUptime(signals.uptimeSecs)}</div>
        </div>
      </div>
      <h2>Devices</h2>
      {devicesLoading ? (
        <p className="empty">Loading devices…</p>
      ) : deviceSummaries.length === 0 ? (
        <p className="empty">No devices registered.</p>
      ) : (
        <table className="data">
          <thead>
            <tr>
              <th>Hostname</th>
              <th>Site</th>
              <th>Device ID</th>
            </tr>
          </thead>
          <tbody>
            {deviceSummaries.map((device) => (
              <tr key={device.device_id}>
                <td>{device.hostname}</td>
                <td>{device.site_id || "—"}</td>
                <td>{device.device_id}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
