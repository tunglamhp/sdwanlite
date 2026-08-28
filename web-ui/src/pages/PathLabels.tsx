import { useMemo } from "react";
import { useSdwanStore } from "../store";

export default function PathLabels() {
  const deviceSummaries = useSdwanStore((state) => state.deviceSummaries);
  const configByDeviceId = useSdwanStore((state) => state.configByDeviceId);
  const labels = useMemo(() => {
    const seen = new Map<string, { deviceId: string; name: string; type: string; sla: string }>();
    for (const device of deviceSummaries) {
      const config = configByDeviceId[device.device_id];
      if (!config) continue;
      for (const label of config.path_labels) {
        seen.set(label.id, { deviceId: device.device_id, name: label.name, type: label.type, sla: label.sla });
      }
    }
    return Array.from(seen.values());
  }, [deviceSummaries, configByDeviceId]);

  return (
    <div className="page">
      <h1>Path Labels</h1>
      <ul>
        {labels.map((label) => (
          <li key={`${label.deviceId}-${label.name}`}>
            {label.name} ({label.type}) — SLA: {label.sla}
          </li>
        ))}
      </ul>
    </div>
  );
}
