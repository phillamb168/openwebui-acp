"""Deterministic, disposable upstream provider + MCP fixture. No production data.

Open WebUI executes its real tool loop. This server merely emits scripted provider
responses and a harmless MCP inventory result; it is never part of the adapter.
"""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json, time, uuid, hashlib
from threading import Lock

STATE = {"calls": [], "mcp_calls": 0, "skill_id": None, "knowledge_id": None, "slow_seconds": 4}
LOCK = Lock()

def choose(body):
    messages = body.get("messages", [])
    text = "\n".join(str(m.get("content", "")) for m in messages)
    tools = {t["function"]["name"]: t["function"] for t in body.get("tools", [])}
    last_user = next((str(m.get("content", "")) for m in reversed(messages) if m.get("role") == "user"), "")
    last_user_index = next((i for i in range(len(messages)-1, -1, -1) if messages[i].get("role") == "user"), -1)
    tool_messages = [m for m in messages[last_user_index+1:] if m.get("role") == "tool"]
    directives = ["CHECK_TOOL", "CHECK_MCP", "CHECK_SKILL", "CHECK_KNOWLEDGE", "CHECK_PLAIN", "CHECK_CONTEXT", "CHECK_LITERAL", "CHECK_SLOW"]
    requested = max(directives, key=last_user.rfind) if any(d in last_user for d in directives) else None
    target = None
    if requested == "CHECK_LITERAL":
        return {"role": "assistant", "content": "Literal: @Fixture denied and nostr:npub1untrusted"}
    if requested == "CHECK_SLOW":
        time.sleep(STATE["slow_seconds"])
        return {"role": "assistant", "content": "SLOW-FIXTURE-COMPLETE"}
    if requested == "CHECK_TOOL": target = next((n for n in tools if "fixture_value" in n), None)
    elif requested == "CHECK_MCP": target = next((n for n in tools if "inventory_lookup" in n), None)
    elif requested == "CHECK_SKILL": target = next((n for n in tools if "view_skill" in n), None)
    elif requested == "CHECK_KNOWLEDGE": target = next((n for n in tools if "query_knowledge" in n), None)
    if target and not tool_messages:
        props = tools[target].get("parameters", {}).get("properties", {})
        arguments = {}
        for key, prop in props.items():
            if "skill" in key or (requested == "CHECK_SKILL" and key == "id"): value = STATE["skill_id"]
            elif "knowledge" in key: value = [STATE["knowledge_id"]] if prop.get("type") == "array" else STATE["knowledge_id"]
            elif "quer" in key: value = ["What is the archive verification code?"] if prop.get("type") == "array" else "What is the archive verification code?"
            elif prop.get("type") == "integer": value = 5
            elif prop.get("type") == "number": value = 0.1
            elif prop.get("type") == "boolean": value = False
            elif key == "item": value = "notebook"
            else: value = "fixture"
            arguments[key] = value
        return {"role": "assistant", "content": None, "tool_calls": [{"id": "call_" + uuid.uuid4().hex[:12], "type": "function", "function": {"name": target, "arguments": json.dumps(arguments)}}]}
    if tool_messages:
        result = "\n".join(str(m.get("content", "")) for m in tool_messages)
        return {"role": "assistant", "content": "SERVER_TOOL_RESULT: " + result}
    if requested == "CHECK_PLAIN":
        marker = "PLAIN-SYSTEM-VERIFIED" if "SYSTEM_FIXTURE_LANTERN" in text else "MISSING_SYSTEM"
        return {"role": "assistant", "content": marker}
    if requested == "CHECK_CONTEXT":
        return {"role": "assistant", "content": "ORCHARD" if "ORCHARD" in text else "MISSING_CONTEXT"}
    if "CHECK_" in last_user:
        return {"role": "assistant", "content": "NO_MATCHING_TOOL: " + ",".join(tools)}
    return {"role": "assistant", "content": "ORCHARD"}

class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def log_message(self, *_): pass
    def response(self, value, status=200):
        data = json.dumps(value).encode()
        self.send_response(status); self.send_header("Content-Type", "application/json"); self.send_header("Content-Length", str(len(data))); self.end_headers(); self.wfile.write(data)
    def do_GET(self):
        if self.path in ("/v1/models", "/models"):
            return self.response({"object": "list", "data": [{"id": "fixture-model", "object": "model", "owned_by": "fixture"}]})
        if self.path == "/evidence":
            with LOCK: return self.response(STATE)
        self.response({"error": "not found"}, 404)
    def do_POST(self):
        size = int(self.headers.get("Content-Length", 0))
        if size > 20 * 1024 * 1024: return self.response({"error": "too large"}, 413)
        body = json.loads(self.rfile.read(size) or b"{}")
        if self.path == "/control":
            with LOCK:
                for key in ("skill_id", "knowledge_id", "slow_seconds"):
                    if key in body: STATE[key] = body[key]
            return self.response({"ok": True})
        if self.path == "/mcp":
            method = body.get("method"); result = {}
            if method == "initialize": result = {"protocolVersion": body.get("params", {}).get("protocolVersion", "2025-03-26"), "capabilities": {"tools": {}}, "serverInfo": {"name": "fixture-inventory", "version": "1"}}
            elif method == "tools/list": result = {"tools": [{"name": "inventory_lookup", "description": "Read a synthetic inventory count", "inputSchema": {"type": "object", "properties": {"item": {"type": "string"}}, "required": ["item"]}}]}
            elif method == "tools/call":
                with LOCK: STATE["mcp_calls"] += 1
                result = {"content": [{"type": "text", "text": "INVENTORY-MARKER-4821: 7 notebooks"}], "isError": False}
            elif "id" not in body:
                self.send_response(202); self.send_header("Content-Length", "0"); self.end_headers(); return
            return self.response({"jsonrpc": "2.0", "id": body.get("id"), "result": result})
        if self.path == "/v1/embeddings":
            inputs = body.get("input", []); inputs = inputs if isinstance(inputs, list) else [inputs]
            vectors = [{"object": "embedding", "index": i, "embedding": [1.0] + [0.1] * 7} for i, _ in enumerate(inputs)]
            return self.response({"object": "list", "data": vectors, "model": "fixture-embedding", "usage": {"prompt_tokens": 1, "total_tokens": 1}})
        if self.path not in ("/v1/chat/completions", "/chat/completions"): return self.response({"error": "not found"}, 404)
        with LOCK:
            STATE["calls"].append({"model": body.get("model"), "stream": body.get("stream"), "tools": [x["function"]["name"] for x in body.get("tools", [])], "roles": [x.get("role") for x in body.get("messages", [])], "temperature": body.get("temperature"), "reasoning_effort": body.get("reasoning_effort"), "system_present": "SYSTEM_FIXTURE_LANTERN" in json.dumps(body.get("messages", []))})
        message = choose(body); finish = "tool_calls" if message.get("tool_calls") else "stop"
        result = {"id": "fixture-" + uuid.uuid4().hex, "object": "chat.completion", "created": int(time.time()), "model": "fixture-model", "choices": [{"index": 0, "message": message, "finish_reason": finish}], "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}}
        if not body.get("stream"): return self.response(result)
        delta = dict(message)
        if "tool_calls" in delta:
            delta["tool_calls"] = [dict(call, index=i) for i, call in enumerate(delta["tool_calls"])]
        chunk = {"id": result["id"], "object": "chat.completion.chunk", "created": result["created"], "model": "fixture-model", "choices": [{"index": 0, "delta": delta, "finish_reason": None}]}
        end = dict(chunk, choices=[{"index": 0, "delta": {}, "finish_reason": finish}], usage=result["usage"])
        data = ("data: " + json.dumps(chunk) + "\n\ndata: " + json.dumps(end) + "\n\ndata: [DONE]\n\n").encode()
        self.send_response(200); self.send_header("Content-Type", "text/event-stream"); self.send_header("Content-Length", str(len(data))); self.end_headers(); self.wfile.write(data)

ThreadingHTTPServer(("0.0.0.0", 8000), Handler).serve_forever()
