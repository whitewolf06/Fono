import { useEffect, useState } from "react";

export function DevKitExtensions() {
  const [toast, setToast] = useState<string | null>(null);
  const [modalOpen, setModalOpen] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [progress, setProgress] = useState(48);
  const [shortcut, setShortcut] = useState("Ctrl + Space");
  const [invalidUrl, setInvalidUrl] = useState(false);

  useEffect(() => {
    if (!toast) return undefined;
    const timeout = window.setTimeout(() => setToast(null), 3200);
    return () => window.clearTimeout(timeout);
  }, [toast]);

  return <>
    <section className="v2-kit-section v2-kit-section--wide"><h2>Обратная связь и загрузка</h2><div className="v2-kit-section__body v2-feedback-layout">
      <div className="v2-progress-card"><div><span>Whisper Small</span><b>{progress}%</b></div><div className="v2-progress"><i style={{ width: `${progress}%` }} /></div><small>{progress === 100 ? "Модель загружена" : "Загрузка модели…"}</small><button className="v2-button" type="button" onClick={() => setProgress((value) => value >= 100 ? 0 : Math.min(100, value + 13))}>{progress === 100 ? "Сбросить" : "Продвинуть"}</button></div>
      <div className="v2-skeleton-card"><span className="v2-skeleton v2-skeleton--icon" /><div><i className="v2-skeleton v2-skeleton--title" /><i className="v2-skeleton v2-skeleton--text" /></div></div>
      <div className="v2-empty-state"><span>◌</span><strong>Пока ничего нет</strong><small>Пустое состояние для списка приложений или истории.</small></div>
    </div></section>

    <section className="v2-kit-section"><h2>Меню, tooltip и toast</h2><div className="v2-kit-section__body">
      <div className="v2-dev-menu-wrap"><button className="v2-icon-button" type="button" aria-label="Открыть меню" onClick={() => setMenuOpen(!menuOpen)}>⋯</button>{menuOpen && <div className="v2-dropdown-menu"><button type="button" onClick={() => { setToast("Настройка скопирована"); setMenuOpen(false); }}>Копировать</button><button type="button" onClick={() => { setToast("Открыта диагностика"); setMenuOpen(false); }}>Диагностика</button><button className="is-danger" type="button" onClick={() => { setModalOpen(true); setMenuOpen(false); }}>Удалить</button></div>}</div>
      <span className="v2-tooltip-wrap"><button className="v2-button" type="button">Что такое VAD?</button><span role="tooltip">Порог, отделяющий речь от тишины. Обычно его не нужно менять.</span></span>
      <button className="v2-button v2-button--primary" type="button" onClick={() => setToast("Настройки сохранены")}>Показать toast</button>
    </div></section>

    <section className="v2-kit-section"><h2>Валидация и горячая клавиша</h2><div className="v2-kit-section__body">
      <label className={`v2-field ${invalidUrl ? "has-error" : ""}`}><span>Адрес LM Studio</span><input defaultValue="http://localhost:1234/v1" onBlur={(event) => setInvalidUrl(!event.target.value.startsWith("http"))} /><small>{invalidUrl ? "Введите URL, начинающийся с http:// или https://" : "Проверяется при сохранении."}</small></label>
      <label className="v2-shortcut-recorder"><span>Горячая клавиша</span><button type="button" onKeyDown={(event) => { event.preventDefault(); const parts = [event.ctrlKey && "Ctrl", event.shiftKey && "Shift", event.altKey && "Alt", event.key.length === 1 ? event.key.toUpperCase() : event.key]; setShortcut(parts.filter(Boolean).join(" + ")); }}>{shortcut}</button><small>Нажмите нужную комбинацию</small></label>
    </div></section>

    <section className="v2-kit-section"><h2>Опасное действие</h2><div className="v2-kit-section__body"><p className="v2-kit-note">Destructive-вариант всегда объясняет последствия и требует подтверждения.</p><button className="v2-button v2-button--danger" type="button" onClick={() => setModalOpen(true)}>Очистить локальные логи</button></div></section>

    {toast && <div className="v2-toast" role="status"><i>✓</i><span>{toast}</span><button type="button" aria-label="Закрыть уведомление" onClick={() => setToast(null)}>×</button></div>}
    {modalOpen && <div className="v2-modal-backdrop" role="presentation" onMouseDown={() => setModalOpen(false)}><section className="v2-modal" role="dialog" aria-modal="true" aria-labelledby="modal-title" onMouseDown={(event) => event.stopPropagation()}><span className="v2-modal__icon">!</span><h2 id="modal-title">Очистить локальные логи?</h2><p>Журналы диагностики будут удалены с этого устройства. Настройки Fono и модели останутся без изменений.</p><div><button className="v2-button" type="button" onClick={() => setModalOpen(false)}>Отмена</button><button className="v2-button v2-button--danger" type="button" onClick={() => { setModalOpen(false); setToast("Локальные логи очищены"); }}>Очистить</button></div></section></div>}
  </>;
}
