import { describe, expect, test, vi } from 'vitest';
import {
  deleteDevice,
  eventsStream,
  fetchDevices,
  fetchHealth,
  postTelemetry,
  registerDevice,
  fetchAlerts,
  putDeviceConfig,
} from './api';

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

describe('api', () => {
  const originalFetch = globalThis.fetch;
  const originalEventSource = globalThis.EventSource;

  beforeEach(() => {
    vi.resetModules();
    globalThis.fetch = vi.fn();
    globalThis.EventSource = class MockEventSource {
      onmessage: ((evt: MessageEvent) => void) | null = null;
      constructor(public url: string) {}
      addEventListener(): void {}
      close(): void {}
    } as unknown as typeof EventSource;
  });

  afterEach(() => {
    globalThis.fetch = originalFetch;
    globalThis.EventSource = originalEventSource;
  });

  test('fetchHealth returns plain text from /healthz', async () => {
    (globalThis.fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      new Response('ok', { status: 200, headers: { 'content-type': 'text/plain' } }),
    );

    const result = await fetchHealth();

    expect(result).toBe('ok');
    expect(globalThis.fetch).toHaveBeenCalledWith(
      expect.stringContaining('/healthz'),
      expect.objectContaining({ headers: expect.any(Headers) }),
    );
  });

  test('fetchHealth throws on HTTP error with body text', async () => {
    (globalThis.fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      new Response('boom', { status: 503, headers: { 'content-type': 'text/plain' } }),
    );

    await expect(fetchHealth()).rejects.toThrow('HTTP 503: boom');
  });

  test('fetchDevices parses JSON array from /api/v1/devices', async () => {
    const devices = [{ device_id: 'd1', org_id: 'o1', site_id: 's1', hostname: 'edge-1' }];
    (globalThis.fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValueOnce(jsonResponse(devices));

    const result = await fetchDevices();

    expect(result).toEqual(devices);
    expect(globalThis.fetch).toHaveBeenCalledWith(expect.stringContaining('/api/v1/devices'), expect.anything());
  });

  test('fetchDevices surfaces server error message', async () => {
    (globalThis.fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      jsonResponse({ error: 'unauthorized', message: 'missing or invalid bearer token' }, 401),
    );

    await expect(fetchDevices()).rejects.toThrow('missing or invalid bearer token');
  });

  test('deleteDevice sends DELETE and resolves on ok', async () => {
    (globalThis.fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      new Response(null, { status: 204 }),
    );

    await deleteDevice('d1');

    expect(globalThis.fetch).toHaveBeenCalledWith(
      expect.stringContaining('/api/v1/devices/d1'),
      expect.objectContaining({ method: 'DELETE' }),
    );
  });

  test('deleteDevice throws on HTTP error', async () => {
    (globalThis.fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      jsonResponse({ error: 'not_found', message: 'device not found' }, 404),
    );

    await expect(deleteDevice('d1')).rejects.toThrow('HTTP 404: {"error":"not_found","message":"device not found"}');
  });

  test('postTelemetry posts frame and parses accepted', async () => {
    const frame = { device_id: 'd1', org_id: 'o1', uptime_secs: 5, links: [], flags: [] };
    (globalThis.fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValueOnce(jsonResponse({ accepted: true }));

    const result = await postTelemetry(frame as never);

    expect(result).toEqual({ accepted: true });
    expect(globalThis.fetch).toHaveBeenCalledWith(
      expect.stringContaining('/api/v1/telemetry'),
      expect.objectContaining({ method: 'POST' }),
    );
  });

  test('eventsStream creates EventSource for /api/events', () => {
    const onMessage = vi.fn();
    const es = eventsStream(onMessage);
    expect(es.url).toBe('/api/events');
  });

  test('registerDevice sends POST to /api/v1/devices/register and parses RegisterResponse', async () => {
    const req = { hostname: 'edge-2', site_id: 's2' };
    const response = { device_id: 'd2', registered: true };
    (globalThis.fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValueOnce(jsonResponse(response));

    const result = await registerDevice(req as any);

    expect(result).toEqual(response);
    expect(globalThis.fetch).toHaveBeenCalledWith(
      expect.stringContaining('/api/v1/devices/register'),
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify(req),
      }),
    );
  });

  test('fetchAlerts parses JSON array from /api/v1/alerts', async () => {
    const alerts = [{ id: 'a1', severity: 'warning', message: 'High CPU usage' }];
    (globalThis.fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValueOnce(jsonResponse(alerts));

    const result = await fetchAlerts();

    expect(result).toEqual(alerts);
    expect(globalThis.fetch).toHaveBeenCalledWith(
      expect.stringContaining('/api/v1/alerts'),
      expect.anything(),
    );
  });

  test('putDeviceConfig sends PUT with config body to /api/v1/devices/{id}/config', async () => {
    const config = { interfaces: [] };
    const response = { success: true };
    (globalThis.fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValueOnce(jsonResponse(response));

    const result = await putDeviceConfig('d1', config as any);

    expect(result).toEqual(response);
    expect(globalThis.fetch).toHaveBeenCalledWith(
      expect.stringContaining('/api/v1/devices/d1/config'),
      expect.objectContaining({
        method: 'PUT',
        body: JSON.stringify(config),
      }),
    );
  });
});
