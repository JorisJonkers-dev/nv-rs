# The fork's own GitHub App

The sync and release workflows need a token that can push branches, open pull requests and tag releases, and
whose pull requests trigger CI (the built-in `GITHUB_TOKEN` cannot do the last). The organisation's shared App
could do it, but it is installed on every repository, and a public fork's secrets are readable by anyone who can
change a workflow. So the fork has an App of its own that can only touch this repository.

| Permission | Why |
| --- | --- |
| Contents: write | push `sync/upstream`, tag releases |
| Pull requests: write | open and label the sync pull request, turn on auto-merge, release-please's release pull request |
| Issues: write | release-please's labels (labels are issues to the API) |
| Metadata: read | implied |
| **Not granted** | Workflows (so it cannot push workflow files: a hostile upstream commit cannot add one), Administration, Secrets, Actions, everything else. No webhook; private |

## Quick way (about two minutes)

You need: `gh` logged in with admin on the repository, a browser logged in to GitHub as an organisation owner,
`openssl`, and the fork's setup already applied (`scripts/fork/setup-repo.sh`).

```sh
cd ~/Projects/JorisJonkers-dev/nv-rs
git pull
scripts/fork/create-app.py --selftest    # optional: checks the local pages work, creates nothing
scripts/fork/create-app.py
```

1. The script opens a local page; press **Continue to GitHub**, confirm the name, press **Create GitHub App**.
2. The script receives GitHub's one-time code, exchanges it for the App's id and private key, and stores them as
   the `FORK_APP_ID` and `FORK_APP_PRIVATE_KEY` secrets of the `fork-sync` environment through `gh`. The key is
   never printed and never written to disk.
3. It opens the installation page with this repository preselected. Choose **Only select repositories**, check it
   is `nv-rs`, press **Install**. Never "All repositories".
4. Press Enter in the terminal: the script signs a short-lived token with the key and reads the installation back.
   It prints `OK` when the scope is one repository and the permissions are exactly the table above, and what is
   wrong otherwise.

Then run the sync once (Actions, Upstream Sync, Run workflow) and check that it opens `sync/upstream`.

## By hand (if the script cannot run)

1. Organisation settings, Developer settings, GitHub Apps, **New GitHub App**.
2. Name `jorisjonkers-dev-nv-rs-fork`; homepage the repository URL; untick **Active** under Webhook; permissions as
   in the table; **Only on this account**.
3. Create it, then **Generate a private key** (a `.pem` downloads) and note the **App ID**.
4. **Install App**, choose **Only select repositories**, pick `nv-rs`.
5. Store them without leaving traces: `gh secret set FORK_APP_ID --env fork-sync -R JorisJonkers-dev/nv-rs`, and
   `gh secret set FORK_APP_PRIVATE_KEY --env fork-sync -R JorisJonkers-dev/nv-rs < key.pem`, then delete the `.pem`.

## Afterwards

- The workflows read `FORK_APP_ID` and `FORK_APP_PRIVATE_KEY` from the `fork-sync` environment, which only runs on
  `fork` can use. The organisation's `RELEASE_APP_*` secrets are no longer used by the fork; to be safe, restrict
  them to the repositories that need them (organisation settings, Secrets) so the public fork cannot read them.
- Rotating the key: Settings of the App, generate a new private key, rerun step 5 of the manual way, delete the old
  key in the App's settings.
- Losing the key or the App: delete the App and run the script again; nothing else depends on it.
