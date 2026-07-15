#!/usr/bin/env python3
"""Tier-B end-to-end LSP benchmark driver (stdlib only).

Spawns a real `bsl-analyzer-app lsp` over stdio and measures what the
in-process harness cannot: spawn->initializeResponse, spawn->workspace-ready
(the workspace-load progress `end`; requests before it are declined with
ContentModified), per-request wall latency including the task pool and
serialization, and the server's RSS timeline via a 1 Hz `/proc` sampler.

Diagnostics profiles (separate runs, never mixed):
  push              production config as-is: didOpen -> publishDiagnostics.
  pull              a *shadow workspace* (tempdir with the config copied and
                    `[features] workspace_diagnostics` forced on, everything
                    else symlinked) so the real workspace's config is never
                    touched — verified by hash before/after:
                    didOpen -> textDocument/diagnostic.
  workspace-stream  pull shadow + one workspace/diagnostic request with
                    partial-result streaming, then a cancelled second request.

Outputs under --out: run.json (timings, per-file latencies, errors) and
rss.csv (t_s,rss_kb).
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import sys
import tempfile
import threading
import time
import tomllib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lsp_client import CONTENT_MODIFIED, LspClient, LspError, RequestTimeout  # noqa: E402

CONFIG_NAME = "bsl-analyzer.toml"


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(65536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def source_root(workspace):
    config = os.path.join(workspace, CONFIG_NAME)
    if os.path.exists(config):
        with open(config, "rb") as fh:
            root = tomllib.load(fh).get("source", {}).get("root")
        if root:
            return os.path.join(workspace, root)
    return workspace


def pick_bsl_files(workspace, count):
    files = []
    for dirpath, dirnames, filenames in os.walk(source_root(workspace)):
        dirnames.sort()
        for name in sorted(filenames):
            if name.lower().endswith(".bsl"):
                files.append(os.path.join(dirpath, name))
                if len(files) >= count:
                    return files
    return files


def make_shadow_workspace(workspace, scope):
    """Tempdir mirroring `workspace` via symlinks, with pull diagnostics
    forced on in a *copied* config. The original config is never modified.
    The tempdir is removed on any failure inside this builder."""
    shadow = tempfile.mkdtemp(prefix="bsl-bench-shadow-")
    try:
        return _fill_shadow_workspace(shadow, workspace, scope)
    except BaseException:
        shutil.rmtree(shadow, ignore_errors=True)
        raise


def _fill_shadow_workspace(shadow, workspace, scope):
    for entry in sorted(os.listdir(workspace)):
        if entry == CONFIG_NAME:
            continue
        os.symlink(os.path.join(workspace, entry), os.path.join(shadow, entry))

    original = os.path.join(workspace, CONFIG_NAME)
    text = ""
    if os.path.exists(original):
        with open(original, encoding="utf-8") as fh:
            text = fh.read()
    # Drop any existing key first: serde treats the camelCase and snake_case
    # spellings as one field, and a duplicate makes the whole config invalid —
    # which bsl-analyzer silently degrades to defaults (pull off). An inline
    # `features = { … }` table is dropped wholesale for the same reason (the
    # shadow only needs the pull scope; other feature toggles keep defaults).
    text = "\n".join(
        line
        for line in text.splitlines()
        if not line.strip().startswith(
            ("workspace_diagnostics", "workspaceDiagnostics", "features =", "features=")
        )
    )
    if "[features]" in text:
        text = text.replace(
            "[features]", f'[features]\nworkspace_diagnostics = "{scope}"', 1
        )
    else:
        text += f'\n[features]\nworkspace_diagnostics = "{scope}"\n'
    shadow_config = os.path.join(shadow, CONFIG_NAME)
    with open(shadow_config, "w", encoding="utf-8") as fh:
        fh.write(text)
    with open(shadow_config, "rb") as fh:
        parsed = tomllib.load(fh)  # the munged config must still parse…
    if parsed.get("features", {}).get("workspace_diagnostics") != scope:
        raise SystemExit("bench_lsp: shadow config did not end up with the pull scope set")
    return shadow


class RssSampler:
    """1 Hz VmRSS timeline of the server process -> CSV."""

    def __init__(self, pid, out_path):
        self.pid = pid
        self.out_path = out_path
        self.rows = []
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._loop, daemon=True)
        self._t0 = time.monotonic()
        self._thread.start()

    def _read_rss_kb(self):
        try:
            with open(f"/proc/{self.pid}/status", encoding="ascii") as fh:
                for line in fh:
                    if line.startswith("VmRSS:"):
                        return int(line.split()[1])
        except OSError:
            return None
        return None

    def _loop(self):
        while not self._stop.is_set():
            rss = self._read_rss_kb()
            if rss is not None:
                self.rows.append((time.monotonic() - self._t0, rss))
            self._stop.wait(1.0)

    def stop(self):
        self._stop.set()
        self._thread.join(5)
        with open(self.out_path, "w", encoding="ascii") as fh:
            fh.write("t_s,rss_kb\n")
            for t, rss in self.rows:
                fh.write(f"{t:.1f},{rss}\n")


def uri_of(path):
    return "file://" + os.path.abspath(path)


def client_capabilities(profile):
    capabilities = {
        "window": {"workDoneProgress": True},
        "textDocument": {"publishDiagnostics": {}},
        "workspace": {"configuration": True},
    }
    if profile in ("pull", "workspace-stream"):
        capabilities["textDocument"]["diagnostic"] = {"dynamicRegistration": False}
        capabilities["workspace"]["diagnostics"] = {"refreshSupport": True}
    return capabilities


def wait_ready_with_probe(client, probe_uri, boot_timeout, run):
    """Primary readiness = progress `end`; fallback = retry a cheap request
    until it stops being declined with ContentModified."""
    if client.wait_ready(boot_timeout):
        run["timings"]["ready_via"] = "progress_end"
        return True
    deadline = time.monotonic() + boot_timeout
    while time.monotonic() < deadline:
        try:
            response = client.request(
                "textDocument/documentSymbol",
                {"textDocument": {"uri": probe_uri}},
                timeout=10.0,
            )
        except RequestTimeout:
            continue
        # Only a successful answer proves readiness: a hard error (invalid
        # params, internal failure) is not "the workspace finished loading".
        if response.ok:
            run["timings"]["ready_via"] = "probe"
            return True
        if response.error_code != CONTENT_MODIFIED:
            raise SystemExit(f"bench_lsp: readiness probe failed hard: {response.error}")
        time.sleep(0.5)
    return False


def run_profile(args, workspace, run):
    files = pick_bsl_files(workspace, args.files)
    if not files:
        raise SystemExit(f"bench_lsp: no .bsl files under {workspace}")

    # The client spawns with cwd=workspace, so a relative binary path would
    # resolve against the workspace, not the invoker's cwd.
    binary = os.path.abspath(args.binary)
    client = LspClient([binary, "lsp"], cwd=workspace, default_timeout=args.timeout)
    sampler = RssSampler(client.proc.pid, os.path.join(args.out, "rss.csv"))
    try:
        response = client.initialize(uri_of(workspace), client_capabilities(args.profile))
        if not response.ok:
            raise SystemExit(f"bench_lsp: initialize failed: {response.error}")
        run["timings"]["spawn_to_initialize_s"] = time.monotonic() - client.spawn_t

        if args.profile in ("pull", "workspace-stream"):
            # End-to-end proof that the shadow config took effect: pull is
            # opt-in, and without it the server advertises no provider and the
            # run would silently measure nothing.
            provider = (response.result or {}).get("capabilities", {}).get("diagnosticProvider")
            if provider is None:
                raise SystemExit(
                    "bench_lsp: server did not advertise diagnosticProvider — "
                    "pull diagnostics are not enabled in the shadow workspace"
                )

        if not wait_ready_with_probe(client, uri_of(files[0]), args.boot_timeout, run):
            raise SystemExit("bench_lsp: workspace never became ready")
        run["timings"]["spawn_to_ready_s"] = time.monotonic() - client.spawn_t

        for path in files:
            uri = uri_of(path)
            with open(path, encoding="utf-8-sig", errors="replace") as fh:
                text = fh.read()
            entry = {"uri": uri}
            t_open = time.monotonic()
            client.notify(
                "textDocument/didOpen",
                {
                    "textDocument": {
                        "uri": uri,
                        "languageId": "bsl",
                        "version": 1,
                        "text": text,
                    }
                },
            )
            if args.profile == "push":
                message = client.wait_notification(
                    "textDocument/publishDiagnostics",
                    lambda p, uri=uri: p.get("uri") == uri,
                    timeout=args.timeout,
                )
                entry["didopen_to_publish_s"] = time.monotonic() - t_open
                entry["diagnostics"] = len(message["params"]["diagnostics"])
            else:
                pulled = client.request(
                    "textDocument/diagnostic", {"textDocument": {"uri": uri}}
                )
                entry["pull_s"] = pulled.elapsed_s
                if pulled.ok:
                    entry["diagnostics"] = len(
                        (pulled.result or {}).get("items", [])
                    )
                else:
                    entry["error"] = pulled.error
                    run["errors"].append({"uri": uri, "error": pulled.error})

            for method, params in (
                ("textDocument/documentSymbol", {"textDocument": {"uri": uri}}),
                ("textDocument/semanticTokens/full", {"textDocument": {"uri": uri}}),
            ):
                feature = client.request(method, params)
                entry[method.rsplit("/", 1)[-1] + "_s"] = feature.elapsed_s
                if not feature.ok:
                    run["errors"].append({"uri": uri, "method": method, "error": feature.error})
            run["files"].append(entry)

        if args.profile == "workspace-stream":
            run["stream"] = run_stream(client, args)

        code = client.shutdown()
        run["timings"]["server_exit_code"] = code
    finally:
        sampler.stop()
        client.close()


def run_stream(client, args):
    """workspace/diagnostic acceptance: partial chunks arrive and accumulate,
    the final report is well-formed, and cancellation actually cancels."""
    outcome = {}
    token = "bench-stream"
    request_id, waiter = client.request_async(
        "workspace/diagnostic", {"previousResultIds": [], "partialResultToken": token}
    )
    response = client.wait_response(request_id, waiter, timeout=args.timeout)
    chunks = [
        n
        for n in client.notifications_snapshot()
        if n.get("method") == "$/progress"
        and (n.get("params") or {}).get("token") == token
    ]
    outcome["chunks"] = len(chunks)
    outcome["final_ok"] = response.ok
    outcome["final_items"] = len((response.result or {}).get("items", [])) if response.ok else None

    cancel_id, cancel_waiter = client.request_async(
        "workspace/diagnostic",
        {"previousResultIds": [], "partialResultToken": token + "-cancel"},
    )
    client.cancel(cancel_id)
    try:
        cancelled = client.wait_response(cancel_id, cancel_waiter, timeout=args.timeout)
        outcome["cancel_error_code"] = cancelled.error_code
    except RequestTimeout:
        outcome["cancel_error_code"] = "timeout"
    return outcome


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, help="path to bsl-analyzer-app")
    parser.add_argument("--workspace", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument(
        "--profile", choices=("push", "pull", "workspace-stream"), default="push"
    )
    parser.add_argument("--pull-scope", choices=("all", "extensions"), default="all")
    parser.add_argument("--files", type=int, default=5)
    parser.add_argument("--timeout", type=float, default=120.0)
    parser.add_argument("--boot-timeout", type=float, default=600.0)
    parser.add_argument(
        "--keep-shadow", action="store_true", help="keep the shadow workspace for debugging"
    )
    args = parser.parse_args()

    os.makedirs(args.out, exist_ok=True)
    workspace = os.path.abspath(args.workspace)
    original_config = os.path.join(workspace, CONFIG_NAME)
    config_hash_before = (
        sha256_file(original_config) if os.path.exists(original_config) else None
    )

    run = {
        "profile": args.profile,
        "workspace": workspace,
        "binary": args.binary,
        "timings": {},
        "files": [],
        "errors": [],
    }

    shadow = None
    try:
        target = workspace
        if args.profile in ("pull", "workspace-stream"):
            shadow = make_shadow_workspace(workspace, args.pull_scope)
            run["shadow_workspace"] = shadow
            target = shadow
        run_profile(args, target, run)
    except LspError as exc:
        run["errors"].append({"fatal": str(exc)})
    finally:
        if shadow and not args.keep_shadow:
            shutil.rmtree(shadow, ignore_errors=True)
        if config_hash_before is not None:
            config_hash_after = sha256_file(original_config)
            run["config_untouched"] = config_hash_before == config_hash_after
            if not run["config_untouched"]:
                run["errors"].append({"fatal": "workspace config hash changed during the run"})

    with open(os.path.join(args.out, "run.json"), "w", encoding="utf-8") as fh:
        json.dump(run, fh, ensure_ascii=False, indent=2)
    print(json.dumps(run["timings"], indent=2))
    sys.exit(1 if run["errors"] else 0)


if __name__ == "__main__":
    main()
