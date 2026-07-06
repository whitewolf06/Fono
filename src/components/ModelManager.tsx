import { useEffect, useState } from "react";
import { ipc } from "@/lib/ipc";
import type { WhisperModelInfo, WhisperModelSize } from "@/lib/types";

const SIZE_LABEL: Record<WhisperModelSize, string> = {
  tiny: "Tiny —最快",
  base: "Base — баланс (рекомендуется)",
  small: "Small — точнее",
  medium: "Medium — очень точно",
  large: "Large — SOTA",
};

export function ModelManager({
  selectedPath,
  onSelect,
}: {
  selectedPath: string | null;
  onSelect: (path: string) => void;
}) {
  const [models, setModels] = useState<WhisperModelInfo[]>([]);
  const [downloading, setDownloading] = useState<WhisperModelSize | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = () => {
    ipc
      .listWhisperModels()
      .then(setModels)
      .catch((e) => setError(String(e)));
  };

  useEffect(() => {
    refresh();
  }, []);

  const download = async (size: WhisperModelSize) => {
    setDownloading(size);
    setError(null);
    try {
      await ipc.downloadWhisperModel(size);
      refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setDownloading(null);
    }
  };

  return (
    <div className="space-y-3">
      {error && (
        <div className="rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-sm text-red-300">
          {error}
        </div>
      )}

      {(["tiny", "base", "small", "medium", "large"] as WhisperModelSize[]).map(
        (size) => {
          const model = models.find((m) => m.size === size);
          const isSelected =
            model?.local_path != null && model.local_path === selectedPath;
          const isDownloading = downloading === size;

          return (
            <div
              key={size}
              className={`flex items-center justify-between rounded-lg border p-3 ${
                isSelected
                  ? "border-brand-500 bg-brand-500/10"
                  : "border-neutral-700 bg-neutral-800/50"
              }`}
            >
              <div className="flex-1">
                <div className="text-sm font-medium text-neutral-100">
                  {SIZE_LABEL[size]}
                </div>
                <div className="text-xs text-neutral-400">
                  {model?.local_path
                    ? `✓ Скачана (${formatBytes(model.bytes)})`
                    : model?.bytes
                      ? `${formatBytes(model.bytes)}`
                      : "Не скачана"}
                </div>
              </div>
              <div className="flex gap-2">
                {model?.local_path ? (
                  <>
                    {!isSelected && (
                      <button
                        className="btn-ghost"
                        onClick={() => onSelect(model.local_path!)}
                      >
                        Выбрать
                      </button>
                    )}
                    {isSelected && (
                      <span className="text-xs font-medium text-brand-300">
                        ✓ выбрана
                      </span>
                    )}
                  </>
                ) : (
                  <button
                    className="btn-secondary"
                    onClick={() => download(size)}
                    disabled={isDownloading}
                  >
                    {isDownloading ? "Скачиваю…" : "Скачать"}
                  </button>
                )}
              </div>
            </div>
          );
        },
      )}

      <p className="text-xs text-neutral-500">
        Модели хранятся локально и не передаются никуда. Первая загрузка может
        занять некоторое время в зависимости от модели.
      </p>
    </div>
  );
}

function formatBytes(bytes: number | null): string {
  if (!bytes) return "—";
  const mb = bytes / (1024 * 1024);
  if (mb < 1024) return `${mb.toFixed(0)} МБ`;
  return `${(mb / 1024).toFixed(1)} ГБ`;
}
