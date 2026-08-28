import { useMemo } from "react";
import { useSdwanStore } from "../store";

export default function Diagnostics() {
  const telemetryByDeviceId = useSdwanStore((state) => state.telemetryByDeviceId);
  const deviceIds = Object.keys(telemetryByDeviceId);
  const frames = useMemo(() => deviceIds.map((id) => telemetryByDeviceId[id]), [deviceIds, telemetryByDeviceId]);

  return (
    <div className="page">
      <h1>Diagnostics</h1>
      <pre>{JSON.stringify({ telemetryFrameCount: frames.length, deviceIds }, null, 2)}</pre>
      <ul>
        {frames.map((frame) => (
          <li key={frame.device_id}>
            <strong>{frame.device_id}</strong> uptime {frame.uptime_secs}s — {frame.links.length} links — {frame.flags.length} flags
          </li>
        ))}
      </ul>
    </div>
  );
}
