# Secrets in the fork

The fork is public, so anything a workflow can read, a person who can change a workflow can read. Masking in
logs is not protection: code that has the secret can print it encoded. What protects a secret is who can run
code with it.

| Rule | How it is kept |
| --- | --- |
| People who open pull requests cannot reach secrets | Workflows from forked pull requests get no secrets; nothing uses `pull_request_target`. Outside contributors need approval before any workflow runs (Settings, Actions, General) |
| There is no API key to read | The sync job trades its GitHub OIDC token for a 10-minute, inference-only Anthropic token (workload identity federation, [WIF.md](WIF.md)); the rule accepts only this repository, the `fork-sync` environment and `fork` |
| Only `fork` can use the environment | `fork-sync` is restricted to runs on the `fork` branch (`scripts/fork/setup-repo.sh`), so a branch with a changed workflow can neither read its secrets nor satisfy the Anthropic rule |
| Few people can push | Write access is the real boundary: keep collaborators to people you trust, and review every change to `.github/` |
| Third-party actions cannot read it | Every action is pinned by commit SHA (Renovate moves the pins); `zizmor` lints the workflows |
| Upstream cannot run code with it | The replay stops at any upstream commit that changes `.github/workflows`, runs from a copy of the scripts, and the sync's token cannot push workflow files |
| Abuse costs little | The token is inference-only, lives ten minutes, and the workspace has a hard monthly spend limit |
| Nobody commits one by accident | Secret scanning with push protection, and gitleaks in Pipeline Complete |

If you ever do hold a key (for a local experiment), keep it out of the repository and out of shell history, and never
paste it into an issue, a pull request, a chat or a workflow file.

## The GitHub App keys

The fork has its own App, installed on this repository only, with contents, pull requests and issues access and
no workflow permission; its id and key are `fork-sync` environment secrets. How it is made, checked and rotated:
[APP.md](APP.md). The organisation's shared `RELEASE_APP_*` secrets are not used here; restrict them to the
repositories that need them so this public fork cannot read them.
