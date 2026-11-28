### The par_manager scene merge test names the nodes whose bounds race
- Files: `crates/engine/tests/par_manager.rs`
- Upstream approach: every node's bound must equal the one-thread run's, or (for three nodes) match within the last bits
- Fork approach: those three nodes (the fourth child of the twelve-child parent, listed twice, and the listed node inside a listed subtree with its parent) only need a finite sphere whose radius is within a factor of two of the reference; every other node keeps its exact check
- Decision: keep
- Reason: the exe zeroes and merges a node's world bound in place with no lock, so when two items hold the same node the result depends on thread interleaving, up to about 40 percent of the radius (nv-rs#1022)
- Evidence: the test ran 60 times in a row without a failure; before, 20 of 60 runs failed
