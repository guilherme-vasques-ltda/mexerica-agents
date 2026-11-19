"""Persistent deterministic ACP sessions for routine restart and queue tests."""
import json
import os
import pathlib
import re
import subprocess
import sys
import threading
import time
import uuid

lock = threading.RLock()
sessions_file = pathlib.Path("sessions.json")
sessions = json.loads(sessions_file.read_text()) if sessions_file.exists() else {}
cancel = threading.Event()
servers_by_session = {}
# I-023: the main session and the previous routine session, to call from a session that isn't the run's own.
other_sessions = {"main": None, "previous": None, "current": None}


def send(value):
    with lock:
        print(json.dumps({"jsonrpc": "2.0", **value}), flush=True)


def log(event, sid, text):
    with lock:
        with pathlib.Path("events.jsonl").open("a") as output:
            output.write(json.dumps({"event": event, "session": sid, "text": text}) + "\n")


def prompt(request, stop):
    sid = request["params"]["sessionId"]
    text = request["params"]["prompt"][0]["text"]
    resumed = "[Donnabot restarted" in text
    with lock:
        if resumed:
            text = sessions[sid]
        else:
            sessions[sid] = text
            temp = sessions_file.with_suffix(".tmp")
            temp.write_text(json.dumps(sessions))
            temp.replace(sessions_file)
    log("resumed" if resumed else "started", sid, text)
    with lock:
        if "Scheduled routine:" not in text:
            other_sessions["main"] = other_sessions["main"] or sid
        elif other_sessions["current"] != sid:
            other_sessions["previous"], other_sessions["current"] = other_sessions["current"], sid
    if "VERIFY_ROUTINE_TOOLS" in text and "Scheduled routine:" not in text:
        try:
            verify_routine_tools(sid)
            log("tools_verified", sid, "Routine MCP lifecycle")
        except Exception as error:
            log("tools_failed", sid, repr(error))
            send({"id": request["id"], "error": {"code": -32000, "message": str(error)}})
            return
    block = re.search(r"BLOCK\[([^]]+)\]", text)
    if block:
        while not pathlib.Path("release-" + block[1]).exists():
            if stop.wait(0.01) and "IGNORE_CANCEL" not in text:
                send({"id": request["id"], "result": {"stopReason": "cancelled"}})
                return
    fail = re.search(r"FAIL_ONCE\[([^]]+)\]", text)
    if fail and not pathlib.Path("failed-" + fail[1]).exists():
        pathlib.Path("failed-" + fail[1]).touch()
        log("failed", sid, text)
        send({"id": request["id"], "error": {"code": -32000, "message": "transient fixture failure"}})
        return
    for via, outcome in re.findall(r"DECLARE_VIA\[(main|previous)\]\[([a-z_]+)\]", text):
        log("declared_via_" + via, sid, json.dumps(declare(other_sessions[via], outcome, listed=via == "previous")))
    for outcome in re.findall(r"DECLARE\[([a-z_]+)\]", text):
        log("declared", sid, json.dumps(declare(sid, outcome)))
    log("finished", sid, text)
    reply = "(pass)" if "SILENT" in text else "Routine completed successfully."
    send({"method": "session/update", "params": {"sessionId": sid, "update": {
        "sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": reply}}}})
    send({"id": request["id"], "result": {"stopReason": "end_turn"}})


def declare(sid, outcome, listed=True):
    """I-023: the routine session's own routines server lists and accepts report_routine_result."""
    server = next(s for s in servers_by_session[sid] if s["name"] == "routines")
    with subprocess.Popen([server["command"], *server["args"]], stdin=subprocess.PIPE,
                          stdout=subprocess.PIPE, text=True,
                          env={**os.environ, **{e["name"]: e["value"] for e in server.get("env", [])}}) as mcp:
        def rpc(identifier, method, params):
            mcp.stdin.write(json.dumps({"jsonrpc": "2.0", "id": identifier,
                                       "method": method, "params": params}) + "\n")
            mcp.stdin.flush()
            return json.loads(mcp.stdout.readline())["result"]

        rpc(1, "initialize", {"protocolVersion": "2025-06-18"})
        names = {t["name"] for t in rpc(2, "tools/list", {})["tools"]}
        assert ("report_routine_result" in names) == listed, names
        arguments = {"outcome": outcome}
        if outcome != "no_news":
            arguments["summary"] = "Fixture " + outcome
        result = rpc(3, "tools/call", {"name": "report_routine_result", "arguments": arguments})
        mcp.stdin.close()
        return result


def verify_routine_tools(sid):
    server = next(s for s in servers_by_session[sid] if s["name"] == "routines")
    with subprocess.Popen([server["command"], *server["args"]], stdin=subprocess.PIPE,
                          stdout=subprocess.PIPE, text=True,
                          env={**os.environ, **{e["name"]: e["value"] for e in server.get("env", [])}}) as mcp:
        identifier = 0

        def rpc(method, params):
            nonlocal identifier
            identifier += 1
            mcp.stdin.write(json.dumps({"jsonrpc": "2.0", "id": identifier,
                                       "method": method, "params": params}) + "\n")
            mcp.stdin.flush()
            response = json.loads(mcp.stdout.readline())
            assert "error" not in response, response
            return response["result"]

        def call(name, arguments):
            result = rpc("tools/call", {"name": name, "arguments": arguments})
            assert not result.get("isError"), result
            return json.loads(result["content"][0]["text"])

        init = rpc("initialize", {"protocolVersion": "2025-06-18"})
        assert init["serverInfo"]["name"] == "codync-routines"
        names = {t["name"] for t in rpc("tools/list", {})["tools"]}
        assert names == {"list_routines", "save_routine", "set_routine_enabled",
                         "delete_routine", "run_routine", "routine_webhook", "ajuda_instrucoes",
                         "duplicate_routine"}, names
        guide = rpc("tools/call", {"name": "ajuda_instrucoes", "arguments": {}})
        assert not guide.get("isError"), guide
        assert "Keep secrets out of routine instructions" in guide["content"][0]["text"]
        assert "For recurring clock schedules" in guide["content"][0]["text"]
        assert not call("list_routines", {})["routines"]
        invalid = rpc("tools/call", {"name": "save_routine", "arguments": {
            "name": "Invalid", "instruction": "Check", "triggers": [{"type": "interval", "seconds": 0}]}})
        assert invalid.get("isError"), invalid
        body = {"name": "Created through MCP", "instruction": "MCP_TASK_RESULT",
                "triggers": [{"type": "webhook"}], "enabled": False}
        routine = call("save_routine", body)["routine"]
        identifier_value = routine["id"]
        assert routine["enabled"] is False
        assert "webhookKey" not in routine
        credentials = call("routine_webhook", {"id": identifier_value})
        assert credentials["url"] is None  # the test host runs with the cloud off
        assert credentials["localUrl"].endswith("/hooks/routines/" + identifier_value)
        assert len(credentials["key"]) >= 32
        rotated = call("routine_webhook", {"id": identifier_value, "rotate": True})
        assert rotated["key"] != credentials["key"] and len(rotated["key"]) >= 32
        body.update({"id": identifier_value, "name": "Edited through MCP"})
        call("save_routine", body)
        call("set_routine_enabled", {"id": identifier_value, "enabled": True})
        call("set_routine_enabled", {"id": identifier_value, "enabled": False})
        # D544: criar exige ausência de id; null não pode virar criação implícita.
        ambiguous = rpc("tools/call", {"name": "save_routine", "arguments": {
            **body, "id": None, "name": "Temporary"}})
        assert ambiguous.get("isError"), ambiguous
        # A segunda rotina continua provando criação e exclusão pelo MCP real.
        extra_body = {key: value for key, value in body.items() if key != "id"}
        extra = call("save_routine", {**extra_body, "name": "Temporary"})["routine"]
        call("delete_routine", {"id": extra["id"]})
        listed = call("list_routines", {})["routines"]
        assert len(listed) == 1 and listed[0]["name"] == "Edited through MCP"
        duplicated = call("duplicate_routine", {"id": identifier_value})["routine"]
        assert duplicated["id"] != identifier_value
        copies = [r for r in call("list_routines", {})["routines"] if r["id"] == duplicated["id"]]
        assert len(copies) == 1
        assert copies[0]["name"] == "Edited through MCP (cópia)"
        assert copies[0]["enabled"] is False
        call("delete_routine", {"id": duplicated["id"]})
        call("run_routine", {"id": identifier_value})
        mcp.stdin.close()


for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    if method == "initialize":
        log("initialize", "", str(os.getpid()))
        capabilities = {"loadSession": not pathlib.Path("no-load").exists(), "sessionCapabilities": {"close": {}}}
        send({"id": request["id"], "result": {"agentCapabilities": capabilities}})
    elif method == "session/new":
        # I-023: an engine that reuses one ACP id for two slots (ambiguous identity).
        sid = "acp-repetida" if pathlib.Path("same-acp").exists() else str(uuid.uuid4())
        servers_by_session[sid] = request["params"]["mcpServers"]
        send({"id": request["id"], "result": {"sessionId": sid}})
    elif method == "session/load":
        sid = request["params"]["sessionId"]
        if sid in sessions:
            servers_by_session[sid] = request["params"]["mcpServers"]
            log("loaded", sid, "")
            send({"id": request["id"], "result": {"sessionId": sid}})
        else:
            send({"id": request["id"], "error": {"code": -32000, "message": "unknown session"}})
    elif method == "session/prompt":
        cancel = threading.Event()
        threading.Thread(target=prompt, args=(request, cancel), daemon=True).start()
    elif method == "session/cancel":
        cancel.set()
    elif method == "session/close":
        log("closed", request["params"]["sessionId"], "")
        send({"id": request["id"], "result": {}})
    elif "id" in request:
        send({"id": request["id"], "result": {}})
