### fnv_decomp licence
- Files: `LEGAL.md`, `docs/adr/0009-fnv-decomp.md`, `docs/adr/README.md`, `docs/HANDOFF.md`
- Upstream approach: `LEGAL.md` on `upstream/main` and `docs/adr/0009-fnv-decomp.md` call fnv_decomp's repository one that "grants no licence" (checked with `git show upstream/main:LEGAL.md`, 2026-11-26)
- Fork approach: the same paragraph says the repository granted no licence at the 2026-11-02 decision and that its own `LICENSE` has dedicated its contributors' own work under CC0 1.0 Universal since 2026-11-19; ADR-0009 records the date and source of the CC0 dedication beside the earlier permission, the ADR index marks it amended, and the handoff note says the same
- Decision: keep; drop the fork's wording when upstream records the CC0 dedication itself
- Reason: the legal notice must agree with the decision record and with the licence fnv_decomp's contributors adopted; a notice that still says no licence is granted contradicts both
- Evidence: fnv_decomp's own `LICENSE`, CC0 1.0 since pull request 45 merged on 2026-11-19 (merge commit `2b07cb3e`); `docs/adr/0009-fnv-decomp.md`; nv-rs#947
