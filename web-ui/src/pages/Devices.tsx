import { useEffect, useRef } from "react";
import { useSdwanStore } from "../store";
import type { Uuid } from "../types/sdwan";

export default function Devices() {
  const deviceSummaries = useSdwanStore((state) => state.deviceSummaries);
  const devicesLoading = useSdwanStore((state) => state.devicesLoading);
  const devicesError = useSdwanStore((state) => state.devicesError);
  const selectedDeviceId = useSdwanStore((state) => state.selectedDeviceId);
  const setSelectedDeviceId = useSdwanStore((state) => state.setSelectedDeviceId);
  const loadDevices = useSdwanStore((state) => state.loadDevices);
  const loadDeviceConfig = useSdwanStore((state) => state.loadDeviceConfig);
  const startConfigStream = useSdwanStore((state) => state.startConfigStream);
  const removeDevice = useSdwanStore((state) => state.removeDevice);
  const stopStreamRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    loadDevices().catch(() => undefined);
    return () => {
      stopStreamRef.current?.();
      stopStreamRef.current = null;
    };
  }, [loadDevices]);

  const select = (id: Uuid) => {
    setSelectedDeviceId(id);
    stopStreamRef.current?.();
    loadDeviceConfig(id).catch(() => undefined);
    try {
      stopStreamRef.current = startConfigStream(id);
    } catch {
      stopStreamRef.current = null;
    }
  };

  const deregister = (id: Uuid, hostname: string) => {
    if (!window.confirm(`Deregister ${hostname}?`)) return;
    if (selectedDeviceId === id) {
      stopStreamRef.current?.();
      stopStreamRef.current = null;
      setSelectedDeviceId(null);
    }
    removeDevice(id).catch(() => undefined);
  };

  return (
    <div className="page">
      <h1>Devices</h1>
      {devicesError ? <div className="alert">{devicesError}</div> : null}
      {devicesLoading ? (
        <p className="empty">Loading devices…</p>
      ) : deviceSummaries.length === 0 ? (
        <p className="empty">No devices registered.</p>
      ) : (
        <table className="data">
          <thead>
            <tr>
              <th>Hostname</th>
              <th>Site</th>
              <th>Device ID</th>
              <th>Actions</th>
            </tr>
          </thead>
          <tbody>
            {deviceSummaries.map((device) => (
              <tr key={device.device_id}>
                <td>
                  {device.hostname}
                  {selectedDeviceId === device.device_id ? <span className="badge badge-ok">selected</span> : null}
                </td>
                <td>{device.site_id || "—"}</td>
                <td>{device.device_id}</td>
                <td>
                  <button type="button" className="btn" onClick={() => select(device.device_id)}>
                    Select
                  </button>{" "}
                  <button
                    type="button"
                    className="btn btn-danger"
                    onClick={() => deregister(device.device_id, device.hostname)}
                  >
                    Deregister
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
