#!/usr/bin/env bash
# coda SessionStart hook — close prior session's summary debt opportunistically
coda=/home/jsy/.local/bin/coda
if [ -x "$coda" ]; then
    "$coda" close --apply --limit 50 >/dev/null 2>&1 &
fi
exit 0
