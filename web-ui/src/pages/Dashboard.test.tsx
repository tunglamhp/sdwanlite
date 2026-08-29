import { describe, expect, test, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import Dashboard from './Dashboard';

const { state, loadDevices, loadTelemetry, loadAlerts } = vi.hoisted(() => {
  const loadDevices = vi.fn();
  const loadTelemetry = vi.fn();
  const loadAlerts = vi.fn();
  const state = {
    deviceSummaries: [
      { device_id: 'd1', org_id: 'o1', site_id: 's1', hostname: 'edge-1' },
    ],
    devicesLoading: false,
    devicesError: null as string | null,
    telemetryByDeviceId: {
      d1: {
        device_id: 'd1',
        org_id: 'o1',
        uptime_secs: 3661,
        links: [],
        flags: [{ kind: 'link_down', path_label: 'internet' }],
      },
    },
    alerts: [],
    loadDevices,
    loadTelemetry,
    loadAlerts,
  };
  return { state, loadDevices, loadTelemetry, loadAlerts };
});

vi.mock('../store', () => ({
  useSdwanStore: (selector: (s: typeof state) => unknown) => selector(state),
}));

loadDevices.mockResolvedValue(undefined);
loadTelemetry.mockResolvedValue(undefined);
loadAlerts.mockResolvedValue(undefined);

describe('Dashboard', () => {
  test('renders device rows and status counts', () => {
    render(<Dashboard />);

    expect(screen.getByRole('heading', { name: 'Dashboard' })).toBeInTheDocument();
    expect(screen.getByText('edge-1')).toBeInTheDocument();
    expect(screen.getByText('1h 1m')).toBeInTheDocument();
    expect(screen.getByText('Links down').parentElement?.querySelector('.card-value')?.textContent).toBe('1');
    expect(loadDevices).toHaveBeenCalled();
    expect(loadTelemetry).toHaveBeenCalled();
  });

  test('renders empty state without devices', () => {
    state.deviceSummaries = [];
    state.telemetryByDeviceId = {};

    render(<Dashboard />);

    expect(screen.getByText('No devices registered.')).toBeInTheDocument();
  });
});
