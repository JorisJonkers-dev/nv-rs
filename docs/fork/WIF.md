# Keyless access to the Claude API (workload identity federation)

The sync workflow asks a small model for commit titles. Instead of storing an API key, the job asks GitHub for a
short-lived identity token (OIDC) and Anthropic trades it for an access token that lives ten minutes. There is
nothing to steal from the repository, nothing to rotate, and the rule only accepts this repository, this
environment and the `fork` branch.

```
job (environment fork-sync, on fork) --OIDC token--> GitHub
job --token--> Anthropic /v1/oauth/token --checks the rule--> access token (10 min, inference only)
job --access token--> /v1/messages
```

What you set up in Anthropic: an **issuer** (GitHub Actions, once per organisation), a **service account**, and a
**federation rule** that says "tokens from that issuer whose subject is `repo:JorisJonkers-dev/nv-rs:environment:fork-sync`,
repository `JorisJonkers-dev/nv-rs`, ref `refs/heads/fork` may act as that service account in this workspace".
`scripts/fork/setup-anthropic-wif.py` does all of it through the Admin API.

## The short way: one script

```sh
scripts/fork/setup-anthropic.sh            # login, workspace, federation, GitHub variables
# merge the baseline pull request to fork, then:
scripts/fork/setup-anthropic.sh --verify   # runs the check, removes the old key, optional first sync
```

The script asks you for the two things only you can do in the Console (the spend limit, and deleting the old key) and
does everything else. The steps below are what it does, for when you want to run them by hand.

## Steps

**1. A dedicated workspace with a spend limit** (Console, Settings, Workspaces, Create workspace). Name it for this
repository, set the monthly spend limit (the $5 you chose), and copy its id (`wrkspc_...`, in the workspace's
URL and settings). A leaked or abused token can never spend more than that.

**2. An admin token for the setup (once, in a shell you use only for this).** Install the `ant` CLI, then:

```sh
ant --profile admin auth login --admin          # opens a browser; log in as an organisation admin. --admin adds the org:admin scope
export ANTHROPIC_AUTH_TOKEN=$(ant --profile admin auth print-credentials --access-token)
unset ANTHROPIC_API_KEY                          # a key in the environment overrides federation
```

(`--profile` is a global flag, so it goes before `auth`. Checked against `ant` 2.7.1.)

The token is short-lived; rerun the `export` line if requests start returning 401. The organisation id is on
Console, Settings, Organization (the script finds it itself and asks for `--org-id` if it cannot).

**3. Create the Anthropic side and store the identifiers:**

```sh
cd ~/Projects/JorisJonkers-dev/nv-rs
python3 -I scripts/fork/setup-anthropic-wif.py --selftest                 # optional: local mock, creates nothing
python3 -I scripts/fork/setup-anthropic-wif.py --workspace wrkspc_... --dry-run
python3 -I scripts/fork/setup-anthropic-wif.py --workspace wrkspc_...
```

It reuses an existing GitHub Actions issuer if you have one, creates the service account `nv-rs-fork-sync`
(developer role), adds it to the workspace, creates the rule (inference-only scope, 600 second tokens), and stores
`ANTHROPIC_FEDERATION_RULE_ID`, `ANTHROPIC_ORGANIZATION_ID`, `ANTHROPIC_SERVICE_ACCOUNT_ID` and
`ANTHROPIC_WORKSPACE_ID` as **variables** of the `fork-sync` environment. They are identifiers, not secrets: knowing
them gets nobody a token. If the Admin API refuses something, the Console does the same: Workload identity,
**Connect workload**, the GitHub Actions tile, with the values above.

**4. Unset the admin token** (`unset ANTHROPIC_AUTH_TOKEN`; `ant --profile admin auth logout` if you want it gone).

**5. Check it end to end.** Actions, **Claude Federation Check**, Run workflow (it runs in the `fork-sync` environment on `fork`).
It prints `token exchange: ok` and `messages call: HTTP 200`. A failure prints the HTTP status; the reason is on the
Console's Workload identity page, tab **Authentication history**.

**6. Remove the old key.** In this order: delete the secret (`gh secret delete ANTHROPIC_API_KEY --env fork-sync -R JorisJonkers-dev/nv-rs`,
and any repository-level copy), then delete the key on the Console's API keys page. A key in the environment would
override federation (the SDK order is: key, then federation), so the check above only proves federation once the
key is gone.

**7. Run Upstream Sync once** (Actions, Upstream Sync, Run workflow) and read the titles it wrote.

## The fallback: an API key in a limited workspace

Federation needs a Console organisation. A personal ("Individual Org") login answers 403 on workspaces and 404 on the service
account and federation endpoints (seen with `setup-anthropic.sh --diagnose`). The scripts then offer the fallback: create a key in
the workspace that has the monthly limit, and store it as the **environment** secret `ANTHROPIC_API_KEY` of `fork-sync`
(`scripts/fork/setup-anthropic.sh` asks for it at a hidden prompt, or `gh secret set ANTHROPIC_API_KEY --env fork-sync -R JorisJonkers-dev/nv-rs`).
Only runs on the `fork` branch can read it, the key can only spend what the workspace limit allows, and the sync works without it
(the fixed rule writes the titles). If you later get an organisation, rerun the setup: federation takes precedence and the key can go.

## If the API says 403 or 404 (and about retries)

The scripts retry what can be transient: network errors, 408, 425, 429 and every 5xx including 529 ("overloaded"), up to 20
times with a growing pause (`ANTHROPIC_RETRIES` changes it). The title step during a sync tries 5 times and then falls back to
the fixed rule, so an Anthropic incident never blocks a sync.

**Which organisation?** `ant auth login` remembers the organisation the first time a profile is used and reuses it silently
(`Using organization <id> from profile ...`). If you have several organisations, the profile can be stuck on the wrong one,
often the personal "Individual Org". `scripts/fork/setup-anthropic.sh` therefore uses a throwaway profile, so the browser
asks; to skip the question run `ORG_ID=<id from Console, Settings, Organization> scripts/fork/setup-anthropic.sh`, or retarget
a profile by hand: `ant --profile NAME profile set organization_id <id>`.

A **404 `Not found`** on the federation endpoints means the feature is not available to that organisation (or you are in a different
organisation than you think). A **403 `Missing permissions`** is different: it is an answer, not an outage, so it is not retried. With a login that says
"Individual Org" the cause is most likely the account type: Anthropic's Admin API documentation says it is not available to
individual Console accounts, which must first set up an organisation (Console, Settings, Organization), and the service
account and federation endpoints are part of it. What to do:

| Option | How |
| --- | --- |
| Set up the organisation, then rerun `scripts/fork/setup-anthropic.sh` | Console, Settings, Organization. Check there what it costs and what it changes before you do it |
| Try the Console wizard | Console, Workload identity, Connect workload, GitHub Actions tile, with the values in the table below. It uses the same service, so it may be refused the same way |
| Keep an API key instead (a smaller guarantee) | In the `nv-rs-fork` workspace (monthly limit set) create a key and store it as an **environment** secret: `gh secret set ANTHROPIC_API_KEY --env fork-sync -R JorisJonkers-dev/nv-rs` (it prompts; nothing in shell history). The sync workflow already passes it to the title step; it is readable only by runs on `fork`, and the spend limit caps the damage. Federation, if you later get it, takes precedence |

If a key is the answer, the rest of the setup (the repository protections, the App, the sync) is unaffected.

## How the rule is pinned

| Match | Value | Why |
| --- | --- | --- |
| `subject_prefix` (exact) | `repo:JorisJonkers-dev/nv-rs:environment:fork-sync` | a job that declares the environment gets this subject; only runs on `fork` can use the environment |
| `claims.repository` | `JorisJonkers-dev/nv-rs` | defence in depth against subject parsing |
| `claims.repository_owner` | `JorisJonkers-dev` | the same |
| `claims.ref` | `refs/heads/fork` | a run from another branch never matches |
| `audience` | `https://api.anthropic.com` | what the workflow asks GitHub for |
| scope | `workspace:inference` | the token can call the model and nothing else; the script falls back to `workspace:developer` only if inference is refused |
| lifetime | 600 seconds | the least that covers one replay |

Never loosen this to `repo:JorisJonkers-dev/*` or to a wildcard without a `ref` claim: that also matches pull
requests from forks, and anyone could mint a token.

## What can go wrong

| Symptom | Cause |
| --- | --- |
| `401 Authentication failed`, history says `match_subject_prefix` | the job has no `environment:` (the subject then ends in `ref:...`), or the environment name differs |
| history says `jti_reused` | a retry presented the same GitHub token twice; `rewrite_title.py` fetches a fresh one for every exchange |
| the titles come from the fixed rule, not the model | the variables are missing, the job lacks `id-token: write`, or the exchange failed (the job log says which); the sync still works |
| the model call works but is billed to the wrong place | the rule's workspace; check the workspace id variable |

## Using the same for other repositories

The issuer is shared. For another repository or environment, run the script again with `--repo`, `--env`, `--name` and
that repository's workspace: one rule each, never a shared wildcard.
