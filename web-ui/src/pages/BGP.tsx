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
      <ul>
        {routes.map((route, index) => (
          <li key={index}>
            {route.destination} via {route.next_hop} {route.metric != null ? `metric ${route.metric}` : ""}
          </li>
        ))}
      </ul>
    </div>
  );
}
