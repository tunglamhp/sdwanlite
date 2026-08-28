import { describe, expect, test, vi } from 'vitest';
import {
  eventsStream,
  fetchHealth,
  fetchLabels,
  fetchLb,
  fetchPolicies,
  fetchSignals,
  fetchStatus,
} from './api';

function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'content-type': 'application/json' },
  });
}

function plainResponse(body: string): Response {
  return new Response(body, {
    status: 200,
    headers: { 'content-type': 'text/plain' },
  });
}

describe('api', () => {
  const originalFetch = globalThis.fetch;
  const originalEventSource = globalThis.EventSource;

  beforeEach(() => {
    vi.resetModules();
    vi.useFakeTimers();
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
    vi.useRealTimers();
  });

  test('fetchHealth returns plain text from /healthz', async () => {
    (globalThis.fetch as unknown as jest.Mock).mockResolvedValueOnce(plainResponse('ok'));

    const result = await fetchHealth();

    expect(result).toBe('ok');
    expect(globalThis.fetch).toHaveBeenCalledWith(
      expect.stringContaining('/healthz'),
      expect.objectContaining({ headers: expect.objectContaining({ 'content-type': 'application/json' }) })
    );
  });

  test('fetchStatus parses JSON from /api/status', async () => {
    (globalThis.fetch as unknown as jest.Mock).mockResolvedValueOnce(jsonResponse({ status: 'ok' }));

    const result = await fetchStatus();

    expect(result).toEqual({ status: 'ok' });
  });

  test('fetchSignals parses JSON from /api/signals', async () => {
    (globalThis.fetch as unknown as jest.Mock).mockResolvedValueOnce(jsonResponse({ signals: [] }));

    const result = await fetchSignals();

    expect(result).toEqual({ signals: [] });
  });

  test('fetchLb parses JSON from /api/lb', async () => {
    (globalThis.fetch as unknown as jest.Mock).mockResolvedValueOnce(jsonResponse({ lb: {} }));

    const result = await fetchLb();

    expect(result).toEqual({ lb: {} });
  });

  test('fetchLabels parses JSON from /api/labels', async () => {
    (globalThis.fetch as unknown as jest.Mock).mockResolvedValueOnce(jsonResponse({ labels: [] }));

    const result = await fetchLabels();

    expect(result).toEqual({ labels: [] });
  });

  test('fetchPolicies parses JSON from /api/policies', async () => {
    (globalThis.fetch as unknown as jest.Mock).mockResolvedValueOnce(jsonResponse({ policies: [] }));

    const result = await fetchPolicies();

    expect(result).toEqual({ policies: [] });
  });

  test('eventsStream creates EventSource for /api/events', () => {
    const onMessage = vi.fn();
    const es = eventsStream(onMessage);
    expect(es.url).toBe('/api/events');
  });

  test('fetchHealth throws on HTTP error', async () => {
    (globalThis.fetch as unknown as jest.Mock).mockResolvedValueOnce(
      new Response(JSON.stringify({ error: 'not_found', message: 'see server logs' }), {
        status: 404,
        headers: { 'content-type': 'application/json' },
      })
    );

    await expect(fetchHealth()).rejects.toThrow('HTTP 404: {"error":"not_found","message":"see server logs"}');
  });
});
