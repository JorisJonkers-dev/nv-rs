#!/usr/bin/env python3
"""Check the federation path of rewrite_title.py against a local mock of GitHub's OIDC endpoint and Anthropic's API."""
import http.server
import importlib.util
import json
import os
import threading
from pathlib import Path

seen: list[tuple[str, str, dict]] = []


class H(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_: object) -> None:
        pass

    def out(self, obj: dict) -> None:
        data = json.dumps(obj).encode()
        self.send_response(200)
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self) -> None:  # noqa: N802
        assert self.headers["authorization"] == "Bearer request-token" and "audience=https://api.anthropic.com" in self.path
        self.out({"value": f"jwt-{len(seen)}"})

    def do_POST(self) -> None:  # noqa: N802
        body = json.loads(self.rfile.read(int(self.headers["content-length"])))
        seen.append((self.path, self.headers.get("authorization", ""), body))
        if self.path == "/v1/oauth/token":
            assert body["federation_rule_id"] == "fdrl_1" and body["assertion"].startswith("jwt-")
            return self.out({"access_token": "sk-ant-oat01-test", "expires_in": 600})
        assert self.headers["authorization"] == "Bearer sk-ant-oat01-test" and "x-api-key" not in self.headers
        self.out({"content": [{"type": "text", "text": "fix(engine): correct the flag\nignored second line"}]})


srv = http.server.HTTPServer(("127.0.0.1", 0), H)
threading.Thread(target=srv.serve_forever, daemon=True).start()
base = f"http://127.0.0.1:{srv.server_address[1]}"
os.environ.update(ANTHROPIC_API_BASE=base, ACTIONS_ID_TOKEN_REQUEST_URL=f"{base}/token?x=1", ACTIONS_ID_TOKEN_REQUEST_TOKEN="request-token",
                  ANTHROPIC_FEDERATION_RULE_ID="fdrl_1", ANTHROPIC_ORGANIZATION_ID="org", ANTHROPIC_SERVICE_ACCOUNT_ID="svac_1",
                  ANTHROPIC_WORKSPACE_ID="wrkspc_1")
os.environ.pop("ANTHROPIC_API_KEY", None)
spec = importlib.util.spec_from_file_location("rt", Path(__file__).with_name("rewrite_title.py"))
rt = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rt)
assert rt.ask("Fix the flag (#5)", "body", "a.rs | 2 +-") == "fix(engine): correct the flag"
assert rt.ask("Another", "body", "b.rs | 1 +") == "fix(engine): correct the flag"
exchanges = [s for s in seen if s[0] == "/v1/oauth/token"]
assert len(exchanges) == 2 and exchanges[0][2]["assertion"] != exchanges[1][2]["assertion"], "a fresh token per exchange"
os.environ.pop("ACTIONS_ID_TOKEN_REQUEST_URL")
assert rt.auth_headers() is None, "no credentials outside Actions and without a key: the rule is used"
srv.shutdown()
print("test_wif ok: OIDC token exchanged fresh each time, bearer used, no key header, graceful without credentials")
