import { useEffect, useRef, useState } from "react";
import { useSdwanStore } from "../store";
import type { Uuid } from "../types/sdwan";
import DeviceDetail from "../components/DeviceDetail";

export default function Devices() {
  const deviceSummaries = useSdwanStore((state) => state.deviceSummaries);
  const devicesLoading = useSdwanStore((state) => state.devicesLoading);
  const devicesError = useSdwanStore((state) => state.devicesError);
  const selectedDeviceId = useSdwanStore((state) => state.selectedDeviceId);
  const setSelectedDeviceId = useSdwanStore((state) => state.setSelectedDeviceId);
  const deviceById = useSdwanStore((state) => state.deviceById);
  const configByDeviceId = useSdwanStore((state) => state.configByDeviceId);
  const loadDevices = useSdwanStore((state) => state.loadDevices);
  const loadDeviceConfig = useSdwanStore((state) => state.loadDeviceConfig);
  const loadDevice = useSdwanStore((state) => state.loadDevice);
  const startConfigStream = useSdwanStore((state) => state.startConfigStream);
  const removeDevice = useSdwanStore((state) => state.removeDevice);
  const registerDevice = useSdwanStore((state) => state.registerDevice);
  const sendApply = useSdwanStore((state) => state.sendApply);
  const stopStreamRef = useRef<(() => void) | null>(null);

  // add-device form
  const [hostname, setHostname] = useState("");
  const [orgId, setOrgId] = useState("");
  const [siteId, setSiteId] = useState("");
  const [formError, setFormError] = useState<string | null>(null);
  const [registering, setRegistering] = useState(false);

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
    loadDevice(id).catch(() => undefined);
    try {
      stopStreamRef.current = startConfigStream(id);
    } catch {
      stopStreamRef.current = null;
    }
  };

  const deregister = (id: Uuid, name: string) => {
    if (!window.confirm(`Deregister ${name}?`)) return;
    if (selectedDeviceId === id) {
      stopStreamRef.current?.();
      stopStreamRef.current = null;
      setSelectedDeviceId(null);
    }
    removeDevice(id).catch(() => undefined);
  };

  const submitRegister = async () => {
    setFormError(null);
    if (!hostname.trim()) {
      setFormError("Hostname is required.");
      return;
    }
    setRegistering(true);
    try {
      await registerDevice({
        device_id: crypto.randomUUID() as Uuid,
        org_id: (orgId.trim() || crypto.randomUUID()) as Uuid,
        site_id: (siteId.trim() || crypto.randomUUID()) as Uuid,
        hostname: hostname.trim(),
      });
      setHostname("");
      setOrgId("");
      setSiteId("");
    } catch (err) {
      setFormError(err instanceof Error ? err.message : String(err));
    } finally {
      setRegistering(false);
    }
  };

  const selectedRecord = selectedDeviceId ? deviceById[selectedDeviceId] : null;
  const selectedConfig = selectedDeviceId ? configByDeviceId[selectedDeviceId] : null;

  return (
    <div className="page">
      <h1>Devices</h1>
      {devicesError ? <div className="alert">{devicesError}</div> : null}

      <div className="detail">
        <h2>Add device</h2>
        {formError ? <div className="alert">{formError}</div> : null}
        <div className="form-row">
          <div className="form-field">
            <label htmlFor="dev-hostname">Hostname</label>
            <input id="dev-hostname" value={hostname} onChange={(e) => setHostname(e.target.value)} placeholder="edge-hanoi-01" />
          </div>
          <div className="form-field">
            <label htmlFor="dev-org">Org ID (UUID, optional)</label>
            <input id="dev-org" value={orgId} onChange={(e) => setOrgId(e.target.value)} placeholder="auto-generate" />
          </div>
          <div className="form-field">
            <label htmlFor="dev-site">Site ID (UUID, optional)</label>
            <input id="dev-site" value={siteId} onChange={(e) => setSiteId(e.target.value)} placeholder="auto-generate" />
          </div>
          <button type="button" className="btn" onClick={submitRegister} disabled={registering}>
            {registering ? "Registering…" : "Register"}
          </button>
        </div>
      </div>

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

      {selectedDeviceId && selectedConfig && selectedRecord ? (
        <DeviceDetail
          deviceId={selectedDeviceId}
          hostname={selectedRecord.hostname}
          config={selectedConfig}
          onApply={(config) => sendApply(selectedDeviceId, config)}
        />
      ) : null}
    </div>
  );
}
