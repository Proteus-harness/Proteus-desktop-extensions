import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "sdk"))
from proteus_extension import Analytics, stack, text, button, select, metric, code, tokens, segments
import report


ORDERS = [("oldest","По порядку"),("newest","Сначала последние"),("longest","Сначала долгие"),("tokens","Больше токенов"),("cost","Сначала дороже")]
STATUSES = [("all","Все запросы"),("problems","С проблемами"),("missing","Без данных о токенах")]


class Usage(Analytics):
    def __init__(self):
        super().__init__()
        self.tab = "requests"
        self.period = "all"
        self.model = None
        self.status = "all"
        self.order = "oldest"
        self.query = ""
        self.page = 0
        self.expanded = set()

    def render(self, view, surface, argument):
        if not self.selected_session:
            self.show(view, text("Нет выбранной сессии.", "muted"))
            return
        header = [text("Расход и контекст"), stack(self.session_picker(), button("Обновить", "refresh", icon="refresh"), direction="row"),
                  segments("tab", [("requests", "Запросы и расход"), ("context", "Контекст")], self.tab)]
        if self.tab == "context":
            snapshot = self.fetch("/context")
            used, maximum, cached, usage = report.context(snapshot)
            actual = usage.get("actual") or {}
            body = [stack(metric("Занято", tokens(used), f"{used / maximum * 100:.1f}% окна" if maximum else None),
                          metric("Размер окна", tokens(maximum) if maximum else "—"),
                          metric("Кэш", f"{cached / used * 100:.0f}%" if used and cached is not None else "—"), direction="row"),
                    {"type":"card", "title":"Измерение провайдера" if usage.get("actual") else "Локальная оценка",
                     "children":[{"type":"cells", "segments":report.cells(snapshot)}]}]
            body += [{"type":"table", "columns":["Кэш","Токенов"], "rows":[["Прочитано",str(actual.get("cached_input_tokens") if actual.get("cached_input_tokens") is not None else "—")], ["Записано",str(actual.get("cache_creation_input_tokens") if actual.get("cache_creation_input_tokens") is not None else "—")]]},
                     code("context", snapshot, 360)]
            self.show(view, stack(*(header + body)))
            return
        snapshot = self.fetch("/usage")
        if snapshot is None:
            self.show(view, stack(*header, text("Журнал расхода недоступен для этого чата.", "muted")))
            return
        requests = report.scoped(snapshot, self.period, self.model)
        summary = report.totals(requests)
        rows = report.filtered(requests, self.status, self.query, self.order)
        models = list(dict.fromkeys(report.model_key(request) for request in snapshot["requests"]))
        turns = list(dict.fromkeys(request["turn_id"] for request in snapshot["requests"] if request.get("turn_id")))
        periods = [("all","Вся сессия"),("latest","Последний ход")] + [(turn,f"Ход {index+1}") for index,turn in enumerate(turns)]
        controls = stack(select("period",dict(periods).get(self.period,"Период"),periods,self.period,"period"),
                         select("model",self.model or "Все модели",[(None,"Все модели")]+[(model,model) for model in models],self.model,"model"),
                         select("status",dict(STATUSES)[self.status],STATUSES,self.status,"status"),
                         select("order",dict(ORDERS)[self.order],ORDERS,self.order,"order"),direction="row")
        body = [controls, stack(metric("Запросов",summary["requests"],f"Usage: {summary['reported']} · auxiliary: {summary['auxiliary']}"),
                 metric("Вход",report.exact(summary["input_tokens"])),metric("Выход",report.exact(summary["output_tokens"])),
                 metric("Стоимость",f"${summary['cost']:.5f}" if summary["priced"] else "—",f"Тариф известен для {summary['priced']} из {summary['requests']} запросов"),direction="row"),
                stack(metric("Кэш прочитан",report.exact(summary["cached_input_tokens"])),metric("Кэш записан",report.exact(summary["cache_creation_input_tokens"])),
                      metric("Рассуждения",report.exact(summary["reasoning_output_tokens"])),metric("Ошибки / без завершения",f"{summary['errors']} / {summary['unfinished']}"),direction="row"),
                text(f"Оценка API-эквивалента · таблица {report.PRICE_DATE} · {report.PRICE_SOURCE}. Рассуждения входят в output; без usage расход неизвестен.","muted"),
                {"type":"input","id":"query","value":self.query,"placeholder":"ID запроса, хода или модель","action":"query"}]
        by_model = [(model,report.totals([request for request in requests if report.model_key(request)==model])) for model in models]
        body.append({"type":"table","columns":["Модель","Запросов","Вход","Выход","Стоимость"],"rows":[[model,str(total["requests"]),report.exact(total["input_tokens"]),report.exact(total["output_tokens"]),f"${total['cost']:.5f}" if total["priced"] else "—"] for model,total in by_model if total["requests"]]})
        pages = max(1,(len(rows)+19)//20)
        self.page = min(self.page,pages-1)
        for request in rows[self.page*20:(self.page+1)*20]:
            usage = request.get("usage")
            price = report.cost(request)
            label = f"{report.model_key(request)} · {request['status']} · {report.seconds(request)} · "
            label += (f"{report.exact(usage['input_tokens'])} → {report.exact(usage['output_tokens'])} токенов" if usage else "usage нет")
            label += f" · ${price:.5f}" if price is not None else " · тариф неизвестен"
            body.append(button(label,"request",request["exchange_id"]))
            if request["exchange_id"] in self.expanded:
                body.append(code(request["exchange_id"],request))
        body.append(stack(button("‹","page",max(0,self.page-1)),text(f"{self.page+1} / {pages} · {len(rows)} запросов"),button("›","page",min(pages-1,self.page+1)),direction="row"))
        self.show(view, stack(*(header+body)))

    def selection_changed(self):
        self.period,self.model,self.page="all",None,0
        self.expanded.clear()

    def action(self, view, action, value):
        super().action(view,action,value)
        if action in ("tab","period","model","status","order","query","page"):
            setattr(self,action,value)
            if action != "page": self.page=0
        elif action == "request":
            self.expanded.symmetric_difference_update({value})


if __name__ == "__main__":
    Usage().run()
