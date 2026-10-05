use std::{fmt, io};

#[derive(Debug)]
pub enum Error {
    Cancelled,
    Busy,
    Configuration,
    Manifest,
    UnsafeUrl,
    Network,
    Size,
    Signature,
    Version,
    Cache(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Cancelled => "Загрузка отменена. Её можно продолжить позже.",
            Self::Busy => "Другой установщик Fono уже использует этот каталог загрузки.",
            Self::Configuration => "Канал установки Fono настроен неверно.",
            Self::Manifest => "Не удалось прочитать сведения о последней версии Fono.",
            Self::UnsafeUrl => "Канал вернул недопустимый адрес загрузки. Установка запрещена.",
            Self::Network => "Не удалось скачать Fono. Проверьте подключение и повторите попытку.",
            Self::Size => "Размер установщика некорректен или превышает 512 МиБ.",
            Self::Signature => "Подпись установщика не прошла проверку. Установка запрещена.",
            Self::Version => "Подписанная версия отличается от версии канала. Установка запрещена.",
            Self::Cache(_) => {
                "Не удалось сохранить загрузку. Проверьте свободное место и права на каталог."
            }
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Cache(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Cache(error)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
