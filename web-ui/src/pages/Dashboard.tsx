import { useEffect, useMemo, useState } from "react";
import {
  Bar,
  BarChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { useSdwanStore } from "../store";
import { formatUptime } from "../format";
import { parseJson, request } from "../api";

export interface LbPool {
  name: string;
  algorithm: string;
  active_conns: number;
  rejected_conns: number;
  backends: Array<{
    addr: string;
    healthy: boolean;
    active_conns: number;
    total_conns: number;
  }>;
}

export interface LbSnapshot {
  tcp: LbPool[];
  http: Array<{
    name: string;
    routes: Array<{
      host: string;
      path_prefix: string;
      algorithm: string;
      backends: number;
    }>;
  }>;
}

export default function Dashboard() {
  const deviceSummaries = useSdwanStore((state) => state.deviceSummaries);
  const loadDevices = useSdwanStore((state) => state.loadDevices);
  const loadTelemetry = useSdwanStore((state) => state.loadTelemetry);
  const loadAlerts = useSdwanStore((state) => state.loadAlerts);
  const devicesLoading = useSdwanStore((state) => state.devicesLoading);
  const devicesError = useSdwanStore((state) => state.devicesError);
  const telemetryByDeviceId = useSdwanStore((state) => state.telemetryByDeviceId);
  const alerts = useSdwanStore((state) => state.alerts);

  const [lb, setLb] = useState<LbSnapshot | null>(null);
  const [lbError, setLbError] = useState<string | null>(null);

  useEffect(() => {
    loadDevices().catch(() => undefined);
    loadTelemetry().catch(() => undefined);
    loadAlerts().catch(() => undefined);
    request("/api/lb")
      .then((r): Promise<LbSnapshot> => parseJson<LbSnapshot>(r))
      .then((lb) => setLb(lb))
      .catch((e: unknown) => setLbError(e instanceof Error ? e.message : "failed to load lb"));
  }, [loadDevices, loadTelemetry, loadAlerts]);

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

  const chartData = useMemo(
    () =>
      Object.values(telemetryByDeviceId).map((frame) => {
        const tx = frame.links.reduce((sum, link) => sum + link.tx_bytes, 0);
        const rx = frame.links.reduce((sum, link) => sum + link.rx_bytes, 0);
        const hostname =
          deviceSummaries.find((d) => d.device_id === frame.device_id)?.hostname ?? frame.device_id.slice(0, 8);
        return { name: hostname, TX: tx, RX: rx };
      }),
    [telemetryByDeviceId, deviceSummaries],
  );

  const devices = deviceSummaries.length;

  const hasFailoverPool =
    !!lb &&
    (lb.tcp.some((pool) => pool.algorithm === "failover") ||
      lb.http.some((pool) => pool.routes.some((route) => route.algorithm === "failover")));

  const unhealthyCount = useMemo(() => {
    if (!lb) return 0;
    let count = 0;
    for (const pool of lb.tcp) {
      for (const backend of pool.backends) {
        if (!backend.healthy) count += 1;
      }
    }
    return count;
  }, [lb]);

  return (
    <div className="page">
      <h1>Dashboard</h1>
      {devicesError ? <div className="alert">{devicesError}</div> : null}
      {lbError ? <div className="alert">{lbError}</div> : null}
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
        {hasFailoverPool ? (
          <div className="card">
            <div className="card-label">Unhealthy backends</div>
            <div className={`card-value ${unhealthyCount > 0 ? "err" : "ok"}`}>{unhealthyCount}</div>
          </div>
        ) : null}
      </div>

      {chartData.length > 0 ? (
        <div className="detail">
          <h2>Traffic (bytes)</h2>
          <ResponsiveContainer width="100%" height={200}>
            <BarChart data={chartData}>
              <CartesianGrid strokeDasharray="3 3" />
              <XAxis dataKey="name" fontSize={11} />
              <YAxis fontSize={11} />
              <Tooltip />
              <Bar dataKey="TX" fill="#2563eb" />
              <Bar dataKey="RX" fill="#1a7f4b" />
            </BarChart>
          </ResponsiveContainer>
        </div>
      ) : null}

      <h2>Alerts</h2>
      {alerts.length === 0 ? (
        <p className="empty">No alerts.</p>
      ) : (
        <table className="data">
          <thead>
            <tr>
              <th>Type</th>
              <th>Title</th>
              <th>Time</th>
            </tr>
          </thead>
          <tbody>
            {[...alerts].reverse().map((alert) => (
              <tr key={alert.id}>
                <td>
                  <span className={`badge ${alert.kind === "link_down" ? "badge-err" : "badge-warn"}`}>
                    {alert.kind}
                  </span>
                </td>
                <td>
                  {alert.title}
                  {alert.detail ? <span className="hint"> — {alert.detail}</span> : null}
                </td>
                <td>{alert.at}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

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
