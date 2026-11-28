### decomp-backend-build-output
- Files: `.gitignore`
- Upstream approach: upstream ignores `target/`, `dist/` and its game-data captures, but has no build tool that writes a native library into the checkout and no ignore entry for one (checked at the branch point `5a6abade9`)
- Fork approach: ignore `/.decomp-backend/`, the folder `scripts/fork/decomp_backend.py` uses for its build copy, wasm module, generated C and native library. The tool's hygiene command refuses any other built artefact visible to git
- Decision: keep; drop when upstream builds the decomp backend itself and names an output folder the fork can use
- Reason: the local backend is built from the public decomp on the user's machine and nothing it produces may be committed or uploaded (nv-rs#916, ADR-0020). An explicit ignore rule keeps the whole output out of the tree even when the tool is run from the repository root
- Evidence: nv-rs#926; `scripts/fork/tests/test_decomp_backend.py` checks both the ignore entry and that built artefacts are refused
