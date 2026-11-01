# Проекция сессии в компактных виджетах

Стенд использует готовые Model, Mode, Context, существующий protocol driver и
общий `/proc` sampler Core. Не запускает Cargo, GUI, backend или модель.
Синтетический снимок содержит восемь результатов тулов по 512 КиБ.

```sh
python3 experiments/performance/session-projection/bench.py --packages /absolute/ready/share/extensions --full --output target/verification/projection-before
python3 experiments/performance/session-projection/bench.py --packages /absolute/ready/share/extensions --output target/verification/projection-after
python3 -m unittest discover -s experiments/performance/session-projection -p 'test_*.py'
```

`--full` — контроль прежнего поведения с полной проекцией. Без него стенд
применяет `session_entries` из манифеста, как production host. Проверяет,
что изменение состояния меняет badge, закрывает view/context, проверяет EOF.
Бинарники должны совпадать между сравнениями; новый код этих виджетов не нужен.
Форма данных и число записей одинаковы. Политика admission тестируется отдельно
в Desktop; этот замер показывает стоимость приёмника, а не всего приложения.
GUI/backend/driver исключены из PSS. Snapshot bytes — encoded JSON, не heap.

В протоколе содержимое entries заменено счётчиком. История и конфигурация
пользователя не читаются. Запускайте последовательно, без сборок и других RAM
бенчмарков. Все пути output должны быть новыми, чтобы не перезаписать доказательства.
