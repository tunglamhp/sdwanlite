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
      <ul>
        {classes.map((qosClass, index) => (
          <li key={index}>
            {qosClass.name} — DSCP {qosClass.dscp}
            {typeof qosClass.bandwidth_bps === "number" ? `, ${qosClass.bandwidth_bps} bps` : ""}
          </li>
        ))}
      </ul>
    </div>
  );
}
