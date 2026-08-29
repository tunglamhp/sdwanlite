import { useMemo } from "react";
import { useSdwanStore } from "../store";
import { formatUptime } from "../format";

export default function Diagnostics() {
  const telemetryByDeviceId = useSdwanStore((state) => state.telemetryByDeviceId);
  const frames = useMemo(() => Object.values(telemetryByDeviceId), [telemetryByDeviceId]);

  return (
    <div className="page">
      <h1>Diagnostics</h1>
      {frames.length === 0 ? (
        <p className="empty">No telemetry received yet.</p>
      ) : (
        <table className="data">
          <thead>
            <tr>
              <th>Device ID</th>
              <th>Uptime</th>
              <th>Links</th>
              <th>Flags</th>
            </tr>
          </thead>
          <tbody>
            {frames.map((frame) => (
              <tr key={frame.device_id}>
                <td>{frame.device_id}</td>
                <td>{formatUptime(frame.uptime_secs)}</td>
                <td>{frame.links.length}</td>
                <td>{frame.flags.length}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
