# Нативные рендереры: эксперимент

Отдельный opt-in workspace с двумя процессными расширениями протокола 4:

| Пакет | Готовая библиотека | Вход |
|---|---|---|
| [mermaid-rust-render](packages/mermaid-rust-render/README.md) | mermaid-rs-renderer 0.3.1, MIT | fenced `mermaid` |
| [latex-render](packages/latex-render/README.md) | latex-rust 2.1.1, MIT/Apache-2.0; STIX Two Math, OFL | математические fenced `latex`, `tex` |

Библиотеки разбирают исходник и строят SVG; адаптеры передают тему и используют
существующий ограниченный кэш. Клиент уже рисует SVG нативно через GPUI/resvg.
Node.js и браузер этим пакетам не нужны. Стандартные Markdown и JSON renderer
уже написаны на Rust; у стандартного Mermaid остаётся Node/Pele worker.

Эксперимент находится вне стандартного `packages/*`, имеет отдельный Cargo.lock
и не попадает в обычный bundle. Версии библиотек закреплены точно. Синтаксис
Mermaid и его компоновка могут отличаться от Pele/mermaid.js. LaTeX пока
поддерживает математические блоки, без inline-формул и компиляции документов.
Пределы входа, кэша и ограничения upstream описаны в README каждого пакета.
В частности, fontdb/кэш метрик Mermaid живёт до завершения процесса; жёсткого
прерывания синхронной компоновки в выбранной библиотеке нет.

Настройки Mermaid находятся на странице расширения: наследование радиуса
приложения или собственное значение. Для обновления уже открытых диаграмм
нужна возможность клиента `surfaces.content.invalidate.v1`; старый клиент
без неё отклоняет этот экспериментальный пакет. Настройки хранятся отдельно
от пакета; установка ZIP их не перезаписывает. Общая рамка кодовых блоков
принадлежит клиенту, parser/layout/рисование содержимого — расширениям.

## Сборка и установка

Команды выполняются из корня `Proteus-desktop-extensions`, текущим toolchain
репозитория. Используйте общий target, сохраняя накопленный кэш:

```bash
export CARGO_TARGET_DIR="$PWD/target"
export CARGO_BUILD_JOBS=2
cargo test --locked --manifest-path experiments/native-renderers/Cargo.toml --workspace
python3 -m unittest discover -s experiments/native-renderers -p 'test_*.py'
python3 experiments/native-renderers/package.py --package mermaid-rust-render
python3 experiments/native-renderers/package.py --package latex-render
```

ZIP появляются в `experiments/native-renderers/dist/`; `--output DIR` меняет
папку. Для локального запуска `--dev` кладёт готовый бинарник в пакет.
Адаптер вызывает поддерживаемый корневой упаковщик и требует явный выбор
одного пакета: сборка Core, Desktop и остальных расширений не запускается.
Без `--package` и с `--bundle` он завершается до вызова Cargo.

ZIP можно установить через настройки расширений готового клиента. Перед
включением `mermaid-rust-render` выключите `mermaid-render`, иначе два пакета
объявляют один язык. Для изолированной проверки скопируйте готовый набор
расширений в отдельную папку, замените там Mermaid на экспериментальный,
добавьте LaTeX и задайте `PROTEUS_EXTENSIONS_DIR` этой папке. Пользовательские
настройки клиента также направьте в отдельный `XDG_CONFIG_HOME`.

## Регрессии стрелок Mermaid

```bash
python3 experiments/native-renderers/test-arrows.py --offline
```

Постоянные геометрические тесты находятся в vendored `src/render/anchors.rs`.
Runner копирует crate в `target/verification/native-renderers/arrow-lib-test`,
изолирует его workspace и запускает только `cargo test --no-default-features
--lib render::anchors::tests`. Общий кэш — `Proteus-desktop-extensions/target`,
`CARGO_PROFILE_TEST_DEBUG=0`; лог — `target/verification/native-renderers/arrow-tests.log`.
Для первого запуска без прогретых зависимостей уберите `--offline`.
Архив upstream не содержит fixtures, нужных другим upstream unit-тестам,
поэтому только в генерируемой копии отключены их test-модули; production-код,
модули render/anchors и проверяемый vendor остаются без изменений.

## Повторяемые замеры

`bench.py` запускает только готовый пакет, без Cargo, GUI, Core и модели.
Он использует существующий Linux sampler из соседнего Proteus-core и сохраняет
исходник, SVG, протокол, `/proc` samples и report.json в новой папке.

```bash
python3 experiments/native-renderers/package.py --package mermaid-rust-render --dev
python3 experiments/native-renderers/package.py --package latex-render --dev
python3 experiments/native-renderers/bench.py \
  --package experiments/native-renderers/packages/mermaid-rust-render \
  --source experiments/native-renderers/packages/mermaid-rust-render/tests/fixtures/flowchart.mmd \
  --language mermaid --output target/verification/native-renderers/rust-mermaid
python3 experiments/native-renderers/bench.py \
  --package experiments/native-renderers/packages/latex-render \
  --source experiments/native-renderers/fixtures/formula.tex \
  --language latex --output target/verification/native-renderers/rust-latex
```

Для сравнения передайте `--package` готовой папке стандартного `mermaid-render`
и тот же Mermaid-исходник. Нужны только синтетические данные: они сохраняются.
Проверяются первый ответ, повторные ответы, две сессии, закрытие первого и
последнего view, завершение по EOF и отсутствие оставшихся процессов.
Отчёт содержит SHA-256 бинарника/входа/манифеста, задержки, RSS/PSS/Swap,
наблюдаемые пики и обнаруженные JavaScript-процессы. Пики сэмплируются и могут
пропускать короткие всплески. Это замер процесса расширения с прогретым кэшем
ОС, а не всей программы и не времени запуска сессии на 5 000/50 000 сообщений.

Для серии `pele-01..03`, `rust-mermaid-01..03`, `rust-latex-01..03`
`summarize.py` пересчитывает медианы непосредственно из raw evidence и
проверяет SHA актуальных бинарников, исходника, SVG и общей темы. Экспорт
содержит только переносимые параметры и результаты, без путей HOME/XDG,
PID и содержимого реальных сессий. Существующий результат не перезаписывается:

```bash
python3 experiments/native-renderers/summarize.py --output experiments/native-renderers/results/local-check
```

Сохранённый результат: [замеры 29.10.2026](results/2026-10-29/MEASUREMENTS.md)
и [машиночитаемый отчёт](results/2026-10-29/report.json).

[fixtures/preview.md](fixtures/preview.md) — небольшой постоянный синтетический
чат для нативной проверки. Его можно передать готовому
`native_scroll_fixture CONFIG_ROOT WORKSPACE 2 --content FILE` из Core;
компилировать Core ради этого не нужно. Используйте отдельные папки данных.
