# Public text

`docs/fork/` and `scripts/fork/` are public. They describe the project, not one person's machine.

## The rule

- Hardware: name the class that gives the numbers their context ("an AMD RDNA 3 card on RADV", "a 16-core desktop"), not the exact card or a PCI device id. Where the code itself carries a reference adapter (`engine_platform::fork_tables`), the docs say "a reference card of the family" and the code is the record.
- Personal tools and paths: say "a local wrapper" or "a test machine", never the wrapper's name or a path under a real home folder. Placeholders (`/home/someone`, `/home/runner`) are fine.
- Role words are fine: "the maintainer's call", "the owner decides". "The owner's machine", "the owner's session" and similar are not.
- Test fixtures follow the same rule: a synthetic log names a synthetic GPU ("Example GPU").

## The check

`python3 -I scripts/fork/public_text.py` greps the tracked files under both folders for the patterns (an exact card, a PCI device id, a real home path, the wrapper's name, the personal-machine phrases) and exits 1 on a finding. `--selftest` proves it on synthetic examples; `scripts/fork/tests/test_public_text.py` does too and checks the tree. It runs in `task scripts` (the "Script self-tests" step of CI).

## Setup scripts named after a service

`setup-anthropic.sh`, `setup-anthropic-wif.py` and `store-wif-ids.sh` are kept under those names: they configure the one external service the sync workflow uses (keyless federation, docs/fork/WIF.md), the name says which service that is, and workflow comments and the walkthrough refer to them by name. They hold no credentials and no personal details; a rename would only break those references. `create-app.py` is named after what it creates (the fork's GitHub App).
