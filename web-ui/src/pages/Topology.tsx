import { useSdwanStore } from "../store";

export default function Topology() {
  const deviceSummaries = useSdwanStore((state) => state.deviceSummaries);

  return (
    <div className="page">
      <h1>Topology</h1>
      <p className="empty">
        Topology graph is not available yet. {deviceSummaries.length} device{deviceSummaries.length === 1 ? "" : "s"} registered.
      </p>
    </div>
  );
}
