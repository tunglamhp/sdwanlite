import { useState } from "react";
import type { DeviceConfig, FirewallAction } from "../types/sdwan";

interface Props {
  deviceId: string;
  hostname: string;
  config: DeviceConfig;
  onApply: (config: DeviceConfig) => Promise<void>;
}

export default function DeviceDetail({ deviceId, hostname, config, onApply }: Props) {
  const [draft, setDraft] = useState<DeviceConfig>(() => structuredClone(config));
  const [applying, setApplying] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [applied, setApplied] = useState<string | null>(null);

  // --- firewall ---
  const [fwAction, setFwAction] = useState<FirewallAction>("accept");
  const [fwSource, setFwSource] = useState("");
  const [fwDest, setFwDest] = useState("");
  const [fwPort, setFwPort] = useState("");

  // --- routes ---
  const [rtDest, setRtDest] = useState("");
  const [rtHop, setRtHop] = useState("");
  const [rtMetric, setRtMetric] = useState("");

  // --- qos ---
  const [qName, setQName] = useState("");
  const [qDscp, setQDscp] = useState("");
  const [qBw, setQBw] = useState("");

  // --- path labels ---
  const [plName, setPlName] = useState("");
  const [plType, setPlType] = useState("mpls");
  const [plSla, setPlSla] = useState("");

  const apply = async () => {
    setApplying(true);
    setError(null);
    setApplied(null);
    const next = { ...draft, version: draft.version + 1 };
    try {
      await onApply(next);
      setDraft(next);
      setApplied(`Applied v${next.version} (verified)`);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setApplying(false);
    }
  };

  return (
    <div className="detail">
      <h2>{hostname} — configuration</h2>
      {error ? <div className="alert">{error}</div> : null}
      {applied ? <p className="status-ok">{applied}</p> : null}

      <h2>Firewall</h2>
      <table className="data">
        <thead>
          <tr>
            <th>Action</th>
            <th>Source</th>
            <th>Destination</th>
            <th>Port</th>
            <th>Comment</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {draft.firewall.rules.map((rule, index) => (
            <tr key={index}>
              <td>{rule.action}</td>
              <td>{rule.source ?? "—"}</td>
              <td>{rule.destination ?? "—"}</td>
              <td>{rule.port ?? "—"}</td>
              <td>{rule.comment ?? "—"}</td>
              <td>
                <button
                  type="button"
                  className="btn btn-danger"
                  onClick={() => {
                    const rules = draft.firewall.rules.filter((_, i) => i !== index);
                    setDraft({ ...draft, firewall: { rules } });
                  }}
                >
                  Remove
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <div className="form-row">
        <select value={fwAction} onChange={(e) => setFwAction(e.target.value as FirewallAction)} aria-label="Rule action">
          <option value="accept">accept</option>
          <option value="drop">drop</option>
          <option value="reject">reject</option>
        </select>
        <input value={fwSource} onChange={(e) => setFwSource(e.target.value)} placeholder="source (CIDR)" aria-label="Rule source" />
        <input value={fwDest} onChange={(e) => setFwDest(e.target.value)} placeholder="destination (CIDR)" aria-label="Rule destination" />
        <input value={fwPort} onChange={(e) => setFwPort(e.target.value)} placeholder="port" aria-label="Rule port" />
        <button
          type="button"
          className="btn"
          onClick={() => {
            const port = fwPort.trim() ? Number(fwPort.trim()) : null;
            if (fwSource.trim() || fwDest.trim()) {
              setDraft({
                ...draft,
                firewall: {
                  rules: [
                    ...draft.firewall.rules,
                    {
                      action: fwAction,
                      source: fwSource.trim() || null,
                      destination: fwDest.trim() || null,
                      protocol: null,
                      port: Number.isFinite(port) ? port : null,
                      comment: null,
                    },
                  ],
                },
              });
              setFwSource("");
              setFwDest("");
              setFwPort("");
            }
          }}
        >
          Add rule
        </button>
      </div>

      <h2>Routes</h2>
      <table className="data">
        <thead>
          <tr>
            <th>Destination</th>
            <th>Next hop</th>
            <th>Metric</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {draft.routes.map((route, index) => (
            <tr key={index}>
              <td>{route.destination}</td>
              <td>{route.next_hop}</td>
              <td>{route.metric ?? "—"}</td>
              <td>
                <button
                  type="button"
                  className="btn btn-danger"
                  onClick={() => setDraft({ ...draft, routes: draft.routes.filter((_, i) => i !== index) })}
                >
                  Remove
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <div className="form-row">
        <input value={rtDest} onChange={(e) => setRtDest(e.target.value)} placeholder="destination (CIDR)" aria-label="Route destination" />
        <input value={rtHop} onChange={(e) => setRtHop(e.target.value)} placeholder="next hop" aria-label="Route next hop" />
        <input value={rtMetric} onChange={(e) => setRtMetric(e.target.value)} placeholder="metric" aria-label="Route metric" />
        <button
          type="button"
          className="btn"
          onClick={() => {
            if (rtDest.trim() && rtHop.trim()) {
              const metric = rtMetric.trim() ? Number(rtMetric.trim()) : undefined;
              setDraft({
                ...draft,
                routes: [
                  ...draft.routes,
                  { destination: rtDest.trim(), next_hop: rtHop.trim(), metric: Number.isFinite(metric) ? metric : undefined },
                ],
              });
              setRtDest("");
              setRtHop("");
              setRtMetric("");
            }
          }}
        >
          Add route
        </button>
      </div>

      <h2>QoS classes</h2>
      <table className="data">
        <thead>
          <tr>
            <th>Class</th>
            <th>DSCP</th>
            <th>Bandwidth</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {draft.qos.classes.map((qosClass, index) => (
            <tr key={index}>
              <td>{qosClass.name}</td>
              <td>{qosClass.dscp}</td>
              <td>{typeof qosClass.bandwidth_bps === "number" ? `${qosClass.bandwidth_bps} bps` : "—"}</td>
              <td>
                <button
                  type="button"
                  className="btn btn-danger"
                  onClick={() => setDraft({ ...draft, qos: { classes: draft.qos.classes.filter((_, i) => i !== index) } })}
                >
                  Remove
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <div className="form-row">
        <input value={qName} onChange={(e) => setQName(e.target.value)} placeholder="class name" aria-label="QoS class name" />
        <input value={qDscp} onChange={(e) => setQDscp(e.target.value)} placeholder="dscp" aria-label="QoS DSCP" />
        <input value={qBw} onChange={(e) => setQBw(e.target.value)} placeholder="bandwidth bps" aria-label="QoS bandwidth" />
        <button
          type="button"
          className="btn"
          onClick={() => {
            const dscp = Number(qDscp.trim());
            if (qName.trim() && Number.isFinite(dscp)) {
              const bw = qBw.trim() ? Number(qBw.trim()) : undefined;
              setDraft({
                ...draft,
                qos: {
                  classes: [
                    ...draft.qos.classes,
                    { name: qName.trim(), dscp, bandwidth_bps: Number.isFinite(bw) ? bw : undefined },
                  ],
                },
              });
              setQName("");
              setQDscp("");
              setQBw("");
            }
          }}
        >
          Add class
        </button>
      </div>

      <h2>Path labels</h2>
      <table className="data">
        <thead>
          <tr>
            <th>Name</th>
            <th>Type</th>
            <th>SLA</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {draft.path_labels.map((label, index) => (
            <tr key={index}>
              <td>{label.name}</td>
              <td>{label.type}</td>
              <td>{label.sla}</td>
              <td>
                <button
                  type="button"
                  className="btn btn-danger"
                  onClick={() => setDraft({ ...draft, path_labels: draft.path_labels.filter((_, i) => i !== index) })}
                >
                  Remove
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <div className="form-row">
        <input value={plName} onChange={(e) => setPlName(e.target.value)} placeholder="label name" aria-label="Path label name" />
        <select value={plType} onChange={(e) => setPlType(e.target.value)} aria-label="Path label type">
          <option value="mpls">mpls</option>
          <option value="internet">internet</option>
          <option value="5g">5g</option>
          <option value="lte">lte</option>
          <option value="starlink">starlink</option>
          <option value="other">other</option>
        </select>
        <input value={plSla} onChange={(e) => setPlSla(e.target.value)} placeholder="sla" aria-label="Path label SLA" />
        <button
          type="button"
          className="btn"
          onClick={() => {
            if (plName.trim()) {
              setDraft({
                ...draft,
                path_labels: [
                  ...draft.path_labels,
                  { id: crypto.randomUUID(), name: plName.trim(), type: plType as DeviceConfig["path_labels"][number]["type"], sla: plSla.trim() || "—" },
                ],
              });
              setPlName("");
              setPlSla("");
            }
          }}
        >
          Add label
        </button>
      </div>

      <div className="form-actions">
        <button type="button" className="btn" onClick={apply} disabled={applying}>
          {applying ? "Applying…" : "Apply"}
        </button>
      </div>
      <p className="hint">Device {deviceId}</p>
    </div>
  );
}
