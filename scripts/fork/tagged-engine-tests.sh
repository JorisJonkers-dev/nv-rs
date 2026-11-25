#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# The engine's tests that the coverage catalogue tags (.fork/catalogue/covers.d/engine_typed.tsv), for the macOS and
# Linux jobs of fork-ci.yml. Built once (`cargo test --no-run`), then the test binaries run side by side, JOBS at a
# time (default: the runner's cores): one after another, as `cargo test` runs them, they took 11 to 12.5 minutes,
# the face family's differential tests alone about 10, on one core of three or four.
# Each binary's output goes to the log after a `Running tests/<file>.rs (<executable>)` line, as cargo writes it, so
# `tally ingest --libtest` reads the log as before. Never hides a failure: the exit status is non-zero when the build
# or any binary failed.
# Usage: tagged-engine-tests.sh <log file>
set -uo pipefail

LOG="${1:?the log file to write}"
JOBS="${JOBS:-$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 2)}"
WORK="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/tagged-engine.XXXXXX")"
mkdir -p "$(dirname "$LOG")"

files=()
while IFS= read -r t; do files+=("$t"); done < <(
  grep -o '^crates/engine/tests/[a-z0-9_]*\.rs' .fork/catalogue/covers.d/engine_typed.tsv | sed 's|.*/||; s|\.rs$||' | sort -u)
if [ ${#files[@]} -eq 0 ]; then
  echo "no tagged engine test files" | tee "$LOG"
  exit 0
fi
args=()
for t in "${files[@]}"; do args+=(--test "$t"); done

echo "::group::Build of ${#files[@]} test files"
start=$SECONDS
# The JSON lines name each test binary; the compiler's own messages still go to the job's log.
if ! cargo test --locked --no-run -p engine "${args[@]}" --message-format=json-render-diagnostics >"$WORK/build.json"; then
  echo "::endgroup::"
  echo "::error title=Tagged engine tests::the build failed"
  echo "the build failed" >"$LOG"
  exit 1
fi
echo "::endgroup::"
echo "Built in $((SECONDS - start)) s"

# "<stem> <executable>" per test file.
jq -r 'select(.reason == "compiler-artifact" and .executable != null and (.target.kind | index("test")))
       | "\(.target.name) \(.executable)"' "$WORK/build.json" | sort -u >"$WORK/binaries"
if [ "$(wc -l <"$WORK/binaries")" -ne ${#files[@]} ]; then
  echo "::error title=Tagged engine tests::expected ${#files[@]} test binaries, the build named $(wc -l <"$WORK/binaries")"
  cat "$WORK/binaries"
  exit 1
fi

# One binary: its output to <stem>.out, its exit status to <stem>.status. Run from the package's folder, as cargo does.
# shellcheck disable=SC2329 # run by xargs below
run_one() {
  local stem="$1" exe="$2"
  (cd crates/engine && "$exe") >"$WORK/$stem.out" 2>&1
  echo $? >"$WORK/$stem.status"
  echo "$stem: exit $(cat "$WORK/$stem.status")"
}
export -f run_one
export WORK

start=$SECONDS
# The face family's tests first: they are the long ones, so the short ones fill the cores around them.
# shellcheck disable=SC2016 # expanded by the child shell
{ grep ' ' "$WORK/binaries" | grep facegen; grep ' ' "$WORK/binaries" | grep -v facegen; } |
  xargs -P "$JOBS" -L 1 bash -c 'run_one "$0" "$1"'
echo "Ran in $((SECONDS - start)) s, $JOBS at a time"

failed=0
: >"$LOG"
while read -r stem exe; do
  {
    echo "     Running tests/$stem.rs ($exe)"
    cat "$WORK/$stem.out"
    echo
  } >>"$LOG"
  status="$(cat "$WORK/$stem.status" 2>/dev/null || echo missing)"
  if [ "$status" != 0 ]; then
    echo "::error title=Tagged engine tests::tests/$stem.rs failed (exit $status)"
    failed=1
  fi
done <"$WORK/binaries"
cat "$LOG"
exit "$failed"
