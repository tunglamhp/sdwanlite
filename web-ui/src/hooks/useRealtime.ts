import { useEffect } from "react";
import { useSdwanStore } from "../store";
import { eventsStream } from "../api";

export default function useRealtime() {
  const upsertTelemetry = useSdwanStore((state) => state.upsertTelemetry);
  useEffect(() => {
    const es = eventsStream((evt) => {
      if (!evt || typeof evt !== "object") return;
      const payload = evt as { type?: string; device_id?: string; deviceId?: string; data?: unknown };
      if (payload.type === "telemetry" && typeof payload.data === "object" && payload.data !== null) {
        const frame = payload.data as { device_id?: string; deviceId?: string };
        const id = frame.device_id ?? frame.deviceId;
        if (!id) return;
        upsertTelemetry({ ...(payload.data as Record<string, unknown>), device_id: id } as never);
      }
    });
    return () => es.close();
  }, [upsertTelemetry]);
}
