# Иконка Fono

Синий микрофон со звуковыми дугами на круглом тёмном фоне. За пределами круга — прозрачность. Сгенерировано встроенным инструментом imagegen 2 октября 2026 года.

Единственный исходник — `app-icon.png`. Команда `npm run icons:generate` создаёт нативные размеры и ICO/ICNS в `src-tauri/icons`, затем синхронизирует `public/fono-icon.png`, `public/favicon.png` и `public/favicon.ico`. Tauri использует этот комплект для приложения, окон, трея и последующих установщиков; веб-интерфейсы используют те же изображения. После изменения исходника нужно выполнить эту команду и пересобрать desktop.

SVG-надпись Fono остаётся текстовым логотипом. Старый исходник красной иконки удалён, чтобы случайная повторная генерация не вернула прежний знак.

## Промпт генерации

Use case: logo-brand. Asset type: production Windows desktop application icon for Fono, a modern local voice dictation app. Create one polished square 1024x1024 icon asset, not a presentation or mockup. A perfect circular deep midnight navy background, centered and occupying about 92% of the canvas; the area outside the circle must be genuinely transparent. The only symbol inside is a distinctive bold electric-blue and cyan microphone/voice mark: an elegant upright rounded microphone capsule, a clean U-shaped cradle and short stem, with one restrained sound-wave arc on each side. The central voice symbol should be large, simple and optically centered, occupying about 60% of the circle, so it stays recognizable at 16-32 pixels. Modern premium desktop branding, smooth precise contours, restrained sapphire-to-cyan gradient on the symbol, extremely subtle inner blue illumination on the navy circle. Strong contrast, clean silhouette, near-flat graphic design with only subtle depth. No letters, no words, no Fono text, no watermark, no decorative scenery, no mountains, no outer square, no rounded-square background, no badge border, no drop shadow outside the circle, no tiny details, no perspective. Round circular background only, transparent outside.
