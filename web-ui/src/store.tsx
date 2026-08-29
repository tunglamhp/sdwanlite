import { create } from "zustand";
import type { DeviceSummary, DeviceRecord, DeviceConfig, TelemetryFrame, Uuid } from "./types/sdwan";
import { deleteDevice, fetchDevice, fetchDeviceConfig, fetchDevices, openConfigStream, postTelemetry, applyDeviceConfig } from "./api";
import { fetchTelemetry } from "./api";

export interface SdwanStore {
  token: string;
  setToken: (token: string) => void;
  selectedDeviceId: Uuid | null;
  setSelectedDeviceId: (deviceId: Uuid | null) => void;
  deviceSummaries: DeviceSummary[];
  setDeviceSummaries: (summaries: DeviceSummary[]) => void;
  devicesLoading: boolean;
  setDevicesLoading: (loading: boolean) => void;
  devicesError: string | null;
  setDevicesError: (error: string | null) => void;
  deviceById: Record<string, DeviceRecord>;
  setDevice: (record: DeviceRecord) => void;
  configByDeviceId: Record<string, DeviceConfig>;
  setConfig: (deviceId: string, config: DeviceConfig) => void;
  telemetryByDeviceId: Record<string, TelemetryFrame>;
  upsertTelemetry: (frame: TelemetryFrame) => void;
  loadDevices: () => Promise<void>;
  loadTelemetry: () => Promise<void>;
  loadDevice: (id: string) => Promise<void>;
  removeDevice: (id: string) => Promise<void>;
  loadDeviceConfig: (id: string) => Promise<void>;
  sendApply: (id: string, config: DeviceConfig) => Promise<void>;
  sendTelemetry: (frame: TelemetryFrame) => Promise<void>;
  startConfigStream: (deviceId: string) => () => void;
}

export const useSdwanStore = create<SdwanStore>((set, get) => ({
  token: "",
  setToken: (token) => set({ token }),
  selectedDeviceId: null,
  setSelectedDeviceId: (selectedDeviceId) => set({ selectedDeviceId }),
  deviceSummaries: [],
  setDeviceSummaries: (deviceSummaries) => set({ deviceSummaries }),
  devicesLoading: false,
  setDevicesLoading: (devicesLoading) => set({ devicesLoading }),
  devicesError: null,
  setDevicesError: (devicesError) => set({ devicesError }),
  deviceById: {},
  setDevice: (record) =>
    set((state) => ({
      deviceById: { ...state.deviceById, [record.device_id]: record },
    })),
  configByDeviceId: {},
  setConfig: (deviceId, config) =>
    set((state) => ({
      configByDeviceId: { ...state.configByDeviceId, [deviceId]: config },
    })),
  telemetryByDeviceId: {},
  upsertTelemetry: (frame) =>
    set((state) => ({
      telemetryByDeviceId: { ...state.telemetryByDeviceId, [frame.device_id]: frame },
    })),
  loadDevices: async () => {
    set({ devicesLoading: true, devicesError: null });
    try {
      const items = await fetchDevices();
      set({ deviceSummaries: items, devicesLoading: false });
    } catch (error) {
      set({ devicesError: error instanceof Error ? error.message : String(error), devicesLoading: false });
    }
  },
  loadTelemetry: async () => {
    try {
      const frames = await fetchTelemetry();
      for (const frame of frames) get().upsertTelemetry(frame);
    } catch {
      // telemetry is best-effort; the dashboard falls back to empty states
    }
  },
  loadDevice: async (id) => {
    try {
      const record = await fetchDevice(id);
      get().setDevice(record);
    } catch (error) {
      set({ devicesError: error instanceof Error ? error.message : String(error) });
    }
  },
  removeDevice: async (id) => {
    await deleteDevice(id);
    set((state) => {
      const deviceById = { ...state.deviceById };
      delete deviceById[id];
      const deviceSummaries = state.deviceSummaries.filter((item) => item.device_id !== id);
      return { deviceById, deviceSummaries };
    });
  },
  loadDeviceConfig: async (id) => {
    try {
      const config = await fetchDeviceConfig(id);
      get().setConfig(id, config);
    } catch (error) {
      set({ devicesError: error instanceof Error ? error.message : String(error) });
    }
  },
  sendApply: async (id, config) => {
    const outcome = await applyDeviceConfig(id, config);
    if (outcome.verified) {
      get().setConfig(id, config);
    }
  },
  sendTelemetry: async (frame) => {
    await postTelemetry(frame);
    get().upsertTelemetry(frame);
  },
  startConfigStream: (deviceId) => {
    const unsubscribers: Array<() => void> = [];
    const ws = openConfigStream(
      deviceId,
      (config) => get().setConfig(deviceId, config),
      () => undefined,
    );
    const close = () => {
      try {
        ws.close();
      } catch {
        // ignore teardown errors
      }
    };
    unsubscribers.push(close);
    return () => {
      unsubscribers.forEach((fn) => fn());
    };
  },
}));
