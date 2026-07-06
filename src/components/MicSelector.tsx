import { useEffect, useState } from "react";
import { ipc } from "@/lib/ipc";
import type { DeviceInfo } from "@/lib/types";

export function MicSelector({
  value,
  onChange,
}: {
  value: string | null;
  onChange: (id: string | null) => void;
}) {
  const [devices, setDevices] = useState<DeviceInfo[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    ipc
      .listAudioDevices()
      .then(setDevices)
      .catch((e) => setError(String(e)));
  }, []);

  if (error) {
    return (
      <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm text-amber-300">
        Не удалось получить список устройств: {error}
      </div>
    );
  }

  return (
    <select
      className="input"
      value={value ?? ""}
      onChange={(e) => onChange(e.target.value || null)}
    >
      <option value="">Системный по умолчанию</option>
      {devices.map((d) => (
        <option key={d.id} value={d.id}>
          {d.name}
          {d.is_default ? " (default)" : ""}
        </option>
      ))}
    </select>
  );
}
