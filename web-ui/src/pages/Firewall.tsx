import { useMemo } from "react";
import { useSdwanStore } from "../store";

export default function Firewall() {
  const selectedDeviceId = useSdwanStore((state) => state.selectedDeviceId);
  const configByDeviceId = useSdwanStore((state) => state.configByDeviceId);
  const config = selectedDeviceId ? configByDeviceId[selectedDeviceId] : null;
  const rules = useMemo(() => config?.firewall?.rules ?? [], [config]);

  return (
    <div className="page">
      <h1>Firewall</h1>
      <ul>
        {rules.map((rule, index) => (
          <li key={index}>
            {rule.action}: {rule.source ?? "*"} → {rule.destination ?? "*"} {rule.protocol ? `(${rule.protocol})` : ""}
            {rule.port != null ? `:${rule.port}` : ""}
            {rule.comment ? ` // ${rule.comment}` : ""}
          </li>
        ))}
      </ul>
    </div>
  );
}
