"""Scripted stdio LSP server for lsp_client transcript tests.

Usage: python3 fake_server.py <scenario> <received-log.jsonl>

Every message the fake server receives (including the client's replies to
server->client requests) is appended to the log file as one JSON line, so
tests can assert on the exact wire exchange.

Scenarios:
  happy            full handshake: workDoneProgress/create + $/progress
                   begin/end + both refresh requests + workspace/configuration
                   + registerCapability + publishDiagnostics; answers hover.
  timeout          never answers hover (everything else works).
  malformed        emits a garbage frame after the initialize response.
  content_modified answers hover with error -32801.
  cancel           answers hover only after receiving its $/cancelRequest,
                   with error -32800.
  stream           workspace/diagnostic: two $/progress partial chunks, then
                   an empty final report; honours cancellation with -32800.
"""

import json
import sys
import threading


class Wire:
    def __init__(self, log_path):
        self.stdin = sys.stdin.buffer
        self.stdout = sys.stdout.buffer
        self.log = open(log_path, "a", encoding="utf-8")
        self.lock = threading.Lock()

    def read(self):
        headers = {}
        line = self.stdin.readline()
        if not line:
            return None
        while line not in (b"\r\n", b"\n"):
            key, value = line.decode("ascii").split(":", 1)
            headers[key.strip().lower()] = value.strip()
            line = self.stdin.readline()
            if not line:
                return None
        body = self.stdin.read(int(headers["content-length"]))
        message = json.loads(body.decode("utf-8"))
        self.log.write(json.dumps(message, ensure_ascii=False) + "\n")
        self.log.flush()
        return message

    def send(self, message):
        body = json.dumps(message, ensure_ascii=False).encode("utf-8")
        with self.lock:
            self.stdout.write(b"Content-Length: %d\r\n\r\n" % len(body))
            self.stdout.write(body)
            self.stdout.flush()

    def send_raw(self, data: bytes):
        with self.lock:
            self.stdout.write(data)
            self.stdout.flush()

    def respond(self, request, result=None, error=None):
        message = {"jsonrpc": "2.0", "id": request["id"]}
        if error is not None:
            message["error"] = error
        else:
            message["result"] = result
        self.send(message)


def happy_handshake(wire):
    """The server->client barrage bsl-analyzer produces around startup."""
    wire.send(
        {
            "jsonrpc": "2.0",
            "id": 100,
            "method": "window/workDoneProgress/create",
            "params": {"token": "load-token"},
        }
    )
    wire.send(
        {
            "jsonrpc": "2.0",
            "method": "$/progress",
            "params": {"token": "load-token", "value": {"kind": "begin", "title": "loading"}},
        }
    )
    wire.send(
        {
            "jsonrpc": "2.0",
            "method": "$/progress",
            "params": {"token": "load-token", "value": {"kind": "end"}},
        }
    )
    wire.send({"jsonrpc": "2.0", "id": 101, "method": "workspace/semanticTokens/refresh"})
    wire.send({"jsonrpc": "2.0", "id": 102, "method": "workspace/diagnostic/refresh"})
    wire.send(
        {
            "jsonrpc": "2.0",
            "id": 103,
            "method": "workspace/configuration",
            "params": {"items": [{"section": "a"}, {"section": "b"}]},
        }
    )
    wire.send(
        {
            "jsonrpc": "2.0",
            "id": 104,
            "method": "client/registerCapability",
            "params": {"registrations": []},
        }
    )
    wire.send(
        {
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {"uri": "file:///a.bsl", "diagnostics": []},
        }
    )


def main():
    scenario, log_path = sys.argv[1], sys.argv[2]
    wire = Wire(log_path)
    pending_cancel_target = {}

    while True:
        message = wire.read()
        if message is None:
            return
        method = message.get("method")

        if method == "initialize":
            wire.respond(message, result={"capabilities": {}})
            if scenario == "malformed":
                wire.send_raw(b"THIS IS NOT AN LSP FRAME\r\n\r\n")
                return
            if scenario == "happy":
                happy_handshake(wire)
        elif method == "textDocument/hover":
            if scenario == "timeout":
                continue
            if scenario == "content_modified":
                wire.respond(
                    message, error={"code": -32801, "message": "content modified"}
                )
            elif scenario == "cancel":
                pending_cancel_target[message["id"]] = message
            else:
                wire.respond(message, result={"contents": "ok"})
        elif method == "$/cancelRequest":
            target = (message.get("params") or {}).get("id")
            if target in pending_cancel_target:
                wire.respond(
                    pending_cancel_target.pop(target),
                    error={"code": -32800, "message": "request cancelled"},
                )
        elif method == "workspace/diagnostic":
            token = (message.get("params") or {}).get("partialResultToken")
            if scenario == "stream":
                for name in ("first.bsl", "second.bsl"):
                    wire.send(
                        {
                            "jsonrpc": "2.0",
                            "method": "$/progress",
                            "params": {
                                "token": token,
                                "value": {
                                    "items": [
                                        {
                                            "uri": f"file:///{name}",
                                            "kind": "full",
                                            "items": [],
                                        }
                                    ]
                                },
                            },
                        }
                    )
                wire.respond(message, result={"items": []})
            elif scenario == "stream_cancel":
                wire.send(
                    {
                        "jsonrpc": "2.0",
                        "method": "$/progress",
                        "params": {
                            "token": token,
                            "value": {
                                "items": [
                                    {"uri": "file:///first.bsl", "kind": "full", "items": []}
                                ]
                            },
                        },
                    }
                )
                pending_cancel_target[message["id"]] = message
        elif method == "shutdown":
            wire.respond(message, result=None)
        elif method == "exit":
            return


if __name__ == "__main__":
    main()
