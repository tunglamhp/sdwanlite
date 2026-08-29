import { useMemo } from "react";
import { useSdwanStore } from "../store";

export default function BGP() {
  const selectedDeviceId = useSdwanStore((state) => state.selectedDeviceId);
  const configByDeviceId = useSdwanStore((state) => state.configByDeviceId);
  const routes = useMemo(() => {
    const config = selectedDeviceId ? configByDeviceId[selectedDeviceId] : null;
    return config?.routes ?? [];
  }, [selectedDeviceId, configByDeviceId]);

  return (
    <div className="page">
      <h1>BGP</h1>
      {!selectedDeviceId ? (
        <p className="empty">Select a device to view BGP routes.</p>
      ) : routes.length === 0 ? (
        <p className="empty">No BGP routes.</p>
      ) : (
        <table className="data">
          <thead>
            <tr>
              <th>Destination</th>
              <th>Next hop</th>
              <th>Metric</th>
            </tr>
          </thead>
          <tbody>
            {routes.map((route, index) => (
              <tr key={index}>
                <td>{route.destination}</td>
                <td>{route.next_hop}</td>
                <td>{route.metric ?? "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
