# Deslop: test harness and scripts

Scope: `tests/`, `scripts/`, `docker/`, `native/` (9.6k first-party lines of Python and shell). Base `e39e5018`; worktree branch `deslop/p2`. Commands run with `RUSTC_WRAPPER=`, `TMPDIR=$PWD/.work/tmp`. Python tools came from the stuff checkout's `.venv` (`ruff 0.17.4`, `ty 0.0.101`) run with `--isolated`, so nothing was installed. Only headless gates ran.

## DSLP-9 Types

```
F=(tests/*.py scripts/*.py)
ty check --output-format concise $F
ruff check --isolated --select <rule> --output-format concise $F
shellcheck -x -s bash scripts/*.sh tests/run.sh docker/linux/*.sh
```

| Candidate | Hits | Disposition |
| --- | ---: | --- |
| ty, default rules | 11 | Rejected, none real. `scripts/bench.py:19` unresolved `harness` (the `sys.path.insert` import); `bench.py:145,154,178,184` come from a dict literal of mixed value types; `harness.py:108,136` `Popen.stdout` is optional by type but set by `stdout=PIPE`; `harness.py:388,392` `width` starts as `None` and is set before the only use (the `color_type` check raises first); `test_native_rpaths.py:45,97` `shutil.which("kache")` in a class that `skipUnless` it. |
| Ruff `F401` | 1 | Adopted by removal: unused `Failure` import in `tests/gui.py` (see DSLP-17). |
| Ruff `B904` | 1 | Rejected: `harness.py:349` re-raises `KeyError` as `Failure`; only the traceback chain changes. |
| Ruff `B905` | 2 | Rejected: `perfstat.py:176,179`, `compare_paired` returns `invalid` for unequal lengths at line 150 before the zips. |
| Ruff `RET503` | 1 | Rejected: `perfstat.py:257` loops over `itertools.count()`. |
| Ruff `RUF100` | 8 | Rejected: seven `# noqa: E402` guard the `sys.path` import pattern and one `BLE001` explains a broad catch; they only look unused because the repository has no Ruff configuration. |
| Ruff `ANN` | 790 | Rejected: no function in about 6.8k lines of Python is annotated; no sampled defect, and the scripts have no typed consumers. |
| Ruff `ANN401`, `PGH003`, `TC`, `E722` | 0 | Nothing to adopt. |
| Ruff `S` | 134 | Rejected: subprocess and `assert` in a harness that runs trusted local commands. |
| shellcheck | 3 warnings, 1 hazard | `SC2034` on three unused `*_REV` variables, see DSLP-17. `SC2115` at `install-layout.sh:43` (`rm -rf "$prefix/lib"` with an unchecked `$prefix`); every caller passes a fixed directory, so it is a finding, not a change. `cargo-rustflags.sh` has no shebang because it is sourced; the remaining notes are `SC2012` on `ls` and `SC1091` on sourced files. |

Adopted profile: none; the repository has no Python lint or type step and this pass adds none. Python escape hatches (`rg -c 'cast\(|# type: ignore|# ty: ignore|\bAny\b|# noqa'`): 9 before, 9 after. No contract changed type, so there is nothing to witness.

## DSLP-13 Tests

Pyramid and costs, warm, on a busy host:

| Tier | What | Cost |
| --- | --- | ---: |
| Unit | `scripts/test_perfstat.py`, 20 tests | 0.6 s, but `ProcessCpuTests` failed in one run under load (below) |
| Unit | `tests/test_harness.py`, 7 tests, new | 0.003 s |
| Unit | `test_bundle.py` 2.0 s, `test_linux_container.py` 0.4 s, `test_release_archives.py` 0.4 s; `test_build_flags.py` and `test_linux_audit.py` skip on macOS | under 3 s |
| Slow unit | `test_native_rpaths.py` runs `cargo build --offline` twice: 254 s; `test_build_deps.py`: 235 s per DSLP-19 | see `ut-all` below |
| Gates | `tests/run.sh <gate>` against `build/mpv`; `traces` (2 checks) and `unit` (2 checks, 1.5 s) need no player | |

False pass or flake found: `ProcessCpuTests.test_busy_child_reports_cpu` and `test_sampling_resolves_below_ten_milliseconds` assert more than 50 ms of CPU inside a 100 ms wall window; both failed in one run (`0.0478 not greater than 0.05`, `0.0347`) when the host was busy. They stay out of the suite.

Change: a `unit` gate in `tests/harness.py` (in `SUITE` and `LINUX_SUITE`) runs the deterministic `test_perfstat.py` classes and `test_harness.py`, so Python tests reach `make test` and CI for about 1.5 s (DSLP-19's cheap half). The two 4-minute files stay outside, and CI workflow files were not touched.

| Contract | Guarding test | Disposition |
| --- | --- | --- |
| Exit code 0 only when every check passed | `tests/test_harness.py` `test_results_exit_zero_only_when_every_check_passed`, `test_unknown_gate_fails_the_run`, `test_a_gate_that_raises_is_a_failed_check_not_a_crash` | New. Fault below. |
| No gate prints usage and exits 2; unknown gate fails | `test_no_gate_prints_usage_and_exits_two`, `test_unknown_gate_fails_the_run` | New. |
| Gate list: `SUITE`, `LINUX_SUITE` name registered gates once | `test_suites_name_only_registered_gates_once` | New. All 21 and 20 names resolve at base. |
| Fixture manifest: every `fixture_entry("name")` exists | `test_every_fixture_the_gates_name_is_in_the_manifest` | New: a typo used to fail only when that gate ran, headfully for some. |
| Fixture byte hashes | gate `fixtures` | Known red on this host (DSLP-18); untouched. |
| Trace format | gate `traces` (`tests/trace.py --check`) | 2/2 pass. |
| Release archive layout | `tests/test_release_archives.py:13` (CI runs it) | Passes. |
| Perf statistics | `scripts/test_perfstat.py` (`ComparePairedTests:88` and four more classes) | Passes. |
| `scripts/*.sh` | `tests/test_linux_container.py`, `test_build_flags.py` (Linux), `test_bundle.py` | Partly guarded; the audit and measure scripts run only in owner or container runs. |

Injected fault (critical decision: what exit code `tests/run.sh` reports). In `Results.finish`, `return 0 if failed == 0 else 1` became `return 0`:

```
cp tests/harness.py .work/tmp/harness.py.orig   # then edit
python3 -m unittest discover -s tests -p test_harness.py   # exit 1: FAILED (failures=3)
  test_results_exit_zero_only_when_every_check_passed, test_unknown_gate_fails_the_run,
  test_a_gate_that_raises_is_a_failed_check_not_a_crash
cp .work/tmp/harness.py.orig tests/harness.py
cmp tests/harness.py .work/tmp/harness.py.orig && echo cmp-ok   # exit 0
```

Acceptance commands: `python3 -m unittest discover -s scripts -p 'test_*.py'` passes (20 tests) on a quiet host and failed 2 tests under load; `python3 -m unittest discover -s tests -p 'test_*.py'` ran 23 tests in 352 s (5 skipped), exit 0, including `test_harness.py`. `make test` was not run: it builds the release app and runs the windowed gates (`gui`, `gpu-api`, `tui`, the macOS window bridge), which the owner deferred.

Needs environment: `coverage.py` and `mutmut` are not installed on this host, so there is no coverage or mutation run for the harness; the required-scope mutation acceptance is unmet for lack of tools. Warm `make test` cost is unmeasured; the gate adds 1.5 s. Headful-only: `gui`, `gpu-api`, `tui` (some cases without `--vo=null`), `hwdec`, `linux-*` gates (Linux container).

## DSLP-17 Dead code

Tools available here: Ruff `F401`/`F841`, a name-reference script, shellcheck. `vulture`, `deptry`, `cargo-machete` are not installed.

| Finding | Searches | Disposition |
| --- | --- | --- |
| `Failure` imported in `tests/gui.py:10` and unused (ruff `F401`) | `rg '\bFailure\b' tests/gui.py` empty; `rg 'from gui\|import gui\|gui\.Failure'` shows only `from gui import run`, `PASS_LINE` | Removed (commit `e1be6e97`). `import gui` still loads. |
| Python top-level `def`, `class` and constants with at most one reference across `tests`, `scripts`, workflows, Makefile, docs | script run over 18 files | Only `unittest.TestCase` classes; nothing dead. |
| Gates registered but in neither suite: `build`, `cli-help` (called by `cli`), `gui`, `linux-audit`, `linux-vaapi`, `meta`, `mpv-diff`, `play` | `rg "run.sh <gate>"` over docs, workflows, scripts | Kept: on-demand gates are a contract. `meta`, `mpv-diff`, `play` and `build` have no doc or workflow reference; consumer question below. |
| Scripts with one reference (`measure-linux.sh`, `package-macos.sh`, `package-linux.sh`, `linux-check.sh`, `dev-deps.sh`, `manual-checklist.sh`) | `rg --fixed-strings` | All referenced by a workflow, a script, a test or a doc. |
| `GLSLANG_REV`, `SPIRV_HEADERS_REV`, `SPIRV_TOOLS_REV` in `scripts/build-deps-linux.sh:95-97` | shellcheck `SC2034`; `rg` finds only the same names in `build-deps.sh`, which exports them without reading them either | Not removed: both scripts are hashed into the dependency cache key (`build-deps-linux.sh:23`, `ci.yml:43`, `macos.yml`), so an edit forces a rebuild of the Linux and macOS prefixes that cannot be checked here. Finding below. |
| `docker/linux/*`, `native/*` | `musl-cc-wrap`, `rust-fontconfig.Cargo.lock`, `gitversion.h.in`, `native/spirv-cross`, the prebuilt shaderc archive | Each referenced by a script, test or doc. |

Platforms: macOS aarch64; Linux scripts were read and shellchecked, not run. Consumer tests: `python3 -m unittest discover -s tests -p test_*.py` (gui is imported by `harness.py` gates), `tests/run.sh unit` pass.

## Findings

- P3 · `meta` gate is red at base and outside both suites. `tests/run.sh meta` reports `meta: no system temp paths` (it matches `$TMPDIR` in `scripts/fetch-gluon.sh` and a fake `TMUX=/tmp/...` value in `harness.py:861`) and `meta: .git/info/exclude` (false in a linked worktree). Fix the pattern or retire the gate.
- P3 · `scripts/test_perfstat.py` `ProcessCpuTests` fail on a busy host; make the CPU thresholds relative or skip under load before they enter the suite.
- P3 · Two Python unit files take about four minutes each (`test_build_deps.py`, `test_native_rpaths.py`); they stay outside `make test` and CI. DSLP-19 stays open for them.
- P4 · `scripts/install-layout.sh:43` runs `rm -rf "$prefix/lib"` with no check that `$prefix` is non-empty; add `[ -n "$prefix" ]` beside the usage check.
- P4 · `GLSLANG_REV`, `SPIRV_HEADERS_REV`, `SPIRV_TOOLS_REV` are dead in both build-deps scripts; remove them in a change that accepts one dependency-cache rebuild.
- P4 · `tests/run.sh` gates `play`, `mpv-diff`, `meta`, `build` have no documented caller; confirm they are still wanted.
