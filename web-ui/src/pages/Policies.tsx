import { useMemo } from "react";
import { useSdwanStore } from "../store";

export default function Policies() {
  const deviceSummaries = useSdwanStore((state) => state.deviceSummaries);
  const configByDeviceId = useSdwanStore((state) => state.configByDeviceId);

  const rows = useMemo(() => {
    const out: Array<{ deviceId: string; hostname: string; name: string; dscp: number }> = [];
    for (const device of deviceSummaries) {
      const config = configByDeviceId[device.device_id];
      if (!config) continue;
      for (const qosClass of config.qos.classes) {
        out.push({ deviceId: device.device_id, hostname: device.hostname, name: qosClass.name, dscp: qosClass.dscp });
      }
    }
    return out;
  }, [deviceSummaries, configByDeviceId]);

  return (
    <div className="page">
      <h1>Policies</h1>
      {rows.length === 0 ? (
        <p className="empty">No policies configured.</p>
      ) : (
        <table className="data">
          <thead>
            <tr>
              <th>Device</th>
              <th>Class</th>
              <th>DSCP</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row, index) => (
              <tr key={index}>
                <td>{row.hostname}</td>
                <td>{row.name}</td>
                <td>{row.dscp}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
