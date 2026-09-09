import { useEffect, useState } from "react";
import {
  createFirewallRule,
  deleteFirewallRule,
  fetchFirewallRules,
  updateFirewallRule,
  type FirewallRule,
} from "../api";
import FormField from "../components/FormField";

type RuleForm = {
  action: string;
  source: string;
  destination: string;
  protocol: string;
  port: string;
  comment: string;
};

const emptyForm: RuleForm = {
  action: "allow",
  source: "",
  destination: "",
  protocol: "tcp",
  port: "",
  comment: "",
};

export default function Firewall() {
  const [rules, setRules] = useState<FirewallRule[]>([]);
  const [form, setForm] = useState<RuleForm>(emptyForm);
  const [editingIndex, setEditingIndex] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const load = async () => {
    try {
      const data = await fetchFirewallRules();
      setRules(data.rules);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : "failed to load firewall rules");
      setRules([]);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    load();
  }, []);

  const onSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    const rule: FirewallRule = {
      action: form.action,
      source: form.source || null,
      destination: form.destination || null,
      protocol: form.protocol || "any",
      port: form.port ? Number(form.port) : null,
      comment: form.comment || null,
    };
    try {
      if (editingIndex == null) {
        const data = await createFirewallRule(rule);
        if (!data.ok) {
          setError("failed to add rule");
          return;
        }
        setRules(data.rules);
      } else {
        const data = await updateFirewallRule(editingIndex, rule);
        if (!data.ok) {
          setError("failed to update rule");
          return;
        }
        setRules(data.rules);
        setEditingIndex(null);
      }
      setForm(emptyForm);
    } catch (e) {
      setError(e instanceof Error ? e.message : "request failed");
    }
  };

  const onEdit = (index: number) => {
    const rule = rules[index];
    setEditingIndex(index);
    setForm({
      action: rule.action,
      source: rule.source ?? "",
      destination: rule.destination ?? "",
      protocol: rule.protocol ?? "tcp",
      port: rule.port != null ? String(rule.port) : "",
      comment: rule.comment ?? "",
    });
  };

  const onDelete = async (index: number) => {
    setError(null);
    try {
      const data = await deleteFirewallRule(index);
      if (!data.ok) {
        setError("failed to delete rule");
        return;
      }
      setRules(data.rules);
      if (editingIndex === index) {
        setEditingIndex(null);
        setForm(emptyForm);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : "request failed");
    }
  };

  return (
    <div className="page">
      <h1>Firewall</h1>
      {error && <p className="alert">{error}</p>}
      {loading && <p className="empty">Loading firewall rules…</p>}
      <form className="form" onSubmit={onSubmit}>
        <FormField label="Action">
          <select
            value={form.action}
            onChange={(e) => setForm((f) => ({ ...f, action: e.target.value }))}
          >
            <option value="allow">Allow</option>
            <option value="deny">Deny</option>
            <option value="reject">Reject</option>
          </select>
        </FormField>
        <FormField label="Source">
          <input
            placeholder="10.0.0.0/24 or 10.0.*"
            value={form.source}
            onChange={(e) => setForm((f) => ({ ...f, source: e.target.value }))}
          />
        </FormField>
        <FormField label="Destination">
          <input
            placeholder="optional"
            value={form.destination}
            onChange={(e) => setForm((f) => ({ ...f, destination: e.target.value }))}
          />
        </FormField>
        <FormField label="Protocol">
          <input
            placeholder="tcp, udp, any"
            value={form.protocol}
            onChange={(e) => setForm((f) => ({ ...f, protocol: e.target.value }))}
          />
        </FormField>
        <FormField label="Port">
          <input
            placeholder="0 = any"
            value={form.port}
            onChange={(e) => setForm((f) => ({ ...f, port: e.target.value }))}
          />
        </FormField>
        <FormField label="Comment">
          <input
            placeholder="optional note"
            value={form.comment}
            onChange={(e) => setForm((f) => ({ ...f, comment: e.target.value }))}
          />
        </FormField>
        <div className="form-row">
          <button type="submit" className="btn">
            {editingIndex == null ? "Add rule" : "Save rule"}
          </button>
          {editingIndex != null && (
            <button
              type="button"
              className="btn"
              onClick={() => {
                setEditingIndex(null);
                setForm(emptyForm);
              }}
            >
              Cancel
            </button>
          )}
        </div>
      </form>
      {!loading && rules.length === 0 && !error && <p className="empty">No firewall rules.</p>}
      {!loading && rules.length > 0 && (
        <table className="data">
          <thead>
            <tr>
              <th>Action</th>
              <th>Source</th>
              <th>Destination</th>
              <th>Protocol</th>
              <th>Port</th>
              <th>Comment</th>
              <th />
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
                <td>
                  <button type="button" className="btn" onClick={() => onEdit(index)}>
                    Edit
                  </button>
                  <button type="button" className="btn btn-danger" onClick={() => onDelete(index)}>
                    Delete
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
