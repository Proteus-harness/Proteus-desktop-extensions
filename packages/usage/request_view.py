"""«Запросы и расход»: period totals, per-model split and the request list."""

from proteus_extension import button, select
import report
from widgets import badge, card, kv, label, metric, note, row, table

PAGE = 20
ORDERS = [("oldest","По порядку"),("newest","Сначала последние"),("longest","Сначала долгие"),("tokens","Больше токенов"),("cost","Сначала дороже")]
STATUSES = [("all","Все запросы"),("problems","С проблемами"),("missing","Без данных о токенах")]


def render(state, snapshot):
    turns = report.turn_ids(snapshot)
    models = list(dict.fromkeys(report.model_key(request) for request in snapshot["requests"]))
    if state.model not in models: state.model = None
    scope = report.scoped(snapshot, state.period, state.model)
    rows = report.filtered(scope, state.status, state.query, state.order)
    pages = max(1, (len(rows) + PAGE - 1) // PAGE)
    state.page = min(state.page, pages - 1)
    numbers = {request["exchange_id"]: index + 1 for index, request in enumerate(snapshot["requests"])}
    body = [controls(state, turns, models), summary(scope),
            {"type":"input","id":"query","value":state.query,"placeholder":"ID запроса, хода или модель","action":"query"},
            row(select("status",dict(STATUSES)[state.status],STATUSES,state.status,"status"),
                select("order",dict(ORDERS)[state.order],ORDERS,state.order,"order"),
                note(f"Запросы: {len(rows)} из {len(scope)}"))]
    if not rows:
        body.append(label("В этой сессии ещё нет записанных запросов." if not snapshot["requests"]
                          else "По этим фильтрам запросов нет.", "muted"))
    for request in rows[state.page * PAGE:(state.page + 1) * PAGE]:
        body.append(entry(request, numbers[request["exchange_id"]], turns, request["exchange_id"] in state.expanded))
    if pages > 1:
        body.append(row(*([button("Назад","page",state.page - 1,variant="ghost")] if state.page > 0 else []),
                        note(f"{state.page + 1} / {pages}"),
                        *([button("Дальше","page",state.page + 1,variant="ghost")] if state.page + 1 < pages else [])))
    body.append(note(f"Оценка по стандартным ценам API ({report.PRICE_SOURCE}, {report.PRICE_DATE}), а не списание. "
                     "Неизвестные модели не оцениваются."))
    return body


def controls(state, turns, models):
    periods = [("all","Весь чат"),("latest","Последний ход")]
    periods += [(turn, f"Ход {index + 1}") for index, turn in reversed(list(enumerate(turns)))]
    if state.period not in dict(periods): state.period = "all"
    children = [select("period",dict(periods)[state.period],periods,state.period,"period")]
    if len(models) > 1:
        children.append(select("model",state.model or "Все модели",[(None,"Все модели")]+[(model,model) for model in models],state.model,"model"))
    return row(*children)


def summary(scope):
    total = report.totals(scope)
    parts = [f"вспомогательные: {total['auxiliary']}" if total["auxiliary"] else None,
             f"ошибки: {total['errors']}" if total["errors"] else None,
             f"без результата: {total['unfinished']}" if total["unfinished"] else None]
    children = [row(label(f"{report.exact(total['input_tokens'] + total['output_tokens'])} токенов", size="large"),
                    label(f"≈ {report.money(total['cost'])}", "accent", "large") if total["priced"]
                    else label("стоимость неизвестна", "muted")),
                row(metric("Вход", report.exact(total["input_tokens"])),
                    metric("Выход", report.exact(total["output_tokens"])),
                    metric("Из кэша", report.exact(total["cached_input_tokens"]), "входит во вход"),
                    metric("Запись кэша", report.exact(total["cache_creation_input_tokens"]), "входит во вход"),
                    metric("Рассуждения", report.exact(total["reasoning_output_tokens"]), "входят в выход"),
                    metric("Запросы", total["requests"], " · ".join(part for part in parts if part) or None), gap="medium")]
    if total["reported"] < total["requests"]:
        children.append(label(f"Токены известны для {total['reported']} из {total['requests']} запросов. "
                              "Остальной расход неизвестен.", "warning", "small"))
    if total["priced"] < total["reported"]:
        children.append(label(f"Стоимость учтена для {total['priced']} из {total['requests']} запросов; "
                              "показана известная часть.", "warning", "small"))
    models = report.by_model(scope)
    if len(models) > 1:
        children.append(table(["Модель", "Запросов", "Токены", "Стоимость"], [
            [model, part["requests"], report.exact(part["input_tokens"] + part["output_tokens"]),
             "≈ " + report.money(part["cost"]) if part["priced"] else "—"] for model, part in models]))
    return card("Итог выбранного периода", *children)


def entry(request, number, turns, open):
    usage = request.get("usage") or {}
    status, tone = report.STATUS.get(request["status"], (request["status"], "muted"))
    turn = request.get("turn_id")
    where = f"Ход {turns.index(turn) + 1}" if turn in turns else "Вне хода"
    price = report.cost(request)
    head = [button(("▾ " if open else "▸ ") + f"#{number} · {request['model']['model']}", "request", request["exchange_id"],
                   variant="ghost", selected=open, hint="Свернуть" if open else "Подробнее"),
            badge(status, tone)]
    if request["origin"] == "auxiliary":
        head.append(badge("Вспомогательный", "accent"))
    head += [note(f"{where} · {report.seconds(request)} · {report.clock(request['started_at_ms'])}"),
             label(f"{report.exact(report.used(request))} токенов" if usage else "токены неизвестны"),
             note(f"≈ {report.money(price)}" if price is not None else "—")]
    children = [row(*head), table(["Вход", "Кэш", "Запись", "Выход", "Мысли"], [[report.exact(usage.get(field)) for field in (
        "input_tokens", "cached_input_tokens", "cache_creation_input_tokens", "output_tokens", "reasoning_output_tokens")]])]
    if open:
        children += details(request)
    return card(None, *children)


def details(request):
    rows = [["Провайдер", request["model"]["provider"]],
            ["Тип", "Вспомогательный запрос" if request["origin"] == "auxiliary" else "Запрос модели"],
            ["Начало", report.clock(request["started_at_ms"])],
            ["Длительность", report.seconds(request)],
            ["Сообщений на входе", request["message_count"]],
            ["Доступных инструментов", request["tool_count"]]]
    if request.get("reasoning_effort"):
        rows.append(["Уровень рассуждения", request["reasoning_effort"]])
    if request.get("max_output_tokens") is not None:
        rows.append(["Лимит выхода", report.exact(request["max_output_tokens"])])
    if request.get("finish_reason") is not None:
        rows.append(["Причина завершения", report.FINISH.get(request["finish_reason"], request["finish_reason"])])
    price = report.price(request)
    if isinstance(price, str):
        rows.append(["Стоимость", price])
    else:
        rows += [["Стоимость входа", report.money(price[0])], ["Стоимость выхода", report.money(price[1])]]
        if price[2]:
            rows.append(["Тариф", "Применён тариф длинного контекста"])
    rows.append(["ID запроса", request["exchange_id"]])
    if request.get("turn_id"):
        rows.append(["ID хода", request["turn_id"]])
    result = [kv(rows)]
    if request.get("usage") is None:
        result.append(label("Провайдер не вернул данные о токенах. Нулевой расход не предполагается.", "warning", "small"))
    return result
