import { useMemo } from "react";

const DEVICE_SAMPLES = [
  {
    device_id: "00000000-0000-0000-0000-000000000101",
    org_id: "00000000-0000-0000-0000-000000000201",
    site_id: "site-a",
    hostname: "edge-samples-01",
    state: "connected",
    version: 3,
    config: {
      org_id: "00000000-0000-0000-0000-000000000201",
      site_id: "site-a",
      hostname: "edge-samples-01",
      interfaces: [
        { name: "eth0", addresses: ["198.51.100.10/24"] },
        { name: "wg0", addresses: ["10.255.0.2/32"] },
      ],
      tunnels: {
        kind: "wire_guard",
        interface: "wg0",
        public_key: "abcdefghijklmnopqrstuvwxyz1234567890AB",
        endpoint: "198.51.100.1:51821",
        keepalive_secs: 20,
      },
      routes: [
        { destination: "0.0.0.0/0", gateway: "10.255.0.1", metric: 100 },
      ],
      firewall: {
        rules: [
          {
            action: "accept",
            source: "10.0.0.0/8",
            destination: "0.0.0.0/0",
            protocol: "tcp",
            port: 443,
            comment: "sample allow",
          },
          { action: "drop", source: "0.0.0.0/0", destination: "0.0.0.0/0", protocol: null, port: null, comment: "default deny" },
        ],
      },
      qos: {
        classes: [
          { name: "voice", dscp: 46, bandwidth_bps: 1000000 },
          { name: "default", dscp: 0, bandwidth_bps: 5000000 },
        ],
      },
      path_labels: [
        { name: "primary", kind: "internet", sla_latency_ms: 30, sla_jitter_ms: 10, sla_loss_ppm: 100 },
      ],
      policies: {
        rules: [
          { match: { protocol: "tcp", port: 443 }, action: "allow", comment: "sample policy" },
        ],
      },
      bgp: { enabled: false },
      health_checks: [],
    },
  },
  {
    device_id: "00000000-0000-0000-0000-000000000102",
    org_id: "00000000-0000-0000-0000-000000000201",
    site_id: "site-b",
    hostname: "edge-samples-02",
    state: "degraded",
    version: 1,
    config: {
      org_id: "00000000-0000-0000-0000-000000000201",
      site_id: "site-b",
      hostname: "edge-samples-02",
      interfaces: [
        { name: "eth0", addresses: ["198.51.100.20/24"] },
      ],
      tunnels: {
        kind: "wire_guard",
        interface: "wg0",
        public_key: "abcdefghijklmnopqrstuvwxyz1234567890AB",
        endpoint: "198.51.100.1:51821",
        keepalive_secs: 20,
      },
      routes: [],
      firewall: { rules: [] },
      qos: { classes: [] },
      path_labels: [],
      policies: { rules: [] },
      bgp: { enabled: false },
      health_checks: [],
    },
  },
];

const TELEMETRY_SAMPLES = [
  {
    device_id: "00000000-0000-0000-0000-000000000101",
    uptime_secs: 86400,
    links: [
      { iface: "eth0", tx_bytes: 1000000, rx_bytes: 2000000, latency_ms: 12 },
      { iface: "wg0", tx_bytes: 500000, rx_bytes: 800000, latency_ms: 28 },
    ],
    flags: [],
  },
  {
    device_id: "00000000-0000-0000-0000-000000000102",
    uptime_secs: 120,
    links: [
      { iface: "eth0", tx_bytes: 0, rx_bytes: 0, latency_ms: 0 },
    ],
    flags: [
      { kind: "link_down", path_label: "primary", subsystem: null },
      { kind: "degraded", path_label: null, subsystem: "link_monitor" },
    ],
  },
];

const FIREWALL_SAMPLES = [
  {
    action: "accept",
    source: "10.0.0.0/8",
    destination: "0.0.0.0/0",
    protocol: "tcp",
    port: 443,
    comment: "sample allow",
  },
  {
    action: "drop",
    source: "0.0.0.0/0",
    destination: "0.0.0.0/0",
    protocol: null,
    port: null,
    comment: "default deny",
  },
  {
    action: "reject",
    source: "203.0.113.0/24",
    destination: "0.0.0.0/0",
    protocol: "tcp",
    port: 22,
    comment: "sample reject ssh",
  },
];

const LB_SAMPLES = {
  tcp: [
    {
      name: "sample-tcp",
      algorithm: "failover",
      active_conns: 3,
      rejected_conns: 0,
      backends: [
        { addr: "127.0.0.1:9101", healthy: true, active_conns: 3, total_conns: 10 },
        { addr: "127.0.0.1:9102", healthy: false, active_conns: 0, total_conns: 0 },
      ],
    },
  ],
  http: [
    {
      name: "sample-http",
      routes: [
        { host: "example.test", path_prefix: "/", algorithm: "round_robin", backends: 2 },
        { host: "api.example.test", path_prefix: "/v1", algorithm: "least_connections", backends: 2 },
      ],
    },
  ],
};

export default function Samples() {
  const copy = (text: string) => {
    navigator.clipboard.writeText(text).catch(() => undefined);
  };

  const deviceJson = useMemo(() => JSON.stringify(DEVICE_SAMPLES, null, 2), []);
  const telemetryJson = useMemo(() => JSON.stringify(TELEMETRY_SAMPLES, null, 2), []);
  const firewallJson = useMemo(() => JSON.stringify(FIREWALL_SAMPLES, null, 2), []);
  const lbJson = useMemo(() => JSON.stringify(LB_SAMPLES, null, 2), []);

  return (
    <div className="page">
      <h1>Samples</h1>
      <p className="hint">Import-ready sample payloads for devices, telemetry, firewall rules, and LB state.</p>

      <section>
        <h2>Virtual Devices</h2>
        <pre>{deviceJson}</pre>
        <button type="button" onClick={() => copy(deviceJson)}>Copy devices JSON</button>
      </section>

      <section>
        <h2>Telemetry Frames</h2>
        <pre>{telemetryJson}</pre>
        <button type="button" onClick={() => copy(telemetryJson)}>Copy telemetry JSON</button>
      </section>

      <section>
        <h2>Firewall Rules</h2>
        <pre>{firewallJson}</pre>
        <button type="button" onClick={() => copy(firewallJson)}>Copy firewall JSON</button>
      </section>

      <section>
        <h2>Load Balancer Snapshot</h2>
        <pre>{lbJson}</pre>
        <button type="button" onClick={() => copy(lbJson)}>Copy LB JSON</button>
      </section>
    </div>
  );
}
