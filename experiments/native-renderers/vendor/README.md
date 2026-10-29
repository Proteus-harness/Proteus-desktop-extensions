# Проверенный Upstream Для Эксперимента

`mermaid-rs-renderer-0.3.1/` автоматически извлечён целиком из проверенного
архива crates.io, затем применён небольшой патч
`../patches/mermaid-rs-renderer-node-corners.patch`.
Это MIT upstream, а не собственная реализация Mermaid. Его исходный `LICENSE`
сохранён; версия, URL, SHA-256 архива/патча и изменённые файлы записаны в
`../packages/mermaid-rust-render/UPSTREAM.json`.

Патч добавляет необязательный `Theme.node_corner_radius` и отрисовку
скруглённой заливки узлов через существующий upstream helper
`rounded_polyline_path`, а также пересечение концов стрелок с этим контуром
через `kurbo`. `None` сохраняет прежние углы, `Some(0)` задаёт острые углы.
У ромба одинаковый предел trim для всех углов устраняет влияние шва
посередине одной стороны.

Маршрутизация ветвей из ромба включает backport upstream
[`3e431ecb`](https://github.com/1jehuang/mermaid-rs-renderer/commit/3e431ecb6c8f98bfeb47fc5af38db60b4c9b1b5a)
с поворотом того же алгоритма для LR/RL/TB/BT. Сам маршрутизатор, обход
препятствий и размещение подписей остаются готовыми механизмами библиотеки;
parser и размещение узлов не заменяются.

Cargo `[patch.crates-io]` действует только в экспериментальном workspace;
vendor исключён из его members. Upstream benches/tests не запускаются вместе
с тестами пакетов. Полный исходник нужен для автономного воспроизводимого
Cargo build; бинарники и target не включены.

Воспроизведение/проверка из корня репозитория (нужен системный `patch`):

```bash
python3 experiments/native-renderers/packages/mermaid-rust-render/update-vendor.py --check
python3 experiments/native-renderers/packages/mermaid-rust-render/update-vendor.py
```

Второй вызов заменяет vendor содержимым проверенного архива плюс патч.
Ни Cargo build, ни запуск расширения не выполняют скачивание/патчирование.
