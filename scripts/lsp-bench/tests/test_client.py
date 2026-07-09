"""Transcript tests for lsp_client against the scripted fake server."""

import json
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from lsp_client import (  # noqa: E402
    CONTENT_MODIFIED,
    REQUEST_CANCELLED,
    LspClient,
    ProtocolError,
    RequestTimeout,
)

FAKE_SERVER = os.path.join(os.path.dirname(__file__), "fake_server.py")


class FakeServerCase(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.log_path = os.path.join(self.tmp.name, "received.jsonl")

    def tearDown(self):
        self.client.close()
        self.tmp.cleanup()

    def spawn(self, scenario) -> LspClient:
        self.client = LspClient(
            [sys.executable, FAKE_SERVER, scenario, self.log_path], default_timeout=10.0
        )
        return self.client

    def received(self):
        with open(self.log_path, encoding="utf-8") as fh:
            return [json.loads(line) for line in fh if line.strip()]

    def test_happy_handshake_auto_replies_and_readiness(self):
        client = self.spawn("happy")
        response = client.initialize("file:///ws")
        self.assertTrue(response.ok)
        self.assertTrue(client.wait_ready(5.0), "progress `end` must mark readiness")

        hover = client.request(
            "textDocument/hover",
            {"textDocument": {"uri": "file:///a.bsl"}, "position": {"line": 0, "character": 0}},
        )
        self.assertTrue(hover.ok)
        self.assertGreater(hover.elapsed_s, 0)

        client.wait_notification(
            "textDocument/publishDiagnostics", lambda p: p["uri"] == "file:///a.bsl", timeout=5.0
        )
        self.assertEqual(client.shutdown(), 0)

        # The client must have answered every server->client request.
        replies = {m["id"]: m for m in self.received() if "id" in m and "method" not in m}
        for request_id in (100, 101, 102, 104):
            self.assertIn(request_id, replies, f"server request {request_id} must be answered")
            self.assertNotIn("error", replies[request_id])
        self.assertEqual(
            replies[103]["result"],
            [None, None],
            "workspace/configuration must be answered per item",
        )
        seen = {r["method"] for r in client.server_requests}
        self.assertIn("workspace/semanticTokens/refresh", seen)
        self.assertIn("workspace/diagnostic/refresh", seen)

    def test_request_timeout_raises(self):
        client = self.spawn("timeout")
        client.initialize("file:///ws")
        with self.assertRaises(RequestTimeout):
            client.request("textDocument/hover", {}, timeout=0.4)

    def test_malformed_frame_surfaces_as_protocol_error(self):
        client = self.spawn("malformed")
        client.initialize("file:///ws")
        deadline = 5.0
        client._closed.wait(deadline)
        self.assertTrue(client.protocol_errors, "garbage frame must be recorded")
        with self.assertRaises((ProtocolError, RequestTimeout)):
            client.request("textDocument/hover", {}, timeout=1.0)

    def test_content_modified_is_a_response_not_an_exception(self):
        client = self.spawn("content_modified")
        client.initialize("file:///ws")
        hover = client.request("textDocument/hover", {})
        self.assertFalse(hover.ok)
        self.assertEqual(hover.error_code, CONTENT_MODIFIED)

    def test_cancellation_roundtrip(self):
        client = self.spawn("cancel")
        client.initialize("file:///ws")
        request_id, waiter = client.request_async("textDocument/hover", {})
        client.cancel(request_id)
        response = client.wait_response(request_id, waiter, timeout=5.0)
        self.assertEqual(response.error_code, REQUEST_CANCELLED)

    def test_workspace_diagnostic_stream_chunks_and_empty_final(self):
        client = self.spawn("stream")
        client.initialize("file:///ws")
        token = "stream-token"
        request_id, waiter = client.request_async(
            "workspace/diagnostic", {"previousResultIds": [], "partialResultToken": token}
        )
        response = client.wait_response(request_id, waiter, timeout=5.0)
        self.assertTrue(response.ok)
        self.assertEqual(response.result, {"items": []}, "final report must be empty")
        chunks = [
            n["params"]["value"]["items"]
            for n in client.notifications_snapshot()
            if n.get("method") == "$/progress" and (n.get("params") or {}).get("token") == token
        ]
        self.assertEqual(len(chunks), 2, "both partial chunks must arrive")
        uris = [item["uri"] for chunk in chunks for item in chunk]
        self.assertEqual(uris, ["file:///first.bsl", "file:///second.bsl"])

    def test_workspace_diagnostic_stream_cancellation(self):
        client = self.spawn("stream_cancel")
        client.initialize("file:///ws")
        token = "cancel-token"
        request_id, waiter = client.request_async(
            "workspace/diagnostic", {"previousResultIds": [], "partialResultToken": token}
        )
        client.wait_notification(
            "$/progress", lambda p: p.get("token") == token, timeout=5.0
        )
        client.cancel(request_id)
        response = client.wait_response(request_id, waiter, timeout=5.0)
        self.assertEqual(response.error_code, REQUEST_CANCELLED)


if __name__ == "__main__":
    unittest.main()
