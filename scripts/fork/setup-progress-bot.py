#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Set up the progress App: the bot that commits the progress pictures to the fork branch (docs/fork/PROGRESS.md).

Run it yourself: creating a GitHub App needs your browser login. Each step is shown before it runs and asks you to
confirm it, and a step that is already done is skipped, so the script is safe to run again.

  1. Create the App with GitHub's App manifest flow: a page on 127.0.0.1 posts the manifest (the name, the permissions
     contents: write and metadata: read, no webhook, private) to GitHub; you press "Create GitHub App"; GitHub sends a
     one-time code back to the page, which the script exchanges for the App's id and private key. Skipped when the App
     exists (a run that needs the key again asks for a key file you download from the App's page).
  2. Install it on the six Raul repositories only (the install page opens with them selected), then check that the
     installation covers exactly those six. Never all repositories.
  3. On each of the six: the App's id as the Actions variable PROGRESS_APP_ID and its private key as the Actions secret
     PROGRESS_APP_PRIVATE_KEY (`gh variable set`, `gh secret set`; the key goes through standard input only).
  4. On nv-rs: the App as a bypass actor of the ruleset "Fork line" (the one that stops the push to `fork`). No
     other rule changes; scripts/fork/apply-ruleset.sh keeps that bypass from then on. Another repository's default
     branch gets the same bypass only when asked with `--bypass <repo>:<ruleset>` (for example `--bypass loot-rs:Main`,
     where tally's report workflow pushes the generated reports); that repository's .github/rulesets file must list
     the App too, or its apply-ruleset script removes the bypass again.

The private key is never printed and never written to a file: it stays in memory and reaches `gh secret set` and
openssl (which signs the App's token for the installation check) through standard input. A key file you download for a
later run is deleted once read, if you agree.

Usage: scripts/fork/setup-progress-bot.py [--owner JorisJonkers-dev] [--name raul-progress-bot] [--yes] [--selftest]
                                         [--bypass <repo>:<ruleset>]...
Needs: python3, gh (logged in as an owner of the organisation), openssl, a browser logged in to GitHub. Stdlib only.
"""
import argparse
import html
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

REPOS = ["nv-rs", "lodwright-rs", "rauls-trusty-toolbox", "nvmod-rs", "loot-rs", "ttwright-rs"]
# What the App may do: write contents (the commit), read metadata (always granted). Nothing else.
PERMISSIONS = {"contents": "write", "metadata": "read"}
RULESET_REPO = "nv-rs"
RULESET_NAME = "Fork line"
VAR_ID = "PROGRESS_APP_ID"
SECRET_KEY = "PROGRESS_APP_PRIVATE_KEY"
AGENT = "fork-setup-progress-bot"


def manifest(name: str, owner: str, port: int) -> dict:
    return {
        "name": name,
        "url": f"https://github.com/{owner}/nv-rs/blob/fork/docs/fork/PROGRESS.md",
        "redirect_url": f"http://127.0.0.1:{port}/callback",
        "public": False,
        "hook_attributes": {"active": False, "url": "https://example.invalid/"},
        "default_permissions": PERMISSIONS,
        "default_events": [],
    }


def page(owner: str, state: str, man: dict) -> bytes:
    action = html.escape(f"https://github.com/organizations/{urllib.parse.quote(owner)}/settings/apps/new?state={state}",
                         quote=True)
    value = html.escape(json.dumps(man), quote=True)
    perms = ", ".join(f"{k}: {v}" for k, v in PERMISSIONS.items())
    return (
        "<!doctype html><meta charset=utf-8><title>Create the progress App</title>"
        "<body style='font:16px system-ui;max-width:40em;margin:3em auto'>"
        f"<h1>Create the App {html.escape(man['name'])}</h1>"
        f"<p>Permissions: {perms}. No webhook, private.</p>"
        f"<form method=post action='{action}'><input type=hidden name=manifest value=\"{value}\">"
        "<button style='font-size:1.1em;padding:.5em 1.2em'>Continue to GitHub</button></form>"
        "<p>GitHub asks you to confirm the name; then press <b>Create GitHub App</b>.</p></body>"
    ).encode()


class Handler(http.server.BaseHTTPRequestHandler):
    server: "Server"

    def log_message(self, *_: object) -> None:
        pass

    def send(self, code: int, body: bytes) -> None:
        self.send_response(code)
        self.send_header("Content-Type", "text/html; charset=utf-8")
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
    code: "str | None" = None


def step(text: str, yes: bool) -> bool:
    print(f"\n== {text}")
    if yes:
        return True
    return input("   Do it? [y/N] ").strip().lower() in ("y", "yes")


def gh(*args: str, input_text: "str | None" = None, check: bool = True) -> subprocess.CompletedProcess:
    return subprocess.run(["gh", *args], input=input_text, text=True, capture_output=True, check=check)


def gh_json(*args: str) -> "dict | list | None":
    r = gh("api", *args, check=False)
    return json.loads(r.stdout) if r.returncode == 0 and r.stdout.strip() else None


class Key:
    """The private key, in memory only: never printed, never written to a file by this script."""

    def __init__(self, pem: str) -> None:
        self.pem = pem

    def close(self) -> None:
        self.pem = ""


def installation(owner: str, slug: str) -> "dict | None":
    """The App's installation in the organisation, as an owner sees it, or None."""
    data = gh_json(f"orgs/{owner}/installations?per_page=100") or {}
    for inst in data.get("installations", []):
        if inst.get("app_slug") == slug:
            return inst
    return None


def b64(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def app_jwt(app_id: int, key: Key) -> str:
    """The App's RS256 token, signed by openssl with the key read from standard input (never from a file)."""
    now = int(time.time())
    head = b64(json.dumps({"alg": "RS256", "typ": "JWT"}).encode())
    body = b64(json.dumps({"iat": now - 60, "exp": now + 540, "iss": app_id}).encode())
    with tempfile.NamedTemporaryFile() as msg:
        msg.write(f"{head}.{body}".encode())
        msg.flush()
        sig = subprocess.run(["openssl", "dgst", "-sha256", "-sign", "/dev/stdin", msg.name],
                             input=key.pem.encode(), capture_output=True, check=True).stdout
    return f"{head}.{body}.{b64(sig)}"


def installed_on(owner: str, repo: str, jwt: str) -> bool:
    req = urllib.request.Request(f"https://api.github.com/repos/{owner}/{repo}/installation", headers={
        "Authorization": f"Bearer {jwt}", "Accept": "application/vnd.github+json", "User-Agent": AGENT})
    try:
        with urllib.request.urlopen(req, timeout=30):  # noqa: S310
            return True
    except urllib.error.HTTPError:
        return False


def create_app(owner: str, name: str) -> "tuple[int, str, Key]":
    srv = Server(("127.0.0.1", 0), Handler)
    srv.owner, srv.state = owner, secrets.token_urlsafe(24)
    srv.man = manifest(name, owner, srv.server_address[1])
    url = f"http://127.0.0.1:{srv.server_address[1]}/"
    print(f"   Opening {url} ; if no browser opens, open it yourself.")
    print("   In the browser: Continue to GitHub, confirm the name, press 'Create GitHub App'.")
    webbrowser.open(url)
    deadline = time.time() + 600
    srv.timeout = 5
    while srv.code is None and time.time() < deadline:
        srv.handle_request()
    if srv.code is None:
        sys.exit("timed out after 10 minutes; nothing was created")
    req = urllib.request.Request(f"https://api.github.com/app-manifests/{srv.code}/conversions", method="POST",
                                 headers={"Accept": "application/vnd.github+json", "User-Agent": AGENT})
    with urllib.request.urlopen(req, timeout=30) as resp:  # noqa: S310
        app = json.load(resp)
    print(f"   Created the App '{app['slug']}' (id {app['id']}). Its private key was not printed.")
    return int(app["id"]), app["slug"], Key(app["pem"])


def key_from_file(slug: str, owner: str) -> Key:
    print(f"   The App exists, and its private key is needed. Make one at "
          f"https://github.com/organizations/{owner}/settings/apps/{slug} ('Generate a private key'),")
    path = input("   then give the downloaded .pem file's path: ").strip().strip("'\"")
    with open(os.path.expanduser(path)) as f:
        key = Key(f.read())
    if input("   Delete the downloaded file now? [Y/n] ").strip().lower() not in ("n", "no"):
        os.remove(os.path.expanduser(path))
        print("   Deleted.")
    return key


def parse_bypass(value: str) -> "tuple[str, str]":
    """`<repo>:<ruleset>` for --bypass; the repository must be one of the six the App is installed on."""
    repo, sep, ruleset = value.partition(":")
    if not sep or not repo or not ruleset.strip():
        raise argparse.ArgumentTypeError(f"'{value}' is not <repo>:<ruleset>")
    if repo not in REPOS:
        raise argparse.ArgumentTypeError(f"'{repo}' is not one of the App's repositories: {', '.join(REPOS)}")
    return repo, ruleset


def ensure_bypass(owner: str, app_id: int, yes: bool, repo_name: str = RULESET_REPO,
                  ruleset_name: str = RULESET_NAME) -> None:
    repo = f"{owner}/{repo_name}"
    rulesets = gh_json(f"repos/{repo}/rulesets") or []
    found = [r for r in rulesets if r.get("name") == ruleset_name]
    if not found:
        sys.exit(f"{repo} has no ruleset named '{ruleset_name}'; nothing changed")
    if len(found) > 1:
        sys.exit(f"{repo} has {len(found)} rulesets named '{ruleset_name}'; nothing changed")
    rs = gh_json(f"repos/{repo}/rulesets/{found[0]['id']}")
    if rs.get("target") != "branch":
        sys.exit(f"{repo} ruleset '{ruleset_name}' targets {rs.get('target')}, not branches; nothing changed")
    actors = rs.get("bypass_actors") or []
    if any(a.get("actor_type") == "Integration" and a.get("actor_id") == app_id for a in actors):
        print(f"\n== {repo}: the App already bypasses '{ruleset_name}' (ruleset {rs['id']}): skipped")
        return
    if not step(f"{repo}: add the App (id {app_id}) as the only new bypass actor of ruleset '{ruleset_name}' "
                f"({rs['id']}); its rules stay as they are", yes):
        return
    kept = ("name", "target", "enforcement", "conditions", "rules")
    body = {k: rs[k] for k in kept}
    body["bypass_actors"] = actors + [{"actor_id": app_id, "actor_type": "Integration", "bypass_mode": "always"}]
    gh("api", "-X", "PUT", f"repos/{repo}/rulesets/{rs['id']}", "--input", "-", input_text=json.dumps(body))
    after = gh_json(f"repos/{repo}/rulesets/{rs['id']}")
    changed = [k for k in kept if after.get(k) != rs.get(k)]
    new_actors = [a for a in after.get("bypass_actors") or [] if a not in actors]
    if changed or [(a.get("actor_id"), a.get("actor_type")) for a in new_actors] != [(app_id, "Integration")]:
        sys.exit(f"the ruleset changed beyond the one bypass actor ({changed or new_actors}): check it at once in the "
                 "repository settings")
    print("   Done; only the one bypass actor was added.")


def selftest() -> None:
    srv = Server(("127.0.0.1", 0), Handler)
    srv.owner, srv.state = "JorisJonkers-dev", "s"
    srv.man = manifest("raul-progress-bot", srv.owner, srv.server_address[1])
    port = srv.server_address[1]
    threading.Thread(target=srv.handle_request, daemon=True).start()
    html = urllib.request.urlopen(f"http://127.0.0.1:{port}/", timeout=5).read().decode()  # noqa: S310
    perms = html.split("Permissions:")[1].split(".")[0]
    assert perms.strip() == "contents: write, metadata: read", perms
    assert srv.man["default_events"] == [] and srv.man["hook_attributes"]["active"] is False
    threading.Thread(target=srv.handle_request, daemon=True).start()
    try:
        urllib.request.urlopen(f"http://127.0.0.1:{port}/callback?state=wrong&code=x", timeout=5)  # noqa: S310
        sys.exit("selftest: a wrong state was accepted")
    except urllib.error.HTTPError as err:
        assert err.code == 400
    assert parse_bypass("loot-rs:Main") == ("loot-rs", "Main")
    for bad in ("loot-rs", "loot-rs:", "other-repo:Main", ":Main"):
        try:
            parse_bypass(bad)
            sys.exit(f"selftest: --bypass {bad} was accepted")
        except argparse.ArgumentTypeError:
            pass
    print("selftest ok: manifest page and permissions, no webhook, wrong state refused, --bypass values checked")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--owner", default="JorisJonkers-dev")
    ap.add_argument("--name", default="raul-progress-bot")
    ap.add_argument("--yes", action="store_true", help="do every step without asking")
    ap.add_argument("--selftest", action="store_true", help="check the page and the key file handling; change nothing")
    ap.add_argument("--bypass", action="append", default=[], type=parse_bypass, metavar="REPO:RULESET",
                    help="also make the App a bypass actor of this ruleset (repeatable), e.g. loot-rs:Main")
    a = ap.parse_args()
    if a.selftest:
        selftest()
        return

    owner = a.owner
    repo_ids = {}
    for r in REPOS:
        info = gh_json(f"repos/{owner}/{r}")
        if not info:
            sys.exit(f"cannot read {owner}/{r} with gh; log in as an owner of {owner}")
        repo_ids[r] = info["id"]

    key = None
    try:
        # 1. The App.
        existing = gh_json(f"apps/{a.name}")
        if existing:
            app_id, slug = int(existing["id"]), existing["slug"]
            print(f"\n== The App '{slug}' exists (id {app_id}): not created again")
        else:
            if not step(f"Create the App '{a.name}' in {owner} (contents: write, metadata: read, no webhook, private)",
                        a.yes):
                sys.exit("stopped; nothing was created")
            app_id, slug, key = create_app(owner, a.name)

        # 2. The installation, on the six repositories only.
        org_id = gh_json(f"orgs/{owner}")["id"]
        ids = "&".join(f"repository_ids[]={repo_ids[r]}" for r in REPOS)
        url = f"https://github.com/apps/{slug}/installations/new/permissions?target_id={org_id}&{ids}"
        inst = installation(owner, slug)
        if inst and inst.get("repository_selection") == "selected" and key is None:
            print("\n== The App is installed (selected repositories only): skipped. Which six it covers is checked "
                  "when the key is at hand (a first run); see the organisation's Installed GitHub Apps page.")
        elif step(f"Install the App on {', '.join(REPOS)} only (the page opens with them selected; keep 'Only select "
                  f"repositories', never 'All repositories')", a.yes):
            if key is None or not all(installed_on(owner, r, app_jwt(app_id, key)) for r in REPOS):
                print(f"   Opening {url}")
                webbrowser.open(url)
                input("   Press Enter after installing: ")
            inst = installation(owner, slug)
            if not inst:
                sys.exit("no installation of the App found; run this again after installing")
            if key is not None:
                jwt = app_jwt(app_id, key)
                missing = [r for r in REPOS if not installed_on(owner, r, jwt)]
                if missing:
                    sys.exit(f"not installed on {', '.join(missing)}; add them to the installation and run this again")
        if inst:
            if inst.get("repository_selection") != "selected":
                sys.exit("the App is installed on ALL repositories: change it to the six in the organisation's "
                         "settings (Installed GitHub Apps), then run this again")
            if inst.get("permissions") != PERMISSIONS:
                sys.exit(f"the installation's permissions are {inst.get('permissions')}, not {PERMISSIONS}")
            print("   Installation: selected repositories only, with the expected permissions.")

        # 3. The id and the key on each repository.
        for r in REPOS:
            full = f"{owner}/{r}"
            var = gh("variable", "get", VAR_ID, "-R", full, check=False).stdout.strip()
            has_key = SECRET_KEY in gh("secret", "list", "-R", full, "--json", "name", "--jq", ".[].name",
                                       check=False).stdout.split()
            if var == str(app_id) and has_key:
                print(f"\n== {full}: {VAR_ID} and {SECRET_KEY} are set: skipped")
                continue
            if not step(f"{full}: set the Actions variable {VAR_ID}={app_id} and the Actions secret {SECRET_KEY} "
                        "(the key through standard input, not printed)", a.yes):
                continue
            if key is None:
                key = key_from_file(slug, owner)
            gh("variable", "set", VAR_ID, "-R", full, "--body", str(app_id))
            gh("secret", "set", SECRET_KEY, "-R", full, input_text=key.pem)
            print("   Set.")

        # 4. The bypass: nv-rs's fork line, and the rulesets asked for with --bypass.
        ensure_bypass(owner, app_id, a.yes)
        for repo_name, ruleset_name in a.bypass:
            ensure_bypass(owner, app_id, a.yes, repo_name, ruleset_name)
        print("\nAll steps done. The next push to fork runs 'Progress pictures' and pushes as the App when a count "
              "changed (docs/fork/PROGRESS.md).")
    finally:
        if key is not None:
            key.close()


if __name__ == "__main__":
    main()
