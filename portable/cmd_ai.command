#!/bin/sh
# macOS launcher for a double-click in the Finder: Terminal starts it from the user's home
# folder, so go to this file's own folder first, then hand over to cmd_ai.sh, which picks
# the binary and explains (in French) what to do when macOS blocks it.

case $0 in
    */*) here=${0%/*} ;;
    *) here=. ;;
esac

if ! CDPATH='' cd -- "${here:-/}"; then
    printf '%s\n' "cmd_ai : impossible d'ouvrir le dossier de cmd_ai.command ($0)." >&2
    exit 127
fi

exec sh ./cmd_ai.sh "$@"
