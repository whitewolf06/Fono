interface ServiceApiPanelProps {
  address: string;
  onCopy(text: string): void;
}

const endpoints = [
  ["GET", "/openapi.json", "Спецификация OpenAPI"],
  ["GET", "/v1/health", "Проверка доступности"],
  ["POST", "/v1/transcriptions", "Файл на распознавание"],
  ["GET", "/v1/transcription-jobs/{id}", "Состояние задачи"],
  ["POST", "/v1/transcription-jobs/{id}/cancel", "Отмена задачи"],
];

export function ServiceApiPanel({ address, onCopy }: ServiceApiPanelProps) {
  const origin = `http://${address}`;
  const specificationUrl = `${origin}/openapi.json`;

  return (
    <section className="v2-service-api">
      <div className="v2-service-api__intro">
        <p className="v2-kicker">Для интеграций</p>
        <h2>Локальный REST API</h2>
        <p>Доступен только на этом компьютере и требует bearer-токен.</p>
      </div>
      <div className="v2-service-api__address">
        <code>{specificationUrl}</code>
        <button
          className="v2-button"
          type="button"
          onClick={() => onCopy(specificationUrl)}
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
        Спецификация доступна по этому URL с тем же токеном, что и REST-запросы.
        Swagger не загружается из интернета.
      </p>
    </section>
  );
}
