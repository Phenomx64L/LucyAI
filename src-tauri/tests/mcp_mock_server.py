"""
Minimal MCP (Model Context Protocol) mock server for Lucy regression tests.

Speaks the JSON-RPC subset Lucy actually uses:
  • initialize           → returns capabilities
  • notifications/initialized (ignored)
  • tools/list           → returns a fixed catalog with one schema'd tool
  • tools/call           → echoes args + counts how many times it was called

Why Python: it's already installed on every Lucy dev/CI machine (Tauri
test runners use it). Spawning a Python subprocess is the cheapest way
to test the full spawn → init → call → close lifecycle without standing
up a full MCP server.

Behavior knobs (env vars):
  MCP_MOCK_SLEEP_MS  — sleep this long before each response (lets the
                       pool test verify session reuse vs spawn cost)
  MCP_MOCK_FAIL      — if "1", reply with error to the first tools/call
"""

import json
import os
import sys
import time

CALL_COUNT = 0


def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


def main():
    sleep_ms = int(os.environ.get("MCP_MOCK_SLEEP_MS", "0"))
    fail_first = os.environ.get("MCP_MOCK_FAIL") == "1"

    while True:
        line = sys.stdin.readline()
        if not line:
            return  # EOF — parent closed us cleanly
        try:
            msg = json.loads(line)
        except Exception:
            continue

        method = msg.get("method")
        rid    = msg.get("id")

        if sleep_ms > 0:
            time.sleep(sleep_ms / 1000.0)

        if method == "initialize":
            send({
                "jsonrpc": "2.0",
                "id":      rid,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities":    {"tools": {}},
                    "serverInfo":      {"name": "lucy-mock", "version": "0.1.0"},
                }
            })
        elif method == "notifications/initialized":
            # Notifications don't get a response.
            continue
        elif method == "tools/list":
            send({
                "jsonrpc": "2.0",
                "id":      rid,
                "result": {
                    "tools": [
                        {
                            "name": "echo",
                            "description": "Echoes the given message back, prefixed with the call counter.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "message": {"type": "string"},
                                    "loud":    {"type": "boolean"},
                                },
                                "required": ["message"],
                            },
                        }
                    ]
                }
            })
        elif method == "tools/call":
            global CALL_COUNT
            CALL_COUNT += 1
            if fail_first and CALL_COUNT == 1:
                send({
                    "jsonrpc": "2.0",
                    "id":      rid,
                    "error":   {"code": -32603, "message": "Mock-injected failure (MCP_MOCK_FAIL=1)"},
                })
                continue
            params = msg.get("params", {})
            args   = params.get("arguments", {})
            msg_text = str(args.get("message", ""))
            if args.get("loud"):
                msg_text = msg_text.upper()
            send({
                "jsonrpc": "2.0",
                "id":      rid,
                "result": {
                    "content": [{"type": "text", "text": f"[{CALL_COUNT}] {msg_text}"}]
                }
            })
        else:
            send({
                "jsonrpc": "2.0",
                "id":      rid,
                "error":   {"code": -32601, "message": f"Method not found: {method}"},
            })


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        pass
