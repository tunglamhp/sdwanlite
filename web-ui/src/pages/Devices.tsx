import { useEffect } from "react";
import { useSdwanStore } from "../store";

export default function Devices() {
  const deviceSummaries = useSdwanStore((state) => state.deviceSummaries);
  const devicesLoading = useSdwanStore((state) => state.devicesLoading);
  const devicesError = useSdwanStore((state) => state.devicesError);
  const selectedDeviceId = useSdwanStore((state) => state.selectedDeviceId);
  const setSelectedDeviceId = useSdwanStore((state) => state.setSelectedDeviceId);
  const loadDevices = useSdwanStore((state) => state.loadDevices);
  const removeDevice = useSdwanStore((state) => state.removeDevice);

  useEffect(() => {
    loadDevices().catch(() => undefined);
  }, [loadDevices]);

  return (
    <div className="page">
      <h1>Devices</h1>
      <pre>{JSON.stringify({ loading: devicesLoading, error: devicesError, count: deviceSummaries.length }, null, 2)}</pre>
      <ul>
        {deviceSummaries.map((device) => (
          <li key={device.device_id}>
            <button
              type="button"
              onClick={() => setSelectedDeviceId(device.device_id)}
              className={selectedDeviceId === device.device_id ? "active" : undefined}
            >
              {device.hostname}
            </button>
            <button type="button" onClick={() => removeDevice(device.device_id)}>
              Deregister
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
