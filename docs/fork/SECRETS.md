# Secrets in the fork

The fork is public, so anything a workflow can read, a person who can change a workflow can read. Masking in
logs is not protection: code that has the secret can print it encoded. What protects a secret is who can run
code with it.

| Rule | How it is kept |
| --- | --- |
| People who open pull requests cannot reach secrets | Workflows from forked pull requests get no secrets; nothing uses `pull_request_target`. Outside contributors need approval before any workflow runs (Settings, Actions, General) |
| Only `main` can read the API key | It is an environment secret of `fork-sync`, restricted to runs on `main` (`scripts/fork/setup-repo.sh`), so a branch with a changed workflow cannot read it |
| Few people can push | Write access is the real boundary: keep collaborators to people you trust, and review every change to `.github/` |
| Third-party actions cannot read it | Every action is pinned by commit SHA (Renovate moves the pins); `zizmor` lints the workflows |
| Upstream cannot run code with it | The replay stops at any upstream commit that changes `.github/workflows`, runs from a copy of the scripts, and the sync's token cannot push workflow files |
| A leaked key costs little | A dedicated Anthropic workspace with a low monthly spend limit; rotate the key if in doubt |
| Nobody commits one by accident | Secret scanning with push protection, and gitleaks in Pipeline Complete |

Set the key without leaving it in shell history: `gh secret set ANTHROPIC_API_KEY --env fork-sync -R JorisJonkers-dev/nv-rs`
(the command asks for it). Never paste it into an issue, a pull request, a chat or a workflow file.

## The GitHub App keys

`RELEASE_APP_ID` and `RELEASE_APP_PRIVATE_KEY` are organisation secrets shared with every repository, and the
App is installed on the whole organisation. A person with write access to this public fork could mint tokens for
your private repositories. Give the fork its own App instead: contents and pull requests read and write, no
workflow permission, installed on this repository only, with its ID and key as `fork-sync` environment secrets
(for `release.yml` too, in an environment of its own). Until then, keep write access to the fork to yourself.
