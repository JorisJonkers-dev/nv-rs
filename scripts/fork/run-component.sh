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

# Tests that can only pass on Windows are skipped on Linux by name (.fork/linux-skip.txt: one libtest name filter
# per line, the reason in the comment), so that upstream's test files carry no Linux-only ignore attributes.
skips=""
if [ "$(uname -s)" = Linux ] && [ -f .fork/linux-skip.txt ]; then
  while IFS= read -r name; do
    name=${name%%#*}
    name=${name//[[:space:]]/}
    [ -n "$name" ] && skips="$skips --skip $name"
  done <.fork/linux-skip.txt
fi

# ARCHIVE: the folder of a test archive (scripts/fork/test_archive.py), already extracted into the checkout. A `cargo test -p P
# --test A --test B` command of the job then runs the archived binaries A and B of P, from P's folder as cargo does, and
# compiles nothing: the archive's build job compiled every binary of the group once. SHARD splits the one binary's tests
# as below.
archived_tests() {
  local cmd="$1" crate="" tests=() w i found t rc=0 picked
  declare -A exes cwds
  read -ra w <<<"$cmd"
  for ((i = 0; i < ${#w[@]}; i++)); do
    case "${w[i]}" in
      -p) crate="${w[i + 1]}" ;;
      --test) tests+=("${w[i + 1]}") ;;
    esac
  done
  for t in "${tests[@]}"; do
    if ! found="$("${PY:-python3}" -I scripts/fork/test_archive.py find "$ARCHIVE" "$crate" "$t")"; then
      echo "::error title=archive::the archive has no test binary $crate/$t"
      return 1
    fi
    exes[$t]="$PWD/${found%%$'\t'*}"
    cwds[$t]="${found#*$'\t'}"
  done
  if [ -n "${SHARD:-}" ]; then
    # SHARD=i/n: every n-th test of the job's binaries together, sorted by binary and name; the other shards run the rest.
    picked="$(for t in "${tests[@]}"; do
      (cd "${cwds[$t]}" && "${exes[$t]}" --list 2>/dev/null) | sed -n "s/^\(.*\): test$/$t \1/p"
    done | LC_ALL=C sort | awk -v i="${SHARD%/*}" -v n="${SHARD#*/}" '(NR - 1) % n == i - 1')"
    if [ -z "$picked" ]; then echo "::error title=shard::no tests listed for shard $SHARD"; return 1; fi
    echo "shard $SHARD: $(wc -l <<<"$picked") tests"
  fi
  for t in "${tests[@]}"; do
    local args=()
    if [ -n "${SHARD:-}" ]; then
      names="$(awk -v t="$t" '$1 == t { print $2 }' <<<"$picked")"
      [ -n "$names" ] || continue
      # shellcheck disable=SC2206 # the names are a word list
      args=(--exact $names)
    fi
    echo "     Running tests/$t.rs (${exes[$t]})"
    # shellcheck disable=SC2206 # the skips are a word list
    args+=($skips)
    (cd "${cwds[$t]}" && CARGO_MANIFEST_DIR="$PWD" CARGO_PKG_NAME="$crate" "${exes[$t]}" "${args[@]}") || rc=1
  done
  return "$rc"
}

rc=0
while IFS= read -r cmd; do
  [ -n "$cmd" ] || continue
  case "$cmd" in
    "cargo test "*" --test "*)
      if [ -n "${ARCHIVE:-}" ] && [[ "$cmd" != *" --lib"* && "$cmd" != *" --doc"* ]]; then
        echo "tests: $cmd (archived)"
        start=$SECONDS
        archived_tests "$cmd" 2>&1 | tee -a "$LOG" | group_binaries
        status=${PIPESTATUS[0]}
        echo "test $((SECONDS - start))" >>"$TIMINGS"
        if [ "$status" -ne 0 ]; then
          echo "::error title=tests failed::$cmd"
          rc=1
        fi
        continue
      fi
      ;&
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
      run="$cmd"
      [[ "$cmd" == *" --doc"* || -z "$skips" ]] || run="$cmd --$skips"
      if [ -n "${SHARD:-}" ]; then
        # SHARD=i/n: this job runs every n-th test (sorted by name) of the one test binary of the command; the other
        # shards run the rest. Names are matched exactly, so a name that is a prefix of another does not pull it in.
        names="$(bash -c "$cmd -- --list" 2>/dev/null | sed -n 's/^\(.*\): test$/\1/p' | LC_ALL=C sort | awk -v i="${SHARD%/*}" -v n="${SHARD#*/}" '(NR - 1) % n == i - 1')"
        if [ -z "$names" ]; then echo "::error title=shard::no tests listed for shard $SHARD"; rc=1; continue; fi
        echo "shard $SHARD: $(wc -l <<<"$names") tests"
        run="$cmd -- --exact $(tr '\n' ' ' <<<"$names")$skips"
      fi
      bash -c "$run" 2>&1 | tee -a "$LOG" | group_binaries
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
