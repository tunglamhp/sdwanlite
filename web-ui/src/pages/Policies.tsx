import { useMemo } from "react";
import { useSdwanStore } from "../store";

export default function Policies() {
  const deviceSummaries = useSdwanStore((state) => state.deviceSummaries);
  const configByDeviceId = useSdwanStore((state) => state.configByDeviceId);
  const policies = useMemo(() => {
    const out: Array<{ deviceId: string; hostname: string; classes: Array<{ name: string; dscp: number }> }> = [];
    for (const device of deviceSummaries) {
      const config = configByDeviceId[device.device_id];
      if (!config) continue;
      out.push({
        deviceId: device.device_id,
        hostname: device.hostname,
        classes: config.qos.classes.map((qosClass) => ({ name: qosClass.name, dscp: qosClass.dscp })),
      });
    }
    return out;
  }, [deviceSummaries, configByDeviceId]);

  return (
    <div className="page">
      <h1>Policies</h1>
      <pre>{JSON.stringify(policies, null, 2)}</pre>
    </div>
  );
}
