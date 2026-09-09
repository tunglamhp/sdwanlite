export type Uuid = string & { readonly __brand: "Uuid" };

export type ProbeType = "icmp" | "http" | "dns" | "tcp";
export type PathLabelType = "mpls" | "internet" | "5g" | "starlink" | "lte" | "other";
export type FirewallAction = "accept" | "drop" | "reject";
export type TunnelKind = "wire_guard"; // P0 only; P1 adds ipsec/sstp

export interface HealthCheckConfig {
  interval_ms: number;
  probe_type: ProbeType;
  threshold: number;
  timeout_ms: number;
}

export interface Interface {
  name: string;
  addresses: string[];
  mtu?: number;
  path_label?: string | null;
}

export interface WireGuardTunnel {
  kind: "wire_guard";
  interface: string;
  path_label: string;
  health_check?: HealthCheckConfig;
  endpoint: string;
  allowed_ips: string[];
  public_key: string;
}

export type TunnelConfig = WireGuardTunnel; // P0 only

export interface Route {
  destination: string;
  next_hop: string;
  metric?: number;
}

export interface FirewallRule {
  action: FirewallAction;
  source?: string | null;
  destination?: string | null;
  protocol?: string | null;
  port?: number | null;
  comment?: string | null;
}

export interface FirewallPolicy {
  rules: FirewallRule[];
}

export interface QosClass {
  name: string;
  dscp: number;
  bandwidth_bps?: number;
}

export interface QosPolicy {
  classes: QosClass[];
}

export interface PathLabel {
  id: string;
  name: string;
  type: PathLabelType;
  sla: string;
}

export interface DeviceConfig {
  device_id: Uuid;
  org_id: Uuid;
  site_id: Uuid;
  hostname: string;
  interfaces: Interface[];
  tunnels: TunnelConfig[];
  routes: Route[];
  firewall: FirewallPolicy;
  qos: QosPolicy;
  path_labels: PathLabel[];
  version: number;
}

export interface RegisterRequest {
  device_id: Uuid;
  org_id: Uuid;
  site_id: Uuid;
  hostname: string;
  version?: number;
}

export interface RegisterResponse {
  device_id: Uuid;
  org_id: Uuid;
  site_id: Uuid;
  current_version: number;
  stream_url: string;
}

export interface DeviceRecord {
  device_id: Uuid;
  org_id: Uuid;
  site_id: Uuid;
  hostname: string;
  state: "Registered" | "Configuring" | "Active" | "Degraded" | "Maintenance";
  last_seen: number;
}

export interface ApplyRequest {
  config: DeviceConfig;
}

export interface ApplyResponse {
  device_id: Uuid;
  applied_version: number;
  verified: boolean;
}

export interface ErrorBody {
  error: string;
  message?: string;
}
export interface AlertEvent {
  id: number;
  kind: string;
  title: string;
  detail?: string;
  at: string;
}

export interface LinkSample {
  path_label: string;
  interface: string;
  local_endpoint: string;
  tx_bytes: number;
  rx_bytes: number;
  peer_endpoint?: string | null;
}

export interface HealthFlagLinkDown {
  kind: "link_down";
  path_label: string;
}

export interface HealthFlagDegraded {
  kind: "degraded";
  subsystem: string;
}

export type HealthFlag = HealthFlagLinkDown | HealthFlagDegraded;

export interface TelemetryFrame {
  device_id: Uuid;
  org_id: Uuid;
  uptime_secs: number;
  links: LinkSample[];
  flags: HealthFlag[];
}

export interface StatusResponse {
  // TODO: define when backend contracts these shapes
  [key: string]: unknown;
}

export interface SignalsResponse {
  // TODO: define when backend contracts these shapes
  [key: string]: unknown;
}

export type DeviceSummary = Pick<DeviceRecord, "device_id" | "org_id" | "site_id" | "hostname">;
