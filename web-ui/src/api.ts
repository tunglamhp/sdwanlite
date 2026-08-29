import type { DeviceSummary, DeviceRecord, DeviceConfig, ApplyResponse, TelemetryFrame } from "./types/sdwan";

const BASE = import.meta.env.VITE_API_BASE ?? "";

function readToken(): string | null {
  try {
    const raw = sessionStorage.getItem("sdwan.token");
    return raw ? raw : null;
  } catch {
    return null;
  }
}

async function request(path: string, init?: RequestInit): Promise<Response> {
  const headers = new Headers(init?.headers);
  if (!headers.has("content-type")) {
    headers.set("content-type", "application/json");
  }
  const token = readToken();
  if (token && !headers.has("Authorization")) {
    headers.set("Authorization", `Bearer ${token}`);
  }
  const res = await fetch(`${BASE}${path}`, {
    ...init,
    headers,
  });
  return res;
}

async function parseJson<T>(res: Response): Promise<T> {
  const text = await res.text();
  if (!res.ok) {
    let body: { error?: string; message?: string } = {};
    try {
      body = JSON.parse(text) as { error?: string; message?: string };
    } catch {
      // ignore non-JSON error bodies
    }
    throw new Error(body.message ?? body.error ?? `HTTP ${res.status}`);
  }
  return JSON.parse(text) as T;
}

export async function fetchHealth(): Promise<string> {
  const res = await request("/healthz");
  if (!res.ok) {
    throw new Error(`HTTP ${res.status}: ${await res.text()}`);
  }
  return res.text();
}

export async function fetchDevices(): Promise<DeviceSummary[]> {
  return parseJson<DeviceSummary[]>(await request("/api/v1/devices"));
}

export async function fetchDevice(id: string): Promise<DeviceRecord> {
  return parseJson<DeviceRecord>(await request(`/api/v1/devices/${encodeURIComponent(id)}`));
}

export async function deleteDevice(id: string): Promise<void> {
  const res = await request(`/api/v1/devices/${encodeURIComponent(id)}`, { method: "DELETE" });
  if (!res.ok) {
    throw new Error(`HTTP ${res.status}: ${await res.text()}`);
  }
}

export async function fetchDeviceConfig(id: string): Promise<DeviceConfig> {
  return parseJson<DeviceConfig>(await request(`/api/v1/devices/${encodeURIComponent(id)}/config`));
}
export async function fetchTelemetry(): Promise<TelemetryFrame[]> {
  return parseJson<TelemetryFrame[]>(await request("/api/v1/telemetry"));
}

export async function applyDeviceConfig(id: string, config: DeviceConfig): Promise<ApplyResponse> {
  return parseJson<ApplyResponse>(
    await request(`/api/v1/devices/${encodeURIComponent(id)}/apply`, {
      method: "POST",
      body: JSON.stringify({ config }),
    }),
  );
}

export async function postTelemetry(frame: TelemetryFrame): Promise<{ accepted: boolean }> {
  return parseJson<{ accepted: boolean }>(await request("/api/v1/telemetry", {
    method: "POST",
    body: JSON.stringify(frame),
  }));
}

export type ConfigStreamMessage = DeviceConfig;

export function eventsStream(onMessage: (evt: unknown) => void): EventSource {
  const es = new EventSource(`${BASE}/api/events`);
  es.onmessage = (e: MessageEvent) => {
    const data = typeof e.data === "string" ? JSON.parse(e.data) : e.data;
    onMessage(data);
  };
  return es;
}

export function openConfigStream(
  deviceId: string,
  onMessage: (msg: ConfigStreamMessage) => void,
  onError?: (err: Event) => void,
): WebSocket {
  const token = readToken();
  const base = BASE ? BASE.replace(/^http/, "ws") : "";
  const wsUrl = new URL(`${base}/stream/config`, window.location.origin);
  wsUrl.searchParams.set("device_id", deviceId);
  if (token) {
    wsUrl.searchParams.set("token", token);
  }
  const ws = new WebSocket(wsUrl.toString());
  ws.onmessage = (event: MessageEvent) => {
    try {
      const data = JSON.parse(event.data) as ConfigStreamMessage;
      onMessage(data);
    } catch {
      // ignore malformed frames
    }
  };
  if (onError) {
    ws.onerror = onError;
  }
  return ws;
}
