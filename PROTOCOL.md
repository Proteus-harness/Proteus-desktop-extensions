# Протокол расширений клиента, версия 3

Манифест `extension.json` и JSON Lines через stdin/stdout. Программа запускается
в папке пакета. stdout содержит только JSON; диагностика идёт в stderr.
Все поставляемые пакеты используют этот же контракт, без отдельных builtin
регистраций. Версия 1 отвергается явно: производитель и клиент обновляются вместе.

```json
{
  "protocol": 3,
  "id": "example",
  "title": "Пример",
  "author": "Proteus Project",
  "description": "Описание",
  "icon": "chart-column",
  "command": ["bin/example"],
  "session_events": true,
  "surfaces": {
    "page": {},
    "panel": {"title": "Пример"},
    "document": {"label": "{filename}"},
    "badge": {"place": "header_end", "narrow": true},
    "popup": {"width": 280, "height": 420}
  }
}
```

`id` совпадает с папкой (латинские строчные буквы, цифры, дефис; до 48 символов).
Нужна хотя бы одна поверхность; popup требует badge. Страница появляется в
настройках расширений. `document.label` поддерживает `{filename}` и
`{argument}`; `command` — программа и аргументы; путь со `/` (например,
`bin/example`) считается относительно папки пакета, иначе программа ищется в
`PATH`. `document.command` объявляет адрес команды, например
`terminal.new`. Приложение создаёт такой документ, не зная id расширения.
`popup.escape_action` перенаправляет Esc действием вместо немедленного закрытия.

Места значка: `composer_start`, `composer_end`, `header_start`, `header_end`,
`rail_start`, `rail_end`, `hidden`. Для rail требуется `narrow: true`; узкий
значок допускает icon (в том числе числовой счётчик) или ring. Текстовые
значки допускаются в остальных местах. Ширина popup — 100–1200 пикселей;
`height` — наибольшая высота до прокрутки, 120–1000 (по умолчанию 420).

## Расширяемые данные и возможности

`protocol: 3` задаёт обязательную форму сообщений. `hello.features` содержит
необязательные возможности клиента; сейчас — `session.settings.v1` и
`widgets.controls.v1` (`checkbox`, `menu`, `confirm`, многострочный `input`,
цвета `#rrggbb`, иконки Lucide, новые поля строк дерева).
`extension.json.required_features` перечисляет возможности, без которых пакет
не работает: клиент проверяет их до запуска и показывает явную ошибку.
Необязательную возможность пакет проверяет перед использованием. Feature не
меняет права процесса и не разрешает новые host callbacks.

В корне `extension.json` разрешён необязательный объект `_meta` с ключами
`namespace/name`, например `org.example/build`. Неизвестные записи сохраняются
как описательные данные; поведение и обязательные поля остаются типизированными.
Произвольные дополнительные поля вне `_meta` не допускаются.

`config.session_settings` — полное актуальное описание доступных настроек:

```json
{"id":"reasoning_effort","name":"Reasoning","category":"thought_level",
 "control":{"type":"select","value":"effort:high","options":[
   {"value":"_default","name":"Default","description":null},
   {"value":"effort:high","name":"high","description":null}]}}
```

`id` и `value` непрозрачны для общего клиента; `category` — только подсказка
для размещения. Неизвестный тип контрола не показывают, его данные сохраняются
при передаче config процессу. Известный select остаётся строгим. Сервер
проверяет id/value и возвращает полный config после изменения, включая
зависимые настройки модели. HTTP-эквивалент — `POST /session/setting` с
`{id, session_dir, setting:{id,value}}`. `_default` у рассуждений снимает явный
effort, чтобы его выбрал провайдер. Новому select не нужна новая версия
нативного enum; новый вид нативного контрола требует поддержки рендерера.

## Клиент → процесс

- `hello`: `protocol: 3`, `features`, `workspace`, `server: {origin, token}`. Токен нужен
  для HTTP app-server, передаётся только процессу. Не записывайте его в логи.
- `render`: `view`, `surface`, необязательные `argument`, `session_dir`.
  `surface`: `page`, `panel`, `document`, `badge`, `popup`. Непрозрачный `view`
  нужно возвращать без изменений.
- `close`: `view`. Поверхность закрыта; её таймеры можно остановить. Процесс
  сохраняется до закрытия workspace, выключения или обновления пакета.
- `action`: `view`, `action`, необязательное `value`, монотонный `sequence`.
- `event`: `event` (`session_opened`, `run_settled`, `navigation_changed`),
  необязательный `session_dir`, `document` — аргумент активного документа
  этого расширения. Отсутствие сессии/документа означает null.
- При `session_events: true`: `session_snapshot` с `session_dir`, `state`,
  `entries`; затем `session_update` с `session_dir`, `update`. Снимок повторяется
  при смене активного чата. Обновление `type: state` заменяет `state`;
  `type: entries` заменяет диапазон `start..start+removed` на `entries`.
  Передаются состояние/конфигурация и метаданные ленты (роль, tool, subagent),
  без текста ответов и повторной пересылки истории при каждом текстовом delta.
- `command_result`: `id`, `session_dir`, `config` или `error` для `set_session`.

`state` содержит `connected`, `config`, `context`, `execution`, `pending`,
`activity`. DTO конфигурации и событий совпадают с app-server. Точные структуры
в Proteus-desktop: `src/declarative/{protocol,session,manifest}.rs`.

## Процесс → клиент

- `view`: `view`, `session_dir` (может быть null), `sequence` (последний обработанный
  action, исходно 0), `root` — полное дерево виджетов. Ответ другой активной
  сессии или более старого action не применяется. Необязательное `keys` —
  объект `{ "ctrl-f": "search-focus", "escape": "escape" }`: сочетание вызывает
  action этой поверхности. Обычные текстовые клавиши в поле ввода сохраняются.
- `badge`: `view`, `session_dir`, `title`, `lines`, `hidden` (исходно false),
  `face`: `{type: "icon", tone, count?}`, `{type: "text", text, icon?, caret?, tone}`
  или `{type: "ring", percent, tone}`. Процент — конечное число 0–100.
- `open_document`: `argument`, `preview` (исходно false), `focus` (исходно true).
  Открывает документ своего расширения.
- `set_session`: `id`, `session_dir`, `setting: {id, value}` (`id` — строка, тип `value` задаёт control).
  Требует `session.settings.v1` в `hello.features`. Допустимые id и значения
  берутся из `config.session_settings`, а не из enum клиента. Команды выполняются по очереди, только для
  активной сессии; результат коррелируется через id. Ошибка видна в чате и
  возвращается процессу. Запросы/изменения остальных данных — через HTTP API.
- `close_popup`: закрыть выбор и вернуть прежний фокус.
- `clipboard`: `text`.
- `focus`: `view`, `control` — id input/tree/code/terminal этой поверхности;
  null возвращает фокус в активный чат. Новая панель с деревом передаёт ему
  фокус контейнера, чтобы стрелки сразу работали.

## Виджеты

Закрытая схема: неизвестное поле даёт ошибку поверхности, неизвестный тип узла
рисуется заглушкой «требует более новой версии Proteus» (остальной вид работает).
Пакеты с контролами из `widgets.controls.v1` указывают её в `required_features`. Доступны
`stack`, `fill`, `split`, `card`, `text`, `metric`, `progress`, `table`, `bar_chart`,
`tabs`, `list`, `button`, `badge`, `divider`, `input`, `checkbox`, `menu`, `code`,
`terminal`, `tree`, `graph`, `select`, `choice`, `steps`, `cells`.
Тон — `default`, `muted`, `accent`, `success`, `warning`, `danger` или цвет
`#rrggbb`. Иконки (`icon` в манифесте, кнопках, меню и строках дерева) — имена
файлов Lucide: `folder-tree`, `git-branch`, `refresh-cw`, `rotate-ccw`,
`chevron-left`, `copy`… Неизвестное имя — ошибка.
`cells` — квадратная сетка с легендой: `columns` (1..100, по умолчанию 20) и
`segments` — `{label, cells, tone?, color?, value?, detail?}`, где `cells` —
число клеток сегмента (всего не больше 2000), `color` — `#rrggbb` вместо тона,
`value`/`detail` выводятся в легенде и подсказке. Распределение клеток
считает процесс.

У интерактивных контролов постоянный `id` и `action`; сохраняйте их тип и
назначение при перерисовке. `input` передаёт строку при каждом изменении;
`lines` (1..40, по умолчанию 1) больше 1 делает растущее многострочное поле;
`submit` — action, который получает текст по Enter (в многострочном поле —
Ctrl+Enter). `checkbox` — `label`, `checked`, `action`, `value?`; шлёт
`{checked, value}` с новым состоянием. `select`/`choice` —
выбранное JSON value; `steps` — индекс варианта; `graph` — id узла;
`tree` — `{id, event}` (`select`, `preview`, `open`, `expand`, `collapse`, `check`).
`button` — `label`, `action`, `value?`, `variant` (`default`, `primary`,
`ghost`, `danger`), `icon?`, `selected` (активная кнопка
группы, например текущая вкладка), `hint?` (подсказка при наведении; для
кнопки только с иконкой `label` может быть пустым) и `confirm?`. SDK-помощник `segments`
строит ряд таких вкладок.
`confirm` — `{title, message?, ok?, danger?}`: перед отправкой action клиент
спрашивает пользователя. `menu` — кнопка (`label?`, `icon?` — по умолчанию
`ellipsis`, `hint?`) с выпадающим списком `items`: `{label, action, value?, icon?,
danger?, confirm?, separated?}` (`separated` рисует разделитель над пунктом).
Строка дерева: `id`, `label`, `detail?` (моноширинный, приглушённый), `depth`,
`expanded` (null — лист), `tone`, `selected`, `icon?` (по умолчанию папка/файл),
`icon_tone?`, `checked?` (чекбокс в строке, шлёт `check`), `strike` (зачёркнуто)
и `menu` — пункты контекстного меню по правому клику.
Дерево поддерживает стрелки, Home/End, PageUp/PageDown, Enter и пробел.
←/→ шлют `collapse`/`expand`, но на листе или свёрнутой ветке ← переводит
курсор к родителю, а на раскрытой ветке → — к первому потомку (с `preview`).
Двойной клик по ветке (`expanded` не null) не шлёт `open`: первый клик уже
отправил `select`.
`code` — редактор только для чтения, `language`, необязательная `height`;
без height занимает оставшееся место. `wrap: true` переносит длинные строки и
скрывает номера строк (для текста и логов); задаётся при создании редактора с
данным `id`. `terminal` — PTY с `cwd`.
`fill.children` отдаёт остаток высоты дереву/редактору/графу/терминалу;
`stack` располагает элементы по содержимому. `split` — ряд колонок на всю
высоту: `children` и `widths` (ширина в px или `null` для резиновой колонки);
дочерний `fill` растягивается по колонке, остальное прокручивается внутри неё.
Контролы, исчезнувшие из дерева,
освобождаются вместе с ресурсами (включая PTY).

Полные поля и проверки — [схема клиента](https://github.com/Proteus-harness/Proteus-desktop/tree/main/src/declarative),
живые примеры — `packages`. Rust SDK (`sdk`, крейт `proteus-extension`) обрабатывает жизненный цикл, последовательности,
обновления сессии и корреляцию команд. Таймеры SDK работают только при открытых
поверхностях. Закрытие приложения останавливает группу процессов расширения.
