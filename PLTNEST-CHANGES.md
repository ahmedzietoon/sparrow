# PLTNest changes to sparrow 0.3.0

Upstream: sparrow 0.3.0 at commit `3be9158`, jagua-rs `=1.0.0`. Branch `main` holds the untouched upstream tree (`aa7236c`). Branch `pltnest` holds these changes. MIT licence and file headers are unchanged.

Why: sparrow stops on wall-clock caps, so the layout depends on machine speed, and a killed run returns nothing (ADR 0020, sections 4.2 and 4.3). This fork adds a deterministic work budget, a best-so-far snapshot and settable counters.

## Changes

1. `Cargo.lock` (commit 1 on `pltnest`): git-ignored upstream; copied from the vendored tree (SHA-256 `56D7E8F2E2B8EA1B09BFC2BA3573DE17A023B74964827940C0DC2EF5A1AC8347`). Needed for `--locked`.
2. `src/util/io.rs`:
   - New flags on `MainCli`: `--max-separations N`, `--max-fails N`, `--cmpr-decay R`, `--snapshot PATH`, `--no-intermediate-svg`.
   - `parse_decay`: `R` must satisfy `0 < R < 1`.
   - `apply_config_overrides`: budget split, `max_conseq_failed_attempts` and `ShrinkDecayStrategy::FailureBased` from the new flags.
   - `ExtSPOutput`: optional `stop_reason`, written after `solution`. Omitted when `None`, so files without it still parse.
   - `write_json_atomic`: writes `<path>.tmp`, fsyncs, then renames over `<path>`.
3. `src/config.rs`: `max_separations: Option<usize>` in `ExplorationConfig` and `CompressionConfig` (default `None`).
4. `src/optimizer/separator.rs`: `Separator::total_separations` counts `separate()` calls.
5. `src/optimizer/explore.rs`: the loop also stops when the phase has used its budget.
6. `src/optimizer/compress.rs`: the loop also stops at the budget. With a budget set, the TimeBased decay follows the budget fraction instead of elapsed time. FailureBased is unchanged.
7. `src/optimizer/mod.rs`: `optimize` returns `(SPSolution, StopReason)`. `StopReason` is `in_progress | converged | budget | safety_stop`. A phase cut by a wall-clock cap is `safety_stop`, checked right after the phase, so a cut is never missed. Otherwise a phase that used its budget is `budget`, else `converged`. The run reports the most machine-dependent of the two phases. Logs `[OPT] separator calls: ...`.
8. `src/util/listener.rs`: `SolutionListener` for pairs `(A, B)`, forwarding every callback, so SVG and snapshot exporters both listen.
9. `src/util/snapshot_exporter.rs` (new): on every `ExplFeas` or `CmprFeas` report (each is a new best feasible solution), writes the final-output JSON with `stop_reason: "in_progress"` atomically. A failed write logs `[SNAP]` and the run continues; the previous snapshot stays valid.
10. `src/util/mod.rs`: registers `snapshot_exporter`.
11. `src/cli.rs`: wires the flags. `--no-intermediate-svg` skips the per-report SVGs; the final SVG is still written. The final JSON gets `stop_reason`.
12. `src/tui/runtime.rs`: follows the new `optimize` return value and the new `ExtSPOutput` field. Not compiled in this environment: the `tui` feature needs crossterm and ratatui, which are not in the offline cache.

Unchanged: all upstream defaults, including `-x` (10 failed attempts, decay 0.9) and the 0.8/0.2 time split. Without a budget the search path is the same as upstream (see Verification).

## Semantics

- Budget unit: one separator call (`Separator::separate`).
- `--max-separations N` gives exploration `floor(0.8 N)` calls and compression the rest, the same 0.8/0.2 split as `-t`. Unused exploration calls are not carried over.
- `-t`, `-e` and `-c` still apply as safety caps. Any cap that fires makes `stop_reason` `safety_stop` and the result is not reproducible. With a budget, set the caps above the expected run time.
- Output is reproducible for the same binary, input, seed, `--workers` value and budget, with no cap firing. Only `--workers 1` was tested; other values were not.
- `--max-fails` and `--cmpr-decay` enable their counter when given. `-x` sets both to the upstream defaults.
- Snapshot: `--snapshot PATH` holds the best feasible solution so far in the final-output schema, with `stop_reason: "in_progress"`. When the run completes, the last snapshot holds the same solution as the final output.

Known limit: a compression attempt that fails before it separates (a strip change error, unreachable because the width is pre-checked) does not count against the budget. Only the wall-clock caps would stop it in that path.

## Build

```
set RUSTUP_HOME=<legacy>\AZNestingWpf\obj\AZNestingRust\rustup   (toolchain 1.90.0)
set CARGO_HOME=<legacy>\AZNestingWpf\obj\AZNestingRust\cargo
set RUSTFLAGS=-C target-feature=+crt-static
cargo build --release --locked --offline --bin sparrow
```

Output: `target/release/sparrow.exe`, SHA-256 `fb714b64cc4a4265c9fe4a81238440264b09f277bdb4a17d5d8a506cde5b7983`, no `VCRUNTIME140.dll` import.

## Verification

Checks are in `2- Software/PLTNest/spikes/TS1-sparrow/results-fork/` (`fork_checks.py`, `determinism.json`, `cap.json`, `cpu.json`, `snapshot_kill.json`, `quality.json`, `extra_checks.json`). Overlaps and excursions use the TS1 harness check.

- Determinism (`--workers 1 -x -s 42 --max-separations 100 -t 600`, 3 reps per input): identical canonical output on D1 (`bedbf74e`), D2 (`be59b2f8`) and D3 (`c434f4a8`). Every run stopped on `budget` with 0 overlaps and all parts placed.
- Wall cap: `-t 60` and `-t 600` give identical canonical output on all three inputs.
- CPUs: 1 and 2 logical CPUs (affinity) give identical canonical output on all three inputs.
- Kill: three kills of D2 at 4, 8 and 12 s after the first snapshot. Each snapshot was valid JSON with `stop_reason: in_progress`, 0 overlaps, 0 excursion, all parts placed, and no `.tmp` left behind. Exit latency about 5 ms.
- Snapshot equals final output on all three inputs (rep 1 of the determinism runs).
- Default path: no budget, caps not reached, D1: the fork and vendored binary give the identical solution (strip 363.9008 mm, 18 of 18 parts).
- Safety label: D3 with `-e 5 -c 5` gives `stop_reason: safety_stop`.
- Quality, D3 (51 parts), budget 190 (152 exploration + 38 compression calls), `-t 600`: strip width 895.482 mm in 3 of 3 identical runs, about 16 s each. Vendored `-t 30` gives 898.172 mm in 32 s. TS1 `-t 600` gave 886.885 mm (ADR 0020, section 4.2).

Not done: a second PC (determinism across machines), a stop file (optional in ADR 0020 section 5), and a Ctrl-C test (the fork's snapshot replaces it).

The JSON key is `stop_reason` (snake case, like the rest of the schema), not `stopReason`.
