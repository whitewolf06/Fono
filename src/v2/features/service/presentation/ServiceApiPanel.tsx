interface ServiceApiPanelProps {
  address: string;
  onCopy(text: string): void;
}

const endpoints = [
  ["GET", "/docs", "Документация в браузере"],
  ["GET", "/openapi.json", "Спецификация OpenAPI"],
  ["GET", "/v1/health", "Проверка доступности"],
  ["POST", "/v1/transcriptions", "Файл на распознавание"],
  ["GET", "/v1/transcription-jobs/{id}", "Состояние задачи"],
  ["POST", "/v1/transcription-jobs/{id}/cancel", "Отмена задачи"],
];

export function ServiceApiPanel({ address, onCopy }: ServiceApiPanelProps) {
  const origin = `http://${address}`;
  const documentationUrl = `${origin}/docs`;

  return (
    <section className="v2-service-api">
      <div className="v2-service-api__intro">
        <p className="v2-kicker">Для интеграций</p>
        <h2>Локальный REST API</h2>
        <p>Доступен только на этом компьютере и требует bearer-токен.</p>
      </div>
      <div className="v2-service-api__address">
        <code>{documentationUrl}</code>
        <button
          className="v2-button"
          type="button"
          onClick={() => onCopy(documentationUrl)}
        >
          Копировать URL
        </button>
      </div>
      <ul className="v2-service-api__routes">
        {endpoints.map(([method, path, description]) => (
          <li key={path}>
            <b className={`is-${method.toLowerCase()}`}>{method}</b>
            <code>{path}</code>
            <span>{description}</span>
          </li>
        ))}
      </ul>
      <p className="v2-service-privacy-note">
        Страница откроется в браузере без токена; вставьте его в её поле, чтобы
        загрузить спецификацию. Токен не передаётся в URL и не сохраняется.
      </p>
    </section>
  );
}
