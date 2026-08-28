import { useEffect, useMemo } from "react";
import { fetchHealth } from "../api";
import { useSdwanStore } from "../store";

export default function Dashboard() {
  const token = useSdwanStore((state) => state.token);
  const deviceSummaries = useSdwanStore((state) => state.deviceSummaries);
  const loadDevices = useSdwanStore((state) => state.loadDevices);
  const devicesLoading = useSdwanStore((state) => state.devicesLoading);
  const devicesError = useSdwanStore((state) => state.devicesError);
  const telemetryByDeviceId = useSdwanStore((state) => state.telemetryByDeviceId);

  useEffect(() => {
    fetchHealth().catch(() => undefined);
    loadDevices().catch(() => undefined);
  }, [loadDevices]);

  const goldenSignals = useMemo(() => {
    const devices = deviceSummaries.length;
    let uptimeSecs = 0;
    let txBytes = 0;
    let rxBytes = 0;
    let linkDown = 0;
    let degraded = 0;
    for (const frame of Object.values(telemetryByDeviceId)) {
      uptimeSecs += frame.uptime_secs;
      for (const link of frame.links) {
        txBytes += link.tx_bytes;
        rxBytes += link.rx_bytes;
      }
      for (const flag of frame.flags) {
        if (flag.kind === "link_down") linkDown += 1;
        if (flag.kind === "degraded") degraded += 1;
      }
    }
    return { devices, uptimeSecs, txBytes, rxBytes, linkDown, degraded };
  }, [deviceSummaries, telemetryByDeviceId]);

  return (
    <div className="page">
      <h1>Dashboard</h1>
      <section>
        <h2>Status</h2>
        <pre>{JSON.stringify({ tokenConfigured: Boolean(token), devicesLoading, devicesError }, null, 2)}</pre>
      </section>
      <section>
        <h2>Golden Signals</h2>
        <pre>{JSON.stringify(goldenSignals, null, 2)}</pre>
      </section>
      <section>
        <h2>Devices</h2>
        <pre>{JSON.stringify(deviceSummaries, null, 2)}</pre>
      </section>
    </div>
  );
}
