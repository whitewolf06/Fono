import type { CSSProperties } from "react";
import type { CommandPreview, LaunchAppDraft } from "../domain/commandCatalog";
import { builtInCommandGroups } from "../domain/commandCatalog";
import { AppIcon, ShortcutIcon } from "./CommandIcons";
import { ShortcutRecorder } from "@/v2/shared/presentation/components/ShortcutRecorder";

interface CommandsDraftValues {
  hotkey: string;
  volumeStep: number;
  applications: LaunchAppDraft[];
  testPhrase: string;
}

interface CommandsDraftActions {
  update: <Key extends keyof CommandsDraftValues>(
    key: Key,
    value: CommandsDraftValues[Key],
  ) => void;
  addApplication: () => void;
  updateApplication: (
    id: string,
    key: keyof Omit<LaunchAppDraft, "id">,
    value: string,
  ) => void;
  removeApplication: (id: string) => void;
}

interface CommandCardProps extends CommandsDraftActions {
  draft: CommandsDraftValues;
}

export function CommandShortcutCard({ draft, update }: CommandCardProps) {
  return (
    <section className="v2-command-card v2-command-card--shortcut">
      <div className="v2-command-card__heading">
        <span className="v2-command-card__icon">
          <ShortcutIcon />
        </span>
        <div>
          <h2>Режим команд</h2>
          <p>
            Записывает фразу отдельно от обычной диктовки и сразу выполняет
            действие.
          </p>
        </div>
      </div>
      <div className="v2-command-shortcut-layout">
        <ShortcutRecorder
          label="Горячая клавиша голосовых команд"
          value={draft.hotkey}
          defaultValue="Ctrl + Shift + Space"
          conflicts={[
            { value: "Ctrl + Alt + F", label: "горячая клавиша диктовки" },
          ]}
          onChange={(value) => update("hotkey", value)}
        />
        <ol className="v2-command-steps">
          <li>
            <b>1</b> Зажмите клавишу.
          </li>
          <li>
            <b>2</b> Скажите команду.
          </li>
          <li>
            <b>3</b> Отпустите — Fono распознает и выполнит её.
          </li>
        </ol>
      </div>
      <p className="v2-command-note">
        После wake word можно начать фразу со слова «команда»: например,
        «команда, громче».
      </p>
    </section>
  );
}

export function BuiltInCommandsCard({ draft, update }: CommandCardProps) {
  return (
    <section className="v2-command-card">
      <div className="v2-command-card__heading">
        <span className="v2-command-card__icon">⌘</span>
        <div>
          <h2>Встроенные команды</h2>
          <p>Работают в Windows без внешней LLM.</p>
        </div>
      </div>
      <div className="v2-command-groups">
        {builtInCommandGroups.map((group) => (
          <article className="v2-command-group" key={group.id}>
            <span className="v2-command-group__icon">{group.icon}</span>
            <div>
              <h3>{group.title}</h3>
              <p>{group.description}</p>
            </div>
            <ul>
              {group.commands.map((command) => (
                <li key={command.phrase}>
                  <kbd>{command.phrase}</kbd>
                  <span>{command.result}</span>
                </li>
              ))}
            </ul>
          </article>
        ))}
      </div>
      <label className="v2-field v2-field--range v2-command-volume">
        <span>
          Шаг изменения громкости
          <b>{draft.volumeStep}%</b>
        </span>
        <input
          type="range"
          min="2"
          max="20"
          step="2"
          value={draft.volumeStep}
          style={
            {
              "--range": `${((draft.volumeStep - 2) / 18) * 100}%`,
            } as CSSProperties
          }
          onChange={(event) => update("volumeStep", Number(event.target.value))}
        />
      </label>
    </section>
  );
}

export function LaunchApplicationsCard({
  addApplication,
  draft,
  removeApplication,
  updateApplication,
}: CommandCardProps) {
  return (
    <section className="v2-command-card">
      <div className="v2-command-card__heading">
        <span className="v2-command-card__icon">
          <AppIcon />
        </span>
        <div>
          <h2>Приложения для запуска</h2>
          <p>Добавьте путь к .exe и варианты произношения названия.</p>
        </div>
        <button
          className="v2-button v2-button--primary"
          type="button"
          onClick={addApplication}
        >
          Добавить приложение
        </button>
      </div>
      {draft.applications.length === 0 ? (
        <div className="v2-command-empty-state">
          <AppIcon />
          <strong>Приложения ещё не добавлены</strong>
          <span>
            После настройки можно сказать: «запусти Telegram» или «старт VS
            Code».
          </span>
        </div>
      ) : (
        <div className="v2-command-app-list">
          {draft.applications.map((application) => (
            <article className="v2-command-app" key={application.id}>
              <label className="v2-field">
                <span>Название</span>
                <input
                  placeholder="Telegram"
                  value={application.name}
                  onChange={(event) =>
                    updateApplication(
                      application.id,
                      "name",
                      event.target.value,
                    )
                  }
                />
              </label>
              <label className="v2-field">
                <span>Путь к .exe</span>
                <input
                  placeholder="C:\\Program Files\\...\\Telegram.exe"
                  value={application.executablePath}
                  onChange={(event) =>
                    updateApplication(
                      application.id,
                      "executablePath",
                      event.target.value,
                    )
                  }
                />
              </label>
              <label className="v2-field">
                <span>Алиасы через запятую</span>
                <input
                  placeholder="телега, тг"
                  value={application.aliases}
                  onChange={(event) =>
                    updateApplication(
                      application.id,
                      "aliases",
                      event.target.value,
                    )
                  }
                />
              </label>
              <button
                className="v2-icon-button"
                type="button"
                aria-label={`Удалить ${application.name || "приложение"}`}
                title="Удалить приложение"
                onClick={() => removeApplication(application.id)}
              >
                ×
              </button>
            </article>
          ))}
        </div>
      )}
    </section>
  );
}

export function CommandPreviewCard({
  draft,
  preview,
  update,
}: Pick<CommandCardProps, "draft" | "update"> & { preview: CommandPreview }) {
  return (
    <section className="v2-command-card">
      <div className="v2-command-card__heading">
        <span className="v2-command-card__icon">◈</span>
        <div>
          <h2>Проверка формулировки</h2>
          <p>
            Предпросмотр объясняет, как Fono поймёт фразу. Он ничего не
            запускает.
          </p>
        </div>
      </div>
      <label className="v2-field">
        <span>Скажите или введите пример команды</span>
        <input
          placeholder="Например: переключись на Telegram"
          value={draft.testPhrase}
          onChange={(event) => update("testPhrase", event.target.value)}
        />
      </label>
      <div className={`v2-command-preview v2-command-preview--${preview.tone}`}>
        <span>
          {preview.tone === "ready"
            ? "✓"
            : preview.tone === "attention"
              ? "!"
              : "i"}
        </span>
        <div>
          <strong>{preview.title}</strong>
          <p>{preview.description}</p>
        </div>
      </div>
    </section>
  );
}

export function CommandRoadmapCard() {
  return (
    <section className="v2-command-card v2-command-roadmap">
      <div className="v2-command-card__heading">
        <span className="v2-command-card__icon">✦</span>
        <div>
          <h2>Свободные команды через LM Studio</h2>
          <p>Будущий command-agent для фраз вроде «сделай громкость 50».</p>
        </div>
        <span className="v2-command-roadmap__badge">В roadmap</span>
      </div>
      <p>
        Сначала Fono будет показывать план действия и просить подтверждение
        перед любым изменением в системе. Эта функция ещё не подключена.
      </p>
    </section>
  );
}
