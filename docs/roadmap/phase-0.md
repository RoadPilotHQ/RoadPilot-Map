# Phase 0 plan

Phase 0 sets up the project foundation: documentation, an empty but working Rust workspace, quality checks and a test policy. It contains no map processing.

Each step is small, independently verifiable and committed separately.

## Steps

| # | Step | Verification | Status |
|---|---|---|---|
| 1 | Commit the pending `.gitignore` that protects local files. | Workflow script status shows a clean working tree. | Done |
| 2 | Add the initial architecture documentation and ADR 0001 in `docs/`, and link it from the root README. | Review of the documents. | Done |
| 3 | Create the Cargo workspace skeleton with the `roadpilot-map` facade library and the `roadpilot-map-cli` binary. The CLI supports `--version`. Add one unit test. | `cargo build` and `cargo test` pass. | Done |
| 4 | Pin the toolchain in `rust-toolchain.toml` and add `rustfmt` and `clippy` configuration. | `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings` pass. | Done |
| 5 | Add a `cargo-deny` configuration with a dependency license allow list and advisory checks. | `cargo deny check` passes. | Done |
| 6 | Register the build, test, format, lint and deny checks with the local Git workflow, and optionally add GitHub Actions CI. | The checks run before every commit; CI is green if added. | Planned |
| 7 | Define the fixture and test policy: a `tests/fixtures/` convention for synthetic data and an opt-in gate (`ROADPILOT_MAP_GAME_DIR`) for tests against a real installation. | The gated test is skipped by default and runs when the variable is set. | Planned |
| 8 | Implement `roadpilot-map doctor --game-dir <path>`: check that the folder looks like an ATS or ETS2 installation and list its `.scs` archives, without parsing them. | Tests against temporary fake installation folders pass. | Planned |

## Notes

- Step 6 changes configuration of the local Git workflow outside this repository and needs explicit approval before it is done.
- Step 8 is the first code that touches a game installation. It only reads folder contents and never parses or copies game files.
- No real game files or generated map data are added to the repository in any step.

## Exit criteria

Phase 0 is complete when:

- the documentation in `docs/` is reviewed and committed,
- the workspace builds, and tests, formatting, linting and dependency checks pass,
- the checks run automatically in the commit workflow,
- `roadpilot-map --version` and `roadpilot-map doctor` work as described.

## Next phase

Phase 1 covers the SCS archive format: documenting it in `docs/formats/`, building synthetic fixtures, and implementing `roadpilot-map-archive` to list and read archive entries.
