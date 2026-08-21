import { useEffect, useState } from "react";
import { ipc } from "@/lib/ipc";
import type { WhisperModelInfo, WhisperModelSize } from "@/lib/types";

const SIZE_LABEL: Record<WhisperModelSize, string> = {
  tiny: "Tiny — самый быстрый",
  base: "Base — баланс (рекомендуется)",
  small: "Small — точнее",
  medium: "Medium — очень точно",
  large: "Large v3 — максимальная точность",
  large_turbo: "Large v3 Turbo — быстрее и компактнее",
};

const MODEL_ORDER: WhisperModelSize[] = [
  "tiny",
  "base",
  "small",
  "medium",
  "large",
  "large_turbo",
];

export function ModelManager({
  selectedPath,
  onSelect,
}: {
  selectedPath: string | null;
  onSelect: (path: string) => Promise<void>;
}) {
  const [models, setModels] = useState<WhisperModelInfo[]>([]);
  const [downloading, setDownloading] = useState<WhisperModelSize | null>(null);
  const [selectingPath, setSelectingPath] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = async (): Promise<WhisperModelInfo[]> => {
    try {
      const nextModels = await ipc.listWhisperModels();
      setModels(nextModels);
      return nextModels;
    } catch (e) {
      setError(String(e));
      return [];
    }
  };

  useEffect(() => {
    void refresh();
  }, []);

  const select = async (path: string) => {
    setSelectingPath(path);
    setError(null);
    try {
      await onSelect(path);
    } catch (e) {
      setError(String(e));
    } finally {
      setSelectingPath(null);
    }
  };

  const download = async (size: WhisperModelSize) => {
    setDownloading(size);
    setError(null);
    try {
      await ipc.downloadWhisperModel(size);
      const nextModels = await refresh();
      const downloadedPath = nextModels.find(
        (model) => model.size === size,
      )?.local_path;
      if (!downloadedPath) {
        throw new Error("Скачанная модель не найдена в локальном списке");
      }
      await select(downloadedPath);
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

      {MODEL_ORDER.map((size) => {
        const model = models.find((m) => m.size === size);
        const isSelected =
          model?.local_path != null && model.local_path === selectedPath;
        const isDownloading = downloading === size;
        const isSelecting = model?.local_path === selectingPath;

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
                      onClick={() => void select(model.local_path!)}
                      disabled={isSelecting}
                    >
                      {isSelecting ? "Подключаю…" : "Выбрать"}
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
      })}

      <p className="text-xs text-neutral-500">
        Модели хранятся локально и не передаются никуда. Выбор сохраняется сразу
        после успешной проверки модели; первая загрузка может занять некоторое
        время.
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
