# Keyless access to the Claude API (workload identity federation)

The sync workflow asks a small model for commit titles. Instead of storing an API key, the job asks GitHub for a
short-lived identity token (OIDC) and Anthropic trades it for an access token that lives ten minutes. There is
nothing to steal from the repository, nothing to rotate, and the rule only accepts this repository, this
environment and `main`.

```
job (environment fork-sync, on main) --OIDC token--> GitHub
job --token--> Anthropic /v1/oauth/token --checks the rule--> access token (10 min, inference only)
job --access token--> /v1/messages
```

What you set up in Anthropic: an **issuer** (GitHub Actions, once per organisation), a **service account**, and a
**federation rule** that says "tokens from that issuer whose subject is `repo:JorisJonkers-dev/nv-rs:environment:fork-sync`,
repository `JorisJonkers-dev/nv-rs`, ref `refs/heads/main` may act as that service account in this workspace".
`scripts/fork/setup-anthropic-wif.py` does all of it through the Admin API.

## Steps

**1. A dedicated workspace with a spend limit** (Console, Settings, Workspaces, Create workspace). Name it for this
repository, set the monthly spend limit (the $5 you chose), and copy its id (`wrkspc_...`, in the workspace's
URL and settings). A leaked or abused token can never spend more than that.

**2. An admin token for the setup (once, in a shell you use only for this).** Install the `ant` CLI, then:

```sh
ant auth login --profile admin --scope "org:admin"      # opens a browser; you log in as an organisation admin
export ANTHROPIC_AUTH_TOKEN=$(ant auth print-credentials --profile admin --access-token)
unset ANTHROPIC_API_KEY                                  # a key in the environment overrides federation
```

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

**4. Unset the admin token** (`unset ANTHROPIC_AUTH_TOKEN`; `ant auth logout --profile admin` if you want it gone).

**5. Check it end to end.** Actions, **Claude Federation Check**, Run workflow (it runs in the `fork-sync` environment on `main`).
It prints `token exchange: ok` and `messages call: HTTP 200`. A failure prints the HTTP status; the reason is on the
Console's Workload identity page, tab **Authentication history**.

**6. Remove the old key.** In this order: delete the secret (`gh secret delete ANTHROPIC_API_KEY --env fork-sync -R JorisJonkers-dev/nv-rs`,
and any repository-level copy), then delete the key on the Console's API keys page. A key in the environment would
override federation (the SDK order is: key, then federation), so the check above only proves federation once the
key is gone.

**7. Run Upstream Sync once** (Actions, Upstream Sync, Run workflow) and read the titles it wrote.

## How the rule is pinned

| Match | Value | Why |
| --- | --- | --- |
| `subject_prefix` (exact) | `repo:JorisJonkers-dev/nv-rs:environment:fork-sync` | a job that declares the environment gets this subject; only runs on `main` can use the environment |
| `claims.repository` | `JorisJonkers-dev/nv-rs` | defence in depth against subject parsing |
| `claims.repository_owner` | `JorisJonkers-dev` | the same |
| `claims.ref` | `refs/heads/main` | a run from another branch never matches |
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
