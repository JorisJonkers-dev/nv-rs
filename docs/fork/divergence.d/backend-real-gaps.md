### backend-real-gaps

- Files: `crates/engine-core/src/typed/mod.rs`
- Upstream approach: upstream marks incomplete translations with `// Translation incomplete:` comments and leaves them as they are; nothing registers other code at those addresses (checked against the files at `origin/fork` on 2026-11-27).
- Fork approach: `apply` ends with one call into the fork-only `typed/backend_gaps.rs`, which, with `NV_BACKEND_GAPS=1` and `NV_BACKEND_LIBRARY` set, registers the local decomp backend's function at each address of the generated list `typed/real_gaps.txt` that the backend has. `scripts/fork/real_gaps.py` writes that list from the markers, keeping the real gaps apart from the exception-frame and `/GS` notes. Without the switch nothing is registered.
- Decision: keep; drop with the backend when the guest memory is retired (nv-rs#916, track C).
- Reason: about 89 translations leave real behaviour out (x87 status words, uninitialised stack reads, register-only entries); the backend is the original code for them (nv-rs#930, ADR-0020).
- Evidence: nv-rs#930; the unit tests in `typed/backend_gaps.rs` and `scripts/fork/tests/test_real_gaps.py`; `.fork/private-integrity.sha256` is updated because `typed/mod.rs` is one of its inputs.
