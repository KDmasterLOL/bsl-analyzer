"""Minimal LSP stdio client with a real protocol pump (stdlib only).

A naive request/response loop cannot benchmark bsl-analyzer: the server
answers `initialize` before the workspace is loaded, declines early requests
with ContentModified, and issues its own server->client requests
(client/registerCapability, workspace/configuration,
window/workDoneProgress/create, workspace/semanticTokens/refresh,
workspace/diagnostic/refresh) that must be answered or the connection stalls.

This client runs a dedicated reader thread that demultiplexes responses by id,
auto-replies to server requests, records every message into a transcript, and
exposes readiness as "the first $/progress `end`" (the workspace-load signal).
"""

from __future__ import annotations

import itertools
import json
import os
import subprocess
import threading
import time
from collections import deque
from dataclasses import dataclass, field

CONTENT_MODIFIED = -32801
REQUEST_CANCELLED = -32800
METHOD_NOT_FOUND = -32601

AUTO_REPLY_METHODS = (
    "client/registerCapability",
    "workspace/configuration",
    "window/workDoneProgress/create",
    "workspace/semanticTokens/refresh",
    "workspace/diagnostic/refresh",
)


class LspError(Exception):
    pass


class RequestTimeout(LspError):
    pass


class ProtocolError(LspError):
    pass


@dataclass
class Response:
    result: object
    error: dict | None
    elapsed_s: float

    @property
    def ok(self) -> bool:
        return self.error is None

    @property
    def error_code(self) -> int | None:
        return self.error.get("code") if self.error else None


@dataclass
class Transcript:
    """Chronological record of every frame, for tests and post-mortems."""

    events: list = field(default_factory=list)
    lock: threading.Lock = field(default_factory=threading.Lock)

    def add(self, direction: str, message: dict) -> None:
        with self.lock:
            self.events.append(
                {
                    "t": time.monotonic(),
                    "dir": direction,
                    "method": message.get("method"),
                    "id": message.get("id"),
                    "has_error": "error" in message,
                }
            )


class LspClient:
    def __init__(self, cmd, cwd=None, env=None, default_timeout=30.0, stderr=subprocess.DEVNULL):
        self.default_timeout = default_timeout
        self.transcript = Transcript()
        self.protocol_errors: list[str] = []
        self.server_requests: list[dict] = []
        self.notifications: deque = deque()
        self._notify_cond = threading.Condition()
        self._pending: dict[object, "queue_like"] = {}
        self._pending_lock = threading.Lock()
        self._send_lock = threading.Lock()
        self._ids = itertools.count(1)
        self._ready = threading.Event()
        self._closed = threading.Event()
        self.spawn_t = time.monotonic()
        self.proc = subprocess.Popen(
            cmd,
            cwd=cwd,
            env=env if env is not None else os.environ.copy(),
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=stderr,
        )
        self._reader = threading.Thread(target=self._reader_loop, daemon=True)
        self._reader.start()

    # -- framing ------------------------------------------------------------

    def _read_exact(self, n: int) -> bytes:
        chunks = []
        remaining = n
        while remaining > 0:
            chunk = self.proc.stdout.read(remaining)
            if not chunk:
                raise ProtocolError("stream closed mid-frame")
            chunks.append(chunk)
            remaining -= len(chunk)
        return b"".join(chunks)

    def _read_message(self) -> dict | None:
        headers = {}
        line = self.proc.stdout.readline()
        if not line:
            return None  # clean EOF
        while line not in (b"\r\n", b"\n"):
            try:
                key, value = line.decode("ascii").split(":", 1)
            except (UnicodeDecodeError, ValueError) as exc:
                raise ProtocolError(f"malformed header line {line!r}: {exc}") from exc
            headers[key.strip().lower()] = value.strip()
            line = self.proc.stdout.readline()
            if not line:
                raise ProtocolError("stream closed inside headers")
        try:
            length = int(headers["content-length"])
        except (KeyError, ValueError) as exc:
            raise ProtocolError(f"missing/invalid Content-Length in {headers!r}") from exc
        body = self._read_exact(length)
        try:
            return json.loads(body.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            raise ProtocolError(f"malformed JSON body: {exc}") from exc

    def _send(self, message: dict) -> None:
        body = json.dumps(message, ensure_ascii=False).encode("utf-8")
        frame = b"Content-Length: %d\r\n\r\n%s" % (len(body), body)
        with self._send_lock:
            self.proc.stdin.write(frame)
            self.proc.stdin.flush()
        self.transcript.add("out", message)

    # -- reader / demux ------------------------------------------------------

    def _reader_loop(self) -> None:
        try:
            while True:
                message = self._read_message()
                if message is None:
                    break
                self.transcript.add("in", message)
                if "method" in message and "id" in message:
                    self._handle_server_request(message)
                elif "id" in message:
                    self._route_response(message)
                else:
                    self._handle_notification(message)
        except ProtocolError as exc:
            self.protocol_errors.append(str(exc))
        finally:
            self._closed.set()
            self._fail_pending("connection closed")
            with self._notify_cond:
                self._notify_cond.notify_all()

    def _fail_pending(self, reason: str) -> None:
        with self._pending_lock:
            pending, self._pending = self._pending, {}
        for waiter in pending.values():
            waiter["error"] = reason
            waiter["event"].set()

    def _route_response(self, message: dict) -> None:
        with self._pending_lock:
            waiter = self._pending.pop(message["id"], None)
        if waiter is not None:
            waiter["message"] = message
            waiter["event"].set()

    def _handle_server_request(self, message: dict) -> None:
        method = message["method"]
        self.server_requests.append({"method": method, "params": message.get("params")})
        if method == "workspace/configuration":
            items = (message.get("params") or {}).get("items", [])
            result = [None] * len(items)
            reply = {"jsonrpc": "2.0", "id": message["id"], "result": result}
        elif method in AUTO_REPLY_METHODS:
            reply = {"jsonrpc": "2.0", "id": message["id"], "result": None}
        else:
            reply = {
                "jsonrpc": "2.0",
                "id": message["id"],
                "error": {"code": METHOD_NOT_FOUND, "message": f"unhandled: {method}"},
            }
        self._send(reply)

    def _handle_notification(self, message: dict) -> None:
        method = message.get("method")
        if method == "$/progress":
            value = (message.get("params") or {}).get("value") or {}
            if value.get("kind") == "end":
                self._ready.set()
        with self._notify_cond:
            self.notifications.append(message)
            self._notify_cond.notify_all()

    # -- public API -----------------------------------------------------------

    def request_async(self, method: str, params) -> tuple[int, dict]:
        request_id = next(self._ids)
        waiter = {"event": threading.Event(), "message": None, "error": None, "t0": time.monotonic()}
        with self._pending_lock:
            self._pending[request_id] = waiter
        self._send({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params})
        return request_id, waiter

    def wait_response(self, request_id: int, waiter: dict, timeout: float | None = None) -> Response:
        timeout = self.default_timeout if timeout is None else timeout
        if not waiter["event"].wait(timeout):
            with self._pending_lock:
                self._pending.pop(request_id, None)
            raise RequestTimeout(f"request {request_id} timed out after {timeout}s")
        if waiter["error"] is not None:
            raise ProtocolError(waiter["error"])
        message = waiter["message"]
        return Response(
            result=message.get("result"),
            error=message.get("error"),
            elapsed_s=time.monotonic() - waiter["t0"],
        )

    def request(self, method: str, params, timeout: float | None = None) -> Response:
        request_id, waiter = self.request_async(method, params)
        return self.wait_response(request_id, waiter, timeout)

    def notify(self, method: str, params) -> None:
        self._send({"jsonrpc": "2.0", "method": method, "params": params})

    def cancel(self, request_id: int) -> None:
        self.notify("$/cancelRequest", {"id": request_id})

    def notifications_snapshot(self) -> list:
        """Locked copy — the reader thread appends concurrently, and iterating
        the live deque would race (`deque mutated during iteration`)."""
        with self._notify_cond:
            return list(self.notifications)

    def wait_notification(self, method: str, predicate=None, timeout: float | None = None):
        timeout = self.default_timeout if timeout is None else timeout
        deadline = time.monotonic() + timeout
        seen = 0
        with self._notify_cond:
            while True:
                items = list(self.notifications)
                for message in items[seen:]:
                    if message.get("method") == method and (
                        predicate is None or predicate(message.get("params"))
                    ):
                        return message
                seen = len(items)
                remaining = deadline - time.monotonic()
                if remaining <= 0 or self._closed.is_set():
                    raise RequestTimeout(f"no {method} notification within {timeout}s")
                self._notify_cond.wait(remaining)

    def initialize(self, root_uri: str, capabilities: dict | None = None, timeout=None) -> Response:
        params = {
            "processId": os.getpid(),
            "rootUri": root_uri,
            "capabilities": capabilities or {},
            "workspaceFolders": [{"uri": root_uri, "name": "bench"}],
        }
        response = self.request("initialize", params, timeout)
        self.notify("initialized", {})
        return response

    def wait_ready(self, timeout: float) -> bool:
        """True once the server reported the workspace-load progress `end`."""
        return self._ready.wait(timeout)

    def shutdown(self, timeout: float = 10.0) -> int | None:
        try:
            self.request("shutdown", None, timeout)
            self.notify("exit", None)
        except LspError:
            pass
        try:
            code = self.proc.wait(timeout)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            code = self.proc.wait(5)
        self._close_pipes()
        return code

    def close(self) -> None:
        if self.proc.poll() is None:
            self.proc.kill()
            self.proc.wait(5)
        self._close_pipes()

    def _close_pipes(self) -> None:
        for pipe in (self.proc.stdin, self.proc.stdout):
            try:
                if pipe is not None:
                    pipe.close()
            except OSError:
                pass
