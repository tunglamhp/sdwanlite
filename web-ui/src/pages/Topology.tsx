import { useMemo } from "react";
import { useSdwanStore } from "../store";

export default function Topology() {
  const deviceSummaries = useSdwanStore((state) => state.deviceSummaries);
  const telemetryByDeviceId = useSdwanStore((state) => state.telemetryByDeviceId);

  const devices = useMemo(
    () =>
      deviceSummaries.map((device) => {
        const frame = telemetryByDeviceId[device.device_id];
        const hasLinkDown = frame?.flags.some((f) => f.kind === "link_down") ?? false;
        const hasDegraded = frame?.flags.some((f) => f.kind === "degraded") ?? false;
        return { device, frame, hasLinkDown, hasDegraded };
      }),
    [deviceSummaries, telemetryByDeviceId],
  );

  const links = useMemo(
    () =>
      Object.values(telemetryByDeviceId).flatMap((frame) =>
        frame.links.map((link) => ({
          deviceId: frame.device_id,
          hostname: deviceSummaries.find((d) => d.device_id === frame.device_id)?.hostname ?? frame.device_id.slice(0, 8),
          ...link,
        })),
      ),
    [telemetryByDeviceId, deviceSummaries],
  );

  return (
    <div className="page">
      <h1>Topology</h1>

      <h2>Devices</h2>
      {devices.length === 0 ? (
        <p className="empty">No devices registered.</p>
      ) : (
        <div className="stats">
          {devices.map(({ device, hasLinkDown, hasDegraded }) => (
            <div className="card" key={device.device_id}>
              <div className="card-label">{device.site_id || "—"}</div>
              <div className="card-value">{device.hostname}</div>
              {hasLinkDown ? (
                <p className="status-err">link down</p>
              ) : hasDegraded ? (
                <p className="status-warn">degraded</p>
              ) : (
                <p className="status-ok">healthy</p>
              )}
            </div>
          ))}
        </div>
      )}

      <h2>Links</h2>
      {links.length === 0 ? (
        <p className="empty">No link telemetry yet.</p>
      ) : (
        <table className="data">
          <thead>
            <tr>
              <th>Device</th>
              <th>Path label</th>
              <th>Interface</th>
              <th>TX</th>
              <th>RX</th>
              <th>Peer</th>
            </tr>
          </thead>
          <tbody>
            {links.map((link, index) => (
              <tr key={index}>
                <td>{link.hostname}</td>
                <td>{link.path_label}</td>
                <td>{link.interface}</td>
                <td>{link.tx_bytes}</td>
                <td>{link.rx_bytes}</td>
                <td>{link.peer_endpoint ?? "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
