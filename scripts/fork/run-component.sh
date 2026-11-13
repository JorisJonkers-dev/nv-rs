#!/usr/bin/env bash
# Runs the commands of one component job (SCRIPT, one command per line, from scripts/fork/components.py) and
# keeps the log readable: the compile of every `cargo test` is its own ::group:: (a `--no-run` build first, so
# its time is its own), each test binary's output is a group of its own (closed at the first failure so the
# panic messages stay visible), a failed test is an annotation, and
# the seconds spent compiling and testing go to $TIMINGS for libtest_summary.py. Never hides a failure: the exit
# status is non-zero when any command failed. Runs under Git Bash on the Windows runners, too.
set -uo pipefail

: "${SCRIPT:?the commands to run, one per line}"
# Outside the checkout: nvinspect's repository hygiene test reads every file git does not ignore, and a cargo log
# holds the runner's user paths.
OUT="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
LOG="${LOG:-$OUT/test.log}"
TIMINGS="${TIMINGS:-$OUT/timings.txt}"
: >"$LOG"
: >"$TIMINGS"

# Wraps each test binary's output in a group; a failed test is an annotation (GitHub shows ten per step).
group_binaries() {
  awk '
    function out(s) { print s; fflush() }
    function close_group() { if (open) { out("::endgroup::"); open = 0 } }
    /^ +(Running|Doc-tests) / { close_group(); out("::group::" $0); open = 1; next }
    /^failures:$/ { close_group(); out($0); next }
    /^test result: / { close_group(); out($0); next }
    /^test .* \.\.\. FAILED$/ { if (failed++ < 10) out("::error title=test failed::" $2); out($0); next }
    { out($0) }
    END { close_group() }
  '
}

rc=0
while IFS= read -r cmd; do
  [ -n "$cmd" ] || continue
  case "$cmd" in
    "cargo test "*)
      if [[ "$cmd" != *" --doc"* && "$cmd" != *" --no-run"* ]]; then
        echo "::group::compile: $cmd"
        start=$SECONDS
        bash -c "$cmd --no-run" 2>&1 | tee -a "$LOG"
        status=${PIPESTATUS[0]}
        echo "::endgroup::"
        echo "compile $((SECONDS - start))" >>"$TIMINGS"
        if [ "$status" -ne 0 ]; then
          echo "::error title=compile failed::$cmd"
          rc=1
          continue
        fi
      fi
      echo "tests: $cmd"
      start=$SECONDS
      bash -c "$cmd" 2>&1 | tee -a "$LOG" | group_binaries
      status=${PIPESTATUS[0]}
      echo "test $((SECONDS - start))" >>"$TIMINGS"
      if [ "$status" -ne 0 ]; then
        echo "::error title=tests failed::$cmd"
        rc=1
      fi
      ;;
    *)
      echo "::group::$cmd"
      start=$SECONDS
      bash -c "$cmd" 2>&1 | tee -a "$LOG"
      status=${PIPESTATUS[0]}
      echo "::endgroup::"
      echo "other $((SECONDS - start))" >>"$TIMINGS"
      if [ "$status" -ne 0 ]; then
        echo "::error title=command failed::$cmd"
        rc=1
      fi
      ;;
  esac
done <<<"$SCRIPT"
exit "$rc"
