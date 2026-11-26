### Owner review of the private run's integrity hash
- Files: `.github/CODEOWNERS`
- Upstream approach: upstream has one CODEOWNERS line (`* @slaterain`) and no private porting run, so no file needs a named owner for re-pinning a checker hash
- Fork approach: the fork adds the maintainer as an owner of `.fork/private-integrity.sha256` and of each protected path (the judge `typed/diff.rs`, its thresholds `typed/mod.rs` and `x87.rs`, the control `diff_reference_peer.rs` and the ledger generator `scripts/ledger/`), so a change to any of them asks the maintainer to review it
- Decision: keep
- Reason: the private run must not accept a change to its own checker that a worker re-pinned in the same pull request (nv-rs#925); the public check fails on a stale hash and code ownership makes the re-pin an owner-reviewed change
- Evidence: `scripts/fork/tests/test_private_run.py` pins the CODEOWNERS entries and the check's wiring in `Taskfile.yml`; `task scripts` runs `private_integrity.py --check` in the required Pipeline Complete check
