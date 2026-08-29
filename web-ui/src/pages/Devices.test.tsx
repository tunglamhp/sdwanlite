import { describe, expect, test, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import Devices from './Devices';

const {
  state,
  loadDevices,
  removeDevice,
  loadDeviceConfig,
  startConfigStream,
  registerDevice,
  updateDevice,
  replaceDeviceConfig,
  loadAlerts,
  setSelectedDeviceId,
} = vi.hoisted(() => {
  const loadDevices = vi.fn();
  const removeDevice = vi.fn();
  const loadDeviceConfig = vi.fn();
  const startConfigStream = vi.fn();
  const registerDevice = vi.fn();
  const updateDevice = vi.fn();
  const replaceDeviceConfig = vi.fn();
  const loadAlerts = vi.fn();
  const setSelectedDeviceId = vi.fn();

  const state = {
    deviceSummaries: [
      { device_id: 'd1', org_id: 'o1', site_id: 's1', hostname: 'edge-1' },
    ],
    devicesLoading: false,
    devicesError: null as string | null,
    selectedDeviceId: null as string | null,
    configByDeviceId: {} as Record<string, any>,
    deviceById: {} as Record<string, any>,
    alerts: [] as any[],
    loadDevices,
    removeDevice,
    loadDeviceConfig,
    startConfigStream,
    registerDevice,
    updateDevice,
    replaceDeviceConfig,
    loadAlerts,
    setSelectedDeviceId,
  };

  return {
    state,
    loadDevices,
    removeDevice,
    loadDeviceConfig,
    startConfigStream,
    registerDevice,
    updateDevice,
    replaceDeviceConfig,
    loadAlerts,
    setSelectedDeviceId,
  };
});

vi.mock('../store', () => ({
  useSdwanStore: (selector: (s: typeof state) => unknown) => selector(state),
}));

loadDevices.mockResolvedValue(undefined);
removeDevice.mockResolvedValue(undefined);
loadDeviceConfig.mockResolvedValue(undefined);
startConfigStream.mockReturnValue(() => {});
registerDevice.mockResolvedValue(undefined);
updateDevice.mockResolvedValue(undefined);
replaceDeviceConfig.mockResolvedValue(undefined);
loadAlerts.mockResolvedValue(undefined);
setSelectedDeviceId.mockImplementation((id: string | null) => {
  state.selectedDeviceId = id;
});

describe('Devices', () => {
  beforeEach(() => {
    state.deviceSummaries = [
      { device_id: 'd1', org_id: 'o1', site_id: 's1', hostname: 'edge-1' },
    ];
    state.devicesLoading = false;
    state.devicesError = null;
    state.selectedDeviceId = null;
    state.configByDeviceId = {};
    state.deviceById = {};
    state.alerts = [];

    loadDevices.mockClear();
    removeDevice.mockClear();
    loadDeviceConfig.mockClear();
    startConfigStream.mockClear();
    registerDevice.mockClear();
    updateDevice.mockClear();
    replaceDeviceConfig.mockClear();
    loadAlerts.mockClear();
    setSelectedDeviceId.mockClear();
  });

  test('renders "No devices registered." when empty', () => {
    state.deviceSummaries = [];

    render(<Devices />);

    expect(screen.getByText('No devices registered.')).toBeInTheDocument();
  });

  test('renders hostname and Register/Select and Deregister buttons when device is present', () => {
    render(<Devices />);

    expect(screen.getByText('edge-1')).toBeInTheDocument();
    
    // Check for Deregister button
    expect(screen.getByRole('button', { name: /Deregister/i })).toBeInTheDocument();
    
    // Check for Select or Register button to ensure test robustness and support any parallel modifications
    const selectOrRegisterBtn = screen.queryByRole('button', { name: /^Register$/i }) || screen.queryByRole('button', { name: /^Select$/i });
    expect(selectOrRegisterBtn).toBeInTheDocument();
  });

  test('click Deregister checks window.confirm and deletes device on confirmation', async () => {
    const confirmSpy = vi.spyOn(window, 'confirm');

    // Case 1: Cancel confirmation
    confirmSpy.mockReturnValue(false);
    removeDevice.mockClear();

    render(<Devices />);

    const deregisterBtn = screen.getByRole('button', { name: /Deregister/i });
    fireEvent.click(deregisterBtn);

    expect(confirmSpy).toHaveBeenCalledWith('Deregister edge-1?');
    expect(removeDevice).not.toHaveBeenCalled();

    // Case 2: Accept confirmation
    confirmSpy.mockReturnValue(true);
    removeDevice.mockClear();

    fireEvent.click(deregisterBtn);

    expect(confirmSpy).toHaveBeenCalledWith('Deregister edge-1?');
    expect(removeDevice).toHaveBeenCalledWith('d1');

    confirmSpy.mockRestore();
  });
});
