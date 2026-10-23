Синтетический ответ для проверки нативных визуализаций. Числа — пример.

```json-render
{
  "type": "card",
  "title": "Синтетический отчёт",
  "children": [
    {"type": "text", "text": "Таблица, метрики, график и локальные Tabs", "tone": "muted"},
    {"type": "stack", "direction": "row", "gap": "large", "children": [
      {"type": "metric", "label": "Готово", "value": "12", "tone": "success"},
      {"type": "metric", "label": "Ожидают", "value": "3", "tone": "warning"}
    ]},
    {"type": "tabs", "tabs": [
      {"label": "Таблица", "count": 2, "children": [
        {"type": "table", "columns": ["Вариант", "Время"], "rows": [["Первый", "8 мин"], ["Второй", "5 мин"]]}
      ]},
      {"label": "График", "children": [
        {"type": "bar_chart", "unit": "мин", "bars": [{"label": "Первый", "value": 8}, {"label": "Второй", "value": 5, "tone": "accent"}]}
      ]}
    ]}
  ]
}
```

Цветной SVG должен показывать оба цвета и стрелку, без браузера.

```svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 240">
  <rect x="20" y="50" width="240" height="140" rx="20" fill="#334155"/>
  <rect x="380" y="50" width="240" height="140" rx="20" fill="#166534"/>
  <path d="M280 120h75m-18-18 18 18-18 18" fill="none" stroke="#f59e0b" stroke-width="8"/>
  <text x="140" y="130" text-anchor="middle" fill="white" font-size="28">Input</text>
  <text x="500" y="130" text-anchor="middle" fill="white" font-size="28">Result</text>
</svg>
```

Сломанный JSON возвращается к исходному коду с ошибкой preview:

```json-render
{"type":"text","text":}
```

Этот тип не входит в каталог; никакой action не выполняется:

```json-render
{"type":"button","label":"Never execute","action":"synthetic-command"}
```

Незарегистрированный язык остаётся обычным кодом:

```unregistered-visualization
{"type":"text","text":"Raw fallback"}
```

Последний незакрытый блок должен оставаться исходным кодом при стриминге:


Native SVG parse error после валидного XML: остаётся source, пустой preview недопустим.

```svg
<svg xmlns="http://www.w3.org/2000/svg" width="0" height="0"/>
```


```json-render
{"type":"text","text":"Incomplete synthetic preview
