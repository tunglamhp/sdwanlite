import { useMemo } from "react";
import { useSdwanStore } from "../store";

export default function QoS() {
  const selectedDeviceId = useSdwanStore((state) => state.selectedDeviceId);
  const configByDeviceId = useSdwanStore((state) => state.configByDeviceId);
  const classes = useMemo(() => {
    const config = selectedDeviceId ? configByDeviceId[selectedDeviceId] : null;
    return config?.qos?.classes ?? [];
  }, [selectedDeviceId, configByDeviceId]);

  return (
    <div className="page">
      <h1>QoS</h1>
      {!selectedDeviceId ? (
        <p className="empty">Select a device to view QoS classes.</p>
      ) : classes.length === 0 ? (
        <p className="empty">No QoS classes.</p>
      ) : (
        <table className="data">
          <thead>
            <tr>
              <th>Class</th>
              <th>DSCP</th>
              <th>Bandwidth</th>
            </tr>
          </thead>
          <tbody>
            {classes.map((qosClass, index) => (
              <tr key={index}>
                <td>{qosClass.name}</td>
                <td>{qosClass.dscp}</td>
                <td>{typeof qosClass.bandwidth_bps === "number" ? `${qosClass.bandwidth_bps} bps` : "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
