const BASE = import.meta.env.VITE_API_BASE ?? "";

async function request(path: string, init?: RequestInit): Promise<Response> {
  const res = await fetch(`${BASE}${path}`, {
    ...init,
    headers: {
      "content-type": "application/json",
      ...(init?.headers ?? {}),
    },
  });
  return res;
}

async function parseJson(res: Response): Promise<unknown> {
  const text = await res.text();
  if (!res.ok) {
    throw new Error(`HTTP ${res.status}: ${text}`);
  }
  return JSON.parse(text);
}

export async function fetchStatus(): Promise<unknown> {
  return parseJson(await request("/api/status"));
}

export async function fetchSignals(): Promise<unknown> {
  return parseJson(await request("/api/signals"));
}

export async function fetchLb(): Promise<unknown> {
  return parseJson(await request("/api/lb"));
}

export async function fetchLabels(): Promise<unknown> {
  return parseJson(await request("/api/labels"));
}

export async function fetchPolicies(): Promise<unknown> {
  return parseJson(await request("/api/policies"));
}

export function eventsStream(onMessage: (evt: unknown) => void): EventSource {
  const es = new EventSource(`${BASE}/api/events`);
  es.onmessage = (e: MessageEvent) => {
    const data = typeof e.data === "string" ? JSON.parse(e.data) : e.data;
    onMessage(data);
  };
  return es;
}
