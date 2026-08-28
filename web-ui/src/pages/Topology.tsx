import { useSdwanStore } from "../store";

export default function Topology() {
  const selectedDeviceId = useSdwanStore((state) => state.selectedDeviceId);
  const deviceSummaries = useSdwanStore((state) => state.deviceSummaries);

  return (
    <div className="page">
      <h1>Topology</h1>
      <pre>{JSON.stringify({ selectedDeviceId, devices: deviceSummaries.map((device) => device.device_id) }, null, 2)}</pre>
    </div>
  );
}
