"""Exercise every shipped process using synthetic public-API data.

--wire emits the actual messages for the Rust protocol conformance test.
"""

import contextlib
import http.server
import json
from pathlib import Path
import queue
import subprocess
import sys
import threading
import urllib.parse

PACKAGES = Path(__file__).resolve().parents[1] / "packages"


def state():
    return {
        "connected":True,"context":{"used":14021,"max":200000,"trigger":180000},
        "execution":{"active":None,"last":None},"pending":{"approvals":[],"user_inputs":[],"queued_user_messages":[]},
        "activity":{"status":"idle"},
        "config":{"profile":"fixture","permission_mode":"normal","model":{"provider":"fixture","name":"gpt-6-astra","label":"Test model"},
                  "session_settings":[
                      {"id":"mode","name":"Permissions","category":"mode","control":{"type":"select","value":"normal","options":[{"value":mode,"name":mode} for mode in ("normal","auto","plan")]}},
                      {"id":"model","name":"Model","category":"model","control":{"type":"select","value":"gpt-6-astra","options":[{"value":"gpt-6-astra","name":"Test model"}]}},
                      {"id":"reasoning_effort","name":"Reasoning","category":"thought_level","control":{"type":"select","value":"_default","options":[{"value":"_default","name":"Default"},{"value":"effort:low","name":"low"},{"value":"effort:high","name":"high"}]}},
                  ],
                  "model_options":[{"provider":"fixture","name":"gpt-6-astra","label":"Test model","hidden":False,"reasoning_efforts":["low","high"],"default_reasoning_effort":"high"}],
                  "reasoning":{"enabled":True,"effort":None,"effort_options":["low","high"]},
                  "registered_tools":[{"name":"probe"}],"modules":[{"slot":"model","id":"fake"}]},
    }


def entry():
    return {"role":"assistant","tool":{"name":"update_plan","args":{"plan":[{"step":"One","status":"completed"},{"step":"Two","status":"pending"}]},"effective_args":None,"status":"completed"},"subagent":None}


def usage():
    request={"exchange_id":"request-1","turn_id":"turn-1","model":{"provider":"fixture","model":"gpt-6-astra"},"origin":"direct",
             "started_at_ms":1000,"finished_at_ms":2000,"status":"completed","finish_reason":"stop",
             "usage":{"input_tokens":10000,"output_tokens":1000,"cached_input_tokens":2000,"cache_creation_input_tokens":None,"reasoning_output_tokens":400},
             "message_count":3,"tool_count":1,"reasoning_effort":"high","max_output_tokens":None}
    return {"session_id":"session-fixture","revision":1,"latest_turn_id":"turn-1","requests":[request]}


def context():
    return {"session_dir":"/session","session_id":"00000000-0000-0000-0000-000000000000","workspace_path":"/workspace",
            "activity":{"status":"idle","running_runs":0,"running_run_ids":[],"pending_approvals":0,"pending_user_inputs":0},
            "history":{"messages":2,"user_messages":1,"assistant_messages":1,"system_messages":0,"tool_results":0,"estimated_tokens":100},
            "latest_usage":{"model_provider":"fixture","model_name":"gpt-6-astra","phase":None,"estimated_input_tokens":100,
                            "max_input_tokens":200000,"compaction_trigger_tokens":180000,"actual":usage()["requests"][0]["usage"],
                            "categories":[{"name":"messages","tokens":100,"source":"estimated"}],"source":"mixed","turn_id":"turn-1","timestamp_ms":2000},
            "latest_context":{"chunks":3,"token_estimate":100,"turn_id":"turn-1","timestamp_ms":1000},
            "latest_compaction":{"status":"completed","summary_present":True,"turn_id":"turn-1","timestamp_ms":1500,
                                 "report":{"changed":True,"input_messages":10,"output_messages":2,"original_token_estimate":9000,"output_token_estimate":800}},
            "tools":{"requested":1,"finished":1,"failed":0,"names":["probe"]},"diagnostics":["fixture warning"]}


def topology():
    return {"profile":"fixture","cwd":"/workspace","config_path":None,"config_files":[],"module_epoch":1,"permission_mode":"normal","model":None,
            "slots":[{"id":"model","title":"Модель","responsibility":"Select model","active_module":"fake","required":True,"category":"backend","order":1}],
            "modules":[{"id":"fake","slot":"model","active":True,"source":{"kind":"process"},"version":"1","api_version":"v12","capabilities":[],"description":"Fixture"}],
            "tools":[{"name":"probe","description":"Test tool","safety":"read_only","source":"fixture","owner":None,"enabled":True,"runtime_managed":False,"registered":True,"input_schema":{"type":"object"}}],
            "edges":[{"from":"config","to":"slot:model","kind":"configures","label":None},{"from":"slot:model","to":"module:model:fake","kind":"selects","label":None},{"from":"tools","to":"tool:probe","kind":"contains","label":None}],"warnings":[],"hooks":[]}


def message(role, text, part=None):
    return {"id":f"message-{role}","role":role,"phase":None,"name":None,"tool_call_id":None,"metadata":None,
            "parts":[{"part_id":"part-1","provenance":"model","scope":"conversation","payload":part or {"Text":{"text":text}}}]}


def analysis():
    """The /analysis contract: turns plus every step kind of the selected one."""
    request=usage()["requests"][0]
    call={"id":"call-1","name":"probe","args":{"path":"sample.py"},"surface":"function","raw_arguments":None}
    model={"kind":"model","exchange_id":"exchange-1","origin":"direct","messages":[],"failure":None,
           "request":{"model":request["model"],"instructions":[{"kind":"system","text":"Be brief.","priority":0}],
                      "messages":[message("User","Synthetic task")],"tools":[{"name":"probe"}]},
           "response":{"messages":[message("Assistant","",{"ToolCall":{"call":call}})],"tool_calls":[call],"finish_reason":"ToolCalls",
                       "usage":request["usage"],"end_turn":None,"provider_metadata":None}}
    hook={"kind":"hook","trace":{"owner":None,"input":{"event":{"event":"before_tool","call":call,"spec":None,"blocked":None}},
          "steps":[{"invocation_id":"hook-1","module_id":"guard","outcome":{"status":"failed","message":"Synthetic hook failure","model_failure":None}}],"output":None}}
    tool={"kind":"tool","call":call,"approval_reason":"Synthetic approval","resolution":{"decision":"approved"},
          "result":{"call_id":"call-1","ok":True,"output":"ok\n"*3,"content":[],"error":None,"metadata":None}}
    compaction={"kind":"compaction","history_revision":2,"report":{"changed":True,"user_message_replacements":[],"reason":"threshold","input_messages":12,
                "output_messages":3,"original_token_estimate":150000,"output_token_estimate":9000,"trigger_tokens":140000,"summary_source":None,
                "skipped_reason":None,"summary":"Synthetic summary","metadata":None}}
    steps=[{"sequence":sequence,"execution_id":"execution-1","thread_id":None,"started_at_ms":1000*sequence,"finished_at_ms":1000*sequence+100,"data":data}
           for sequence,data in enumerate((model,hook,tool,compaction),1)]
    turns=[{"turn_id":f"turn-{number}","number":number,"prompt_preview":"Synthetic task "*20,"status":"success" if number%3 else "error",
            "started_at_ms":1000*number,"finished_at_ms":None} for number in range(1,11)]
    return {"session_id":"session-fixture","revision":7,"turns":turns,
            "selected":{"turn_id":"turn-10","task":{"text":"Synthetic task","cwd":"/workspace"},"module_epoch":1,"config_snapshot":{"fixture":True},
                        "steps":steps,"output":{"text":"Synthetic answer","metadata":None},"error":"Synthetic turn error"}}


WRITES=[]


def changes():
    return {"repository":True,"branch":"main","head":{"short":"aaaaaaa","summary":"Initial commit"},"upstream":"origin/main","ahead":1,"behind":0,
            "entries":[{"path":"sample.py","original":None,"status":"modified","staged":None,"unstaged":"modified"},
                       {"path":"docs/new.md","original":None,"status":"untracked","staged":None,"unstaged":"untracked"},
                       {"path":"moved.py","original":"old.py","status":"renamed","staged":"renamed","unstaged":"modified"}],"truncated":False}


class Api(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.headers.get("Authorization") != "Bearer fixture":
            self.send_error(403)
            return
        parsed=urllib.parse.urlsplit(self.path)
        query=urllib.parse.parse_qs(parsed.query)
        path=parsed.path
        if path=="/sessions": data=[{"session_dir":"/session","preview":"Synthetic session","message_count":2}]
        elif path=="/usage": data=usage()
        elif path=="/context": data=context()
        elif path=="/model/quota": data={"observed_at":1700000000,"buckets":[{"id":"main","windows":[{"id":"week","duration_seconds":604800,"used_percent":80}]}]}
        elif path=="/workspace/changes": data=changes()
        elif path=="/workspace/branches": data={"current":"main","branches":[{"name":"main","upstream":"origin/main","last_commit_time":1700000000},{"name":"topic","upstream":None,"last_commit_time":1690000000}],"truncated":False}
        elif path=="/workspace/log": data={"entries":[{"hash":"a"*40,"short":"aaaaaaa","summary":"Initial commit","author":"Fixture","time":1700000000}]}
        elif path=="/workspace/list":
            folder=query.get("path",[""])[0]
            data={"path":folder,"entries":[{"name":"sample.py","path":"sample.py","kind":"file"}],"truncated":False}
        elif path=="/workspace/search": data={"query":query["q"][0],"entries":[{"name":"sample.py","path":"sample.py","kind":"file"}],"truncated":False}
        elif path=="/workspace/file": data={"path":"sample.py","size":12,"kind":"text","text":"print('ok')\n"}
        elif path=="/workspace/diff": data={"path":"sample.py","kind":"text","patch":"+print('ok')\n"}
        elif path=="/inspect/topology": data=topology()
        elif path=="/inspect/topology.mmd":
            self.send_response(200);self.end_headers();self.wfile.write(b"graph LR\n  config --> model\n");return
        elif path=="/analysis":
            data=analysis()
        else:
            self.send_error(404);return
        self.send_response(200);self.send_header("Content-Type","application/json");self.end_headers();self.wfile.write(json.dumps(data).encode())

    def do_POST(self):
        if self.headers.get("Authorization") != "Bearer fixture":
            self.send_error(403)
            return
        body=json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        WRITES.append((self.path.split("?")[0],body))
        data={"commit":{"short":"bbbbbbb","summary":body.get("message","")},"changes":changes()} if self.path.startswith("/workspace/commit") else changes()
        self.send_response(200);self.send_header("Content-Type","application/json");self.end_headers();self.wfile.write(json.dumps(data).encode())

    def log_message(self,*_): pass


class Process:
    def __init__(self,package,origin):
        self.manifest=json.loads((package/"extension.json").read_text())
        self.process=subprocess.Popen(self.manifest["command"],cwd=package,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
        self.messages=queue.Queue()
        self.wire=[]
        def read():
            for line in self.process.stdout:
                message=json.loads(line);self.wire.append(message);self.messages.put(message)
            self.messages.put(None)
        self.reader=threading.Thread(target=read,daemon=True);self.reader.start()
        self.send({"type":"hello","protocol":3,"features":["session.settings.v1","widgets.controls.v1"],"workspace":"/workspace","server":{"origin":origin,"token":"fixture"}})
        if self.manifest.get("session_events"):
            self.send({"type":"session_snapshot","session_dir":"/session","state":state(),"entries":[{"role":"user","tool":None,"subagent":None},entry()]})

    def send(self,message):
        self.process.stdin.write(json.dumps(message)+"\n");self.process.stdin.flush()

    def wait(self,predicate):
        while True:
            message=self.messages.get(timeout=10)
            if message is None: raise AssertionError("Extension exited: "+self.process.stderr.read())
            if message.get("type")=="view" and message["root"].get("tone")=="danger":
                raise AssertionError(f"{self.manifest['id']}: {message['root']}")
            if predicate(message): return message

    def render(self,surface):
        argument="1" if self.manifest["id"]=="terminal" else "sample.py"
        view="document:"+argument if surface=="document" else surface
        self.send({"type":"render","surface":surface,"view":view,"session_dir":"/session","argument":argument if surface=="document" else None})
        return self.wait(lambda message:message.get("view")==view and message["type"] in ("view","badge"))

    def action(self,view,action,value):
        self.send({"type":"action","view":view,"action":action,"value":value,"sequence":1})

    def close(self):
        self.process.stdin.close()
        self.process.wait(timeout=10);self.reader.join(timeout=5)
        error=self.process.stderr.read()
        self.process.stdout.close();self.process.stderr.close()
        if self.process.returncode or error: raise AssertionError(f"{self.manifest['id']}: {error}")


def exercise(packages=PACKAGES):
    wire=[]
    with contextlib.ExitStack() as stack:
        server=http.server.ThreadingHTTPServer(("127.0.0.1",0),Api)
        threading.Thread(target=server.serve_forever,daemon=True).start()
        stack.callback(server.server_close);stack.callback(server.shutdown)
        for package in sorted(packages.iterdir()):
            if not (package/"extension.json").exists():continue
            process=Process(package,f"http://127.0.0.1:{server.server_port}")
            try:
                for surface in process.manifest["surfaces"]:process.render(surface)
                identity=package.name
                if identity=="mode":
                    process.action("popup","mode","plan")
                    command=process.wait(lambda message:message["type"]=="set_session")
                    assert command["session_dir"]=="/session" and command["setting"]["value"]=="plan"
                elif identity=="model":
                    process.action("popup","effort",0)
                    assert process.wait(lambda message:message["type"]=="set_session")["setting"]=={"id":"reasoning_effort","value":"effort:low"}
                elif identity=="files":
                    process.action("panel","search-focus",None)
                    assert process.wait(lambda message:message["type"]=="focus")["control"]=="search"
                    process.action("panel","search","sample")
                    process.wait(lambda message:message.get("view")=="panel" and message.get("sequence")==1)
                    process.action("panel","file",{"id":"sample.py","event":"open"})
                    assert process.wait(lambda message:message["type"]=="open_document")["argument"]=="sample.py"
                elif identity=="usage":
                    process.action("page","request","request-1")
                    assert "Причина завершения" in json.dumps(process.wait(lambda message:message.get("view")=="page" and message.get("sequence")==1),ensure_ascii=False)
                elif identity=="analysis":
                    process.action("page","step",3)
                    process.wait(lambda message:message.get("view")=="page" and message.get("sequence")==1)
                elif identity=="git":
                    process.action("panel","tree",{"id":"sample.py","event":"check"})
                    process.wait(lambda message:message.get("view")=="panel" and message.get("sequence")==1)
                    assert WRITES[-1]==("/workspace/stage",{"session_dir":"/session","paths":["sample.py"]}),WRITES
                    process.action("panel","commit","Fixture commit")
                    process.wait(lambda message:message.get("view")=="panel" and message.get("sequence")==1)
                    assert WRITES[-1]==("/workspace/commit",{"session_dir":"/session","message":"Fixture commit","amend":False}),WRITES
            finally:
                process.close()
                wire.extend(process.wire)
    return wire


if __name__=="__main__":
    packages = Path(sys.argv[sys.argv.index("--packages")+1]) if "--packages" in sys.argv else PACKAGES
    messages=exercise(packages)
    if "--wire" in sys.argv:
        for message in messages:print(json.dumps(message,ensure_ascii=False))
    else:print(f"{sum(1 for package in packages.iterdir() if (package/'extension.json').exists())} packages: {len(messages)} real protocol messages checked")
