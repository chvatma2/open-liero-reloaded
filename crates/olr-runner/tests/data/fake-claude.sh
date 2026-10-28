#!/usr/bin/env bash
# A stand-in for `claude -p … --output-format stream-json`, driven by
# `fake.env` in the working directory, or `target/fake.env` (where the
# runner's workspace keeps it out of git's way):
#   FAKE_MODE    done | silent | linger | limit-then-done | crash | commit
#   FAKE_RESULT  the result file to write (done, limit-then-done, commit)
#   FAKE_FILE, FAKE_SUBJECT  what `commit` commits
#   FAKE_LIMIT_TEXT  the limit message (default: resets 3:45pm)
#   FAKE_BEFORE  a shell command run first (e.g. push to origin)
#   FAKE_STATUS  the result file's first line (default: done)
set -u
envf=fake.env; [ -f "$envf" ] || envf=target/fake.env
[ -f "$envf" ] && . "./$envf"
resume=no
for a in "$@"; do [ "$a" = "--resume" ] && resume=yes; done
all="$*"; echo "args: ${all//$'\n'/ }" >> "$(dirname "$envf")/fake.calls"
[ -n "${FAKE_BEFORE:-}" ] && eval "$FAKE_BEFORE"

activity() {
  echo '{"type":"system","subtype":"init"}'
  echo '{"type":"assistant","message":{"content":[]}}'
}
finish() {
  mkdir -p "$(dirname "$FAKE_RESULT")"
  printf '%s\nDid the thing.\n' "${FAKE_STATUS:-done}" > "$FAKE_RESULT"
  echo '{"type":"result","subtype":"success","is_error":false,"result":"Finished."}'
}

case "${FAKE_MODE:-done}" in
  done) activity; finish ;;
  silent) activity; sleep 30 ;;
  # Finishes, then waits on a background task that never ends.
  linger) activity; finish; sleep 30 ;;
  limit-then-done)
    activity
    if [ "$resume" = yes ]; then finish; else
      text=${FAKE_LIMIT_TEXT:-"You've hit your session limit · resets 3:45pm"}
      echo "{\"type\":\"result\",\"is_error\":true,\"result\":\"$text\"}"
    fi ;;
  crash) activity; echo "boom" >&2; exit 3 ;;
  commit)
    activity
    mkdir -p "$(dirname "$FAKE_FILE")"
    echo "change" > "$FAKE_FILE"
    git add -A -- "$FAKE_FILE"
    git -c user.name=t -c user.email=t@t commit -q -m "$FAKE_SUBJECT"
    finish ;;
esac
