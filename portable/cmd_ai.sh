#!/bin/sh
# cmd_ai launcher for Linux and macOS: POSIX sh, checked by shellcheck in CI.
#
# Runs the binary of this OS and CPU, found next to this script, with every argument:
#   Linux x86_64        -> cmd_ai-linux-x86_64
#   Linux aarch64/arm64 -> cmd_ai-linux-aarch64
#   macOS (any CPU)     -> cmd_ai-macos (universal binary)
#   Git Bash / MSYS2 / Cygwin on Windows -> cmd_ai.exe
# Works without its own exec bit (`sh cmd_ai.sh ...`) and through symbolic links; the folder
# is resolved from $0 without `readlink -f`, which old macOS releases lack.
#
# Exit codes of the launcher itself: 126 = binary present but not executable (or blocked by
# macOS), 127 = no binary for this system. Otherwise the exit code is cmd_ai's own.
# User-facing messages are in French (constitution, principle VII).

say() {
    printf '%s\n' "$@" >&2
}

# Parent folder of a path, without `dirname` ("." when the path has no slash).
parent_of() {
    case $1 in
        */*)
            p=${1%/*}
            printf '%s\n' "${p:-/}"
            ;;
        *) printf '.\n' ;;
    esac
}

# ---- folder of this script ----------------------------------------------------------------

script=$0
case $script in
    */*) ;;
    *)
        # `sh cmd_ai.sh` from its folder, or a shell that found it through PATH.
        if [ ! -f "$script" ]; then
            found=$(command -v "$script") && script=$found
        fi
        ;;
esac

# Follow symbolic links one hop at a time (portable replacement for `readlink -f`).
hops=0
while [ -L "$script" ] && [ "$hops" -lt 32 ]; do
    target=$(readlink "$script") || break
    case $target in
        /*) script=$target ;;
        *) script=$(parent_of "$script")/$target ;;
    esac
    hops=$((hops + 1))
done

if ! dir=$(CDPATH='' cd -P -- "$(parent_of "$script")" && pwd -P); then
    say "cmd_ai : impossible de trouver le dossier du lanceur ($0)."
    exit 127
fi

# ---- binary of this system ----------------------------------------------------------------

os=$(uname -s) || os=inconnu
arch=$(uname -m) || arch=inconnue

unsupported() {
    say "cmd_ai : ce système n'est pas pris en charge ($os, $arch)." \
        "Binaires disponibles : Windows x64 (cmd_ai.exe), Linux x86_64 et aarch64," \
        "macOS Intel et Apple Silicon."
    exit 127
}

case $os in
    Linux)
        case $arch in
            x86_64 | amd64) bin=cmd_ai-linux-x86_64 ;;
            aarch64 | arm64) bin=cmd_ai-linux-aarch64 ;;
            *) unsupported ;;
        esac
        ;;
    Darwin) bin=cmd_ai-macos ;;
    MINGW* | MSYS* | CYGWIN*) bin=cmd_ai.exe ;;
    *) unsupported ;;
esac
path=$dir/$bin

if [ ! -f "$path" ]; then
    say "cmd_ai : binaire introuvable pour ce système ($os, $arch) :" \
        "  $path" \
        "Le dossier cmd_ai doit contenir cmd_ai.exe, cmd_ai-linux-x86_64," \
        "cmd_ai-linux-aarch64 et cmd_ai-macos à côté de cmd_ai.sh." \
        "Décompressez à nouveau le paquet portable, téléchargé depuis :" \
        "  https://github.com/brandonviry/cmd_ai/releases"
    exit 127
fi

if [ ! -x "$path" ]; then
    # The execute bit may have been lost when the zip was extracted: try to restore it
    # (inside the cmd_ai folder only). It cannot help on FAT32 or on a noexec mount.
    chmod +x "$path" 2>/dev/null || :
fi

if [ ! -x "$path" ]; then
    say "cmd_ai : le binaire n'est pas exécutable :" \
        "  $path" \
        ""
    if [ "$os" = Darwin ]; then
        say "Rendez-le exécutable :" \
            "  chmod +x \"$path\"" \
            "Si macOS bloque ensuite le lancement (binaire non signé), retirez la quarantaine :" \
            "  xattr -d com.apple.quarantine \"$path\""
    else
        say "Causes possibles :" \
            "- La clé USB est formatée en FAT32 : Linux la monte avec l'option « showexec »," \
            "  qui retire le droit d'exécution aux fichiers sans extension .exe, .com ou .bat," \
            "  et chmod n'y peut rien. Reformatez la clé en exFAT (conseillé)." \
            "- Le support est monté avec l'option « noexec »." \
            "" \
            "En attendant, copiez le binaire sur ce PC et gardez la configuration de la clé" \
            "(cette copie laisse une trace sur la machine hôte) :" \
            "  cp \"$path\" ~/cmd_ai && chmod +x ~/cmd_ai && ~/cmd_ai --home \"$dir\""
    fi
    exit 126
fi

# ---- launch ---------------------------------------------------------------------------------

if [ "$os" = Darwin ] && xattr -p com.apple.quarantine "$path" >/dev/null 2>&1; then
    # Gatekeeper may kill a quarantined, unsigned binary on its first launch: keep this
    # shell alive to explain how to unblock it. A trap with a handler (not an ignored
    # signal) is reset in the child, so cmd_ai still receives Ctrl+C itself.
    trap ':' INT TERM HUP
    "$path" "$@"
    status=$?
    if [ "$status" -eq 137 ] || [ "$status" -eq 126 ]; then
        say "" \
            "cmd_ai : macOS a bloqué ce binaire (V1 non signée, Gatekeeper)." \
            "Retirez la quarantaine, puis relancez :" \
            "  xattr -d com.apple.quarantine \"$path\"" \
            "ou autorisez-le dans Réglages Système > Confidentialité et sécurité."
    fi
    exit "$status"
fi

exec "$path" "$@"
