#!/usr/bin/env python3
"""Set up Anthropic workload identity federation for this repository's GitHub Actions, so no API key is stored.

Creates (or reuses) in your Anthropic organisation: a federation issuer for GitHub Actions, a service account,
its membership of one workspace, and a federation rule that only the `fork-sync` environment of this repository
on `main` can satisfy. Then stores the (non-secret) identifiers as environment variables of the repository with `gh`.

Usage:  export ANTHROPIC_AUTH_TOKEN=$(ant auth print-credentials --profile admin --access-token)   # see docs/fork/WIF.md
        scripts/fork/setup-anthropic-wif.py --workspace wrkspc_... [--org-id <uuid>] [--dry-run]
        scripts/fork/setup-anthropic-wif.py --selftest        # runs against a local mock, changes nothing

Needs an OAuth token with the org:admin scope (Admin API keys are not accepted). Idempotent: rerunning reuses
what exists. Stdlib only.
"""
import argparse
import json
import os
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request

VERSION = "2026-10-21"
GITHUB_ISSUER = "https://token.actions.githubusercontent.com"
AUDIENCE = "https://api.anthropic.com"


class Api:
    def __init__(self, base: str, token: str, dry: bool) -> None:
        self.base, self.token, self.dry = base.rstrip("/"), token, dry

    def call(self, method: str, path: str, body: dict | None = None) -> dict:
        if self.dry and method != "GET":
            print(f"  [dry-run] {method} {path} {json.dumps(body) if body else ''}")
            return {"id": "dry-run"}
        req = urllib.request.Request(
            f"{self.base}/v1/organizations/{path}", method=method,
            data=json.dumps(body).encode() if body is not None else None,
            headers={"authorization": f"Bearer {self.token}", "anthropic-version": VERSION, "content-type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=60) as resp:  # noqa: S310
                return json.load(resp)
        except urllib.error.HTTPError as err:
            detail = err.read().decode(errors="replace")[:600]
            raise SystemExit(f"{method} {path} failed: HTTP {err.code} {detail}") from err

    def find(self, path: str, field: str, value: str) -> dict | None:
        page = None
        while True:
            q = {"limit": "100"} | ({"page": page} if page else {})
            out = self.call("GET", f"{path}?{urllib.parse.urlencode(q)}")
            for item in out.get("data", []):
                if item.get(field) == value:
                    return item
            page = out.get("next_page")
            if not page:
                return None


def gh_var(repo: str, env: str, name: str, value: str) -> None:
    subprocess.run(["gh", "variable", "set", name, "--env", env, "-R", repo, "--body", value], check=True, stdout=subprocess.DEVNULL)


def run(a: argparse.Namespace, api: Api) -> dict:
    owner, _, repo_name = a.repo.partition("/")
    org_id = a.org_id
    if not org_id:
        org_id = api.call("GET", "me").get("id", "")
    if not org_id:
        sys.exit("cannot read the organisation id: pass --org-id (Console, Settings, Organization)")

    issuer = api.find("federation_issuers", "issuer_url", GITHUB_ISSUER)
    if issuer:
        print(f"issuer: reusing {issuer['id']} ({issuer.get('name')})")
    else:
        issuer = api.call("POST", "federation_issuers", {"name": "github-actions", "issuer_url": GITHUB_ISSUER, "jwks": {"type": "discovery"}})
        print(f"issuer: created {issuer['id']}")

    sa = api.find("service_accounts", "name", a.name)
    if sa:
        print(f"service account: reusing {sa['id']}")
    else:
        sa = api.call("POST", "service_accounts", {"name": a.name, "organization_role": "developer"})
        print(f"service account: created {sa['id']}")
    api.call("POST", f"service_accounts/{sa['id']}/workspaces", {"workspace_id": a.workspace, "workspace_role": "workspace_developer"})
    print(f"service account: member of {a.workspace}")

    rule = api.find("federation_rules", "name", a.name)
    if rule:
        print(f"rule: reusing {rule['id']} (check its match in the Console if you changed --repo or --env)")
    else:
        body = {
            "name": a.name,
            "issuer_id": issuer["id"],
            "match": {
                "subject_prefix": f"repo:{a.repo}:environment:{a.env}",
                "audience": AUDIENCE,
                "claims": {"repository_owner": owner, "repository": a.repo, "ref": "refs/heads/main"},
            },
            "target": {"type": "service_account", "service_account_id": sa["id"]},
            "workspace_id": a.workspace,
            "oauth_scope": "workspace:inference",
            "token_lifetime_seconds": 600,
        }
        try:
            rule = api.call("POST", "federation_rules", body)
        except SystemExit as err:
            if "HTTP 400" not in str(err) and "HTTP 422" not in str(err):
                raise
            print("rule: workspace:inference was refused, falling back to workspace:developer (same access as an API key)")
            rule = api.call("POST", "federation_rules", body | {"oauth_scope": "workspace:developer"})
        print(f"rule: created {rule['id']}")

    ids = {
        "ANTHROPIC_FEDERATION_RULE_ID": rule["id"],
        "ANTHROPIC_ORGANIZATION_ID": org_id,
        "ANTHROPIC_SERVICE_ACCOUNT_ID": sa["id"],
        "ANTHROPIC_WORKSPACE_ID": a.workspace,
    }
    if a.no_github or a.dry_run:
        print("identifiers (not secrets):", json.dumps(ids, indent=2))
    else:
        for k, v in ids.items():
            gh_var(a.repo, a.env, k, v)
        print(f"stored {', '.join(ids)} as variables of the '{a.env}' environment of {a.repo}")
    return ids


def selftest() -> None:
    import http.server
    import threading

    state: dict[str, list] = {"issuers": [], "sas": [], "rules": [], "calls": []}

    class H(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_: object) -> None:
            pass

        def reply(self, obj: dict) -> None:
            data = json.dumps(obj).encode()
            self.send_response(200)
            self.send_header("content-length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def handle_any(self) -> None:
            n = int(self.headers.get("content-length") or 0)
            body = json.loads(self.rfile.read(n)) if n else None
            assert self.headers["authorization"] == "Bearer test-token"
            path = urllib.parse.urlparse(self.path).path.removeprefix("/v1/organizations/")
            state["calls"].append((self.command, path, body))
            if path == "me":
                return self.reply({"id": "11111111-2222-3333-4444-555555555555"})
            kind = {"federation_issuers": "issuers", "service_accounts": "sas", "federation_rules": "rules"}.get(path)
            if kind and self.command == "GET":
                return self.reply({"data": state[kind], "next_page": None})
            if kind and self.command == "POST":
                item = (body or {}) | {"id": {"issuers": "fdis_1", "sas": "svac_1", "rules": "fdrl_1"}[kind]}
                state[kind].append(item)
                return self.reply(item)
            return self.reply({"ok": True})

        do_GET = do_POST = handle_any  # noqa: N815

    srv = http.server.HTTPServer(("127.0.0.1", 0), H)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    a = argparse.Namespace(repo="JorisJonkers-dev/nv-rs", env="fork-sync", name="nv-rs-fork-sync", workspace="wrkspc_test",
                           org_id="", no_github=True, dry_run=False)
    api = Api(f"http://127.0.0.1:{srv.server_address[1]}", "test-token", False)
    ids = run(a, api)
    again = run(a, api)
    assert ids == again, "second run must reuse everything"
    rule = state["rules"][0]
    assert rule["match"]["subject_prefix"] == "repo:JorisJonkers-dev/nv-rs:environment:fork-sync"
    assert rule["match"]["claims"]["ref"] == "refs/heads/main" and rule["oauth_scope"] == "workspace:inference"
    assert len(state["rules"]) == 1 and len(state["sas"]) == 1 and len(state["issuers"]) == 1
    assert ("POST", "service_accounts/svac_1/workspaces", {"workspace_id": "wrkspc_test", "workspace_role": "workspace_developer"}) in state["calls"]
    srv.shutdown()
    print("selftest ok: created once, reused on the second run, rule pinned to the environment, main and the repository")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--workspace", help="wrkspc_... of the dedicated workspace")
    ap.add_argument("--org-id", default="")
    ap.add_argument("--repo", default="JorisJonkers-dev/nv-rs")
    ap.add_argument("--env", default="fork-sync")
    ap.add_argument("--name", default="nv-rs-fork-sync", help="name of the service account and the rule")
    ap.add_argument("--dry-run", action="store_true", help="read, but do not create or store anything")
    ap.add_argument("--no-github", action="store_true", help="do not store the variables with gh")
    ap.add_argument("--selftest", action="store_true")
    a = ap.parse_args()
    if a.selftest:
        selftest()
        return
    token = os.environ.get("ANTHROPIC_AUTH_TOKEN", "")
    if not token:
        sys.exit("set ANTHROPIC_AUTH_TOKEN to an org:admin OAuth token (docs/fork/WIF.md, step 2)")
    if os.environ.get("ANTHROPIC_API_KEY"):
        print("note: ANTHROPIC_API_KEY is set in this shell; the Admin API ignores it, unset it to avoid confusion")
    if not a.workspace:
        sys.exit("--workspace wrkspc_... is required")
    run(a, Api(os.environ.get("ANTHROPIC_API_BASE", "https://api.anthropic.com"), token, a.dry_run))
    print("\nDone. Next: run the 'Fork WIF check' workflow, then remove the old key (docs/fork/WIF.md, steps 6 and 7).")


if __name__ == "__main__":
    main()
