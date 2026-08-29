import { useMemo } from "react";
import { useSdwanStore } from "../store";

export default function Firewall() {
  const selectedDeviceId = useSdwanStore((state) => state.selectedDeviceId);
  const configByDeviceId = useSdwanStore((state) => state.configByDeviceId);
  const rules = useMemo(() => {
    const config = selectedDeviceId ? configByDeviceId[selectedDeviceId] : null;
    return config?.firewall?.rules ?? [];
  }, [selectedDeviceId, configByDeviceId]);

  return (
    <div className="page">
      <h1>Firewall</h1>
      {!selectedDeviceId ? (
        <p className="empty">Select a device to view its firewall rules.</p>
      ) : rules.length === 0 ? (
        <p className="empty">No firewall rules.</p>
      ) : (
        <table className="data">
          <thead>
            <tr>
              <th>Action</th>
              <th>Source</th>
              <th>Destination</th>
              <th>Protocol</th>
              <th>Port</th>
              <th>Comment</th>
            </tr>
          </thead>
          <tbody>
            {rules.map((rule, index) => (
              <tr key={index}>
                <td>{rule.action}</td>
                <td>{rule.source ?? "—"}</td>
                <td>{rule.destination ?? "—"}</td>
                <td>{rule.protocol ?? "—"}</td>
                <td>{rule.port ?? "—"}</td>
                <td>{rule.comment ?? "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
