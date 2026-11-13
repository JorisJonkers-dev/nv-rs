#!/usr/bin/env python3
"""Create the fork's own GitHub App with only the permissions the automation needs.

Uses GitHub's app manifest flow: this script serves one local page that posts the manifest to GitHub; you
press "Create GitHub App" once; GitHub redirects back here with a one-time code; the script exchanges it for
the App's id and private key and stores both as ENVIRONMENT secrets of the repository
(FORK_APP_ID and FORK_APP_PRIVATE_KEY in the fork-sync environment) through `gh`. The key is never printed
and never written to disk. You then install the App on this one repository (one click, link printed).

Usage: scripts/fork/create-app.py [--owner JorisJonkers-dev] [--repo JorisJonkers-dev/nv-rs] [--name NAME]
                                  [--env fork-sync] [--selftest]
Needs: python3, gh (logged in, admin on the repository), openssl (for the check at the end), a browser
logged in to GitHub as an owner of the organisation. Stdlib only.
"""
import argparse
import base64
import http.server
import json
import os
import secrets
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import webbrowser

# Exactly what fork-sync.yml and release.yml use; no `workflows`, no `administration`, no `secrets`.
PERMISSIONS = {"contents": "write", "pull_requests": "write", "issues": "write"}


def manifest(name: str, port: int) -> dict:
    return {
        "name": name,
        "url": "https://github.com/JorisJonkers-dev/nv-rs",
        "redirect_url": f"http://127.0.0.1:{port}/callback",
        "public": False,
        "default_permissions": PERMISSIONS,
        "default_events": [],
    }


def page(owner: str, state: str, man: dict) -> bytes:
    action = f"https://github.com/organizations/{owner}/settings/apps/new?state={state}"
    value = json.dumps(man).replace("&", "&amp;").replace('"', "&quot;")
    return (
        "<!doctype html><meta charset=utf-8><title>Create the fork's GitHub App</title>"
        "<body style='font:16px system-ui;max-width:40em;margin:3em auto'>"
        "<h1>Create the fork's GitHub App</h1>"
        f"<p>Permissions: {', '.join(f'{k}: {v}' for k, v in PERMISSIONS.items())}. No webhook, private, no workflow access.</p>"
        f"<form method=post action='{action}'><input type=hidden name=manifest value=\"{value}\">"
        "<button style='font-size:1.1em;padding:.5em 1.2em'>Continue to GitHub</button></form>"
        "<p>GitHub will ask you to confirm the name, then press <b>Create GitHub App</b>.</p></body>"
    ).encode()


class Handler(http.server.BaseHTTPRequestHandler):
    server: "Server"

    def log_message(self, *_: object) -> None:
        pass

    def send(self, code: int, body: bytes, ctype: str = "text/html; charset=utf-8") -> None:
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:  # noqa: N802
        url = urllib.parse.urlparse(self.path)
        if url.path == "/":
            self.send(200, page(self.server.owner, self.server.state, self.server.man))
        elif url.path == "/callback":
            q = urllib.parse.parse_qs(url.query)
            if q.get("state", [""])[0] != self.server.state or not q.get("code"):
                self.send(400, b"Bad state or no code. Start again from the first page.")
                return
            self.server.code = q["code"][0]
            self.send(200, b"<p>Done. You can close this tab and go back to the terminal.</p>")
        else:
            self.send(404, b"not found")


class Server(http.server.HTTPServer):
    owner: str
    state: str
    man: dict
    code: str | None = None


def gh_secret(repo: str, env: str, name: str, value: str) -> None:
    subprocess.run(["gh", "secret", "set", name, "--env", env, "-R", repo], input=value, text=True, check=True,
                   stdout=subprocess.DEVNULL)


def b64(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def app_jwt(app_id: int, pem: str) -> str:
    """RS256 JWT for the App, signed with openssl so no library is needed. The key goes through stdin only."""
    now = int(time.time())
    head = b64(json.dumps({"alg": "RS256", "typ": "JWT"}).encode())
    body = b64(json.dumps({"iat": now - 60, "exp": now + 540, "iss": app_id}).encode())
    with tempfile.NamedTemporaryFile() as msg:
        msg.write(f"{head}.{body}".encode())
        msg.flush()
        sig = subprocess.run(["openssl", "dgst", "-sha256", "-sign", "/dev/stdin", msg.name], input=pem.encode(),
                             capture_output=True, check=True).stdout
    return f"{head}.{body}.{b64(sig)}"


def api(path: str, token: str, method: str = "GET") -> dict:
    req = urllib.request.Request(f"https://api.github.com{path}", method=method, headers={
        "Authorization": f"Bearer {token}", "Accept": "application/vnd.github+json", "User-Agent": "fork-create-app"})
    with urllib.request.urlopen(req, timeout=30) as resp:  # noqa: S310
        return json.load(resp)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--owner", default="JorisJonkers-dev")
    ap.add_argument("--repo", default="JorisJonkers-dev/nv-rs")
    ap.add_argument("--name", default="jorisjonkers-dev-nv-rs-fork")
    ap.add_argument("--env", default="fork-sync")
    ap.add_argument("--selftest", action="store_true", help="serve the pages and check the callback handling, create nothing")
    a = ap.parse_args()

    srv = Server(("127.0.0.1", 0), Handler)
    srv.owner, srv.state = a.owner, secrets.token_urlsafe(24)
    srv.man = manifest(a.name, srv.server_address[1])
    port = srv.server_address[1]

    if a.selftest:
        threading.Thread(target=srv.handle_request, daemon=True).start()
        html = urllib.request.urlopen(f"http://127.0.0.1:{port}/", timeout=5).read().decode()  # noqa: S310
        assert "contents: write" in html and "pull_requests: write" in html and "workflows" not in html.split("Permissions:")[1].split(".")[0]
        threading.Thread(target=srv.handle_request, daemon=True).start()
        try:
            urllib.request.urlopen(f"http://127.0.0.1:{port}/callback?state=wrong&code=x", timeout=5)  # noqa: S310
            sys.exit("selftest: a wrong state was accepted")
        except urllib.error.HTTPError as err:
            assert err.code == 400
        print("selftest ok: manifest page served, wrong state refused")
        return

    # Make sure the environment that will hold the secrets exists (restricted to the fork branch by setup-repo.sh).
    # Only create it when absent: a PUT on an existing environment can reset its deployment branch policy.
    exists = subprocess.run(["gh", "api", f"repos/{a.repo}/environments/{a.env}", "--silent"], capture_output=True).returncode == 0
    if not exists:
        subprocess.run(["gh", "api", "-X", "PUT", f"repos/{a.repo}/environments/{a.env}", "--silent"], check=True)
        print(f"created the '{a.env}' environment; run scripts/fork/setup-repo.sh to restrict it to the fork branch")

    print(f"Opening http://127.0.0.1:{port}/ ; if no browser opens, open it yourself.")
    print("In the browser: Continue to GitHub, confirm the name, press 'Create GitHub App'.")
    webbrowser.open(f"http://127.0.0.1:{port}/")
    deadline = time.time() + 600
    srv.timeout = 5
    while srv.code is None and time.time() < deadline:
        srv.handle_request()
    if srv.code is None:
        sys.exit("timed out after 10 minutes; nothing was created")

    req = urllib.request.Request(f"https://api.github.com/app-manifests/{srv.code}/conversions", method="POST",
                                 headers={"Accept": "application/vnd.github+json", "User-Agent": "fork-create-app"})
    with urllib.request.urlopen(req, timeout=30) as resp:  # noqa: S310
        app = json.load(resp)
    app_id, slug, pem = app["id"], app["slug"], app["pem"]
    gh_secret(a.repo, a.env, "FORK_APP_ID", str(app_id))
    gh_secret(a.repo, a.env, "FORK_APP_PRIVATE_KEY", pem)
    print(f"Created the App '{slug}' (id {app_id}). Its id and private key are now secrets of the "
          f"'{a.env}' environment of {a.repo}. The key was not printed or saved anywhere else.")
    repo_id = subprocess.run(["gh", "api", f"repos/{a.repo}", "--jq", ".id"], capture_output=True, text=True, check=True).stdout.strip()
    org_id = subprocess.run(["gh", "api", f"orgs/{a.owner}", "--jq", ".id"], capture_output=True, text=True, check=True).stdout.strip()
    url = f"https://github.com/apps/{slug}/installations/new/permissions?target_id={org_id}&repository_ids[]={repo_id}"
    print(f"\nInstall it on this one repository: {url}")
    print("If the repository is not preselected, choose 'Only select repositories' and pick it. Never 'All repositories'.")
    webbrowser.open(url)
    input("Press Enter after installing, to check the installation: ")
    try:
        inst = api(f"/repos/{a.repo}/installation", app_jwt(app_id, pem))
    except (urllib.error.HTTPError, subprocess.CalledProcessError) as err:
        sys.exit(f"could not check the installation ({err}); look at https://github.com/organizations/{a.owner}/settings/installations")
    perms = {k: v for k, v in inst["permissions"].items() if k != "metadata"}
    print(f"installation scope: {inst['repository_selection']}; permissions: {perms}")
    problems = []
    if inst["repository_selection"] != "selected":
        problems.append("installed on ALL repositories: change it to the one repository")
    if perms != PERMISSIONS:
        problems.append(f"permissions differ from the expected {PERMISSIONS}")
    print("OK" if not problems else "PROBLEMS:\n  " + "\n  ".join(problems))
    del pem
    os.environ.pop("FORK_APP_PRIVATE_KEY", None)


if __name__ == "__main__":
    main()
