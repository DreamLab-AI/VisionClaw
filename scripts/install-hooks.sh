#!/usr/bin/env bash
#
# install-hooks.sh — install this repository's tracked git hooks, and
# optionally provision the shared git-gen-utils environment they use.
#
#   ./scripts/install-hooks.sh                  # install the hooks
#   ./scripts/install-hooks.sh --with-git-gen   # ...and provision git-gen-utils
#   ./scripts/install-hooks.sh --uninstall      # remove the hooks we installed
#   ./scripts/install-hooks.sh --status         # report what is installed
#
# Hooks are symlinked one by one into the repository's effective hooks
# directory rather than by repointing core.hooksPath, so hooks installed by
# hand (scripts/pre-commit-validate.sh, for example) keep working. Worktrees
# share the common git dir, so installing once covers every worktree.
#
# The git-gen-utils environment is shared and lives under
# $XDG_CACHE_HOME/git-gen-utils/venv. It is never created during a commit:
# git-gen-utils pulls in llama-cpp-python, which on PyPI ships as a source
# distribution only, so an implicit install compiles a C++ tree and a commit
# hangs for many minutes. We install it here, once, from the upstream CPU
# wheel index, with --only-binary so a compile is an error rather than a wait.

set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"

# The hooks directory is shared by every worktree, so whatever we plant in it
# must outlive any one worktree. When the main working tree (the parent of the
# common git dir) has the tracked hooks we symlink to those and updates arrive
# for free. When it does not — main checked out on a branch that predates
# .githooks, which is exactly the situation when this lands — we copy instead,
# so removing the worktree does not leave a dangling hook behind.
main_worktree="$(dirname "$(cd "$(git rev-parse --git-common-dir)" && pwd)")"
if [[ -d "$main_worktree/.githooks" ]]; then
    hooks_src="$main_worktree/.githooks"
    link_hooks=1
else
    hooks_src="$repo_root/.githooks"
    link_hooks=0
fi
venv="${GIT_GEN_UTILS_VENV:-${XDG_CACHE_HOME:-$HOME/.cache}/git-gen-utils/venv}"

# Upstream's prebuilt CPU wheels. PyPI carries the sdist only.
LLAMA_CPP_CPU_INDEX="https://abetlen.github.io/llama-cpp-python/whl/cpu"

if [[ -t 1 && -z "${NO_COLOR:-}" ]]; then
    C_BOLD=$'\e[1m'; C_GREEN=$'\e[32m'; C_YELLOW=$'\e[33m'; C_RED=$'\e[31m'; C_OFF=$'\e[0m'
else
    C_BOLD=''; C_GREEN=''; C_YELLOW=''; C_RED=''; C_OFF=''
fi

info()  { printf '%s\n' "$*"; }
ok()    { printf '%s✔%s %s\n' "$C_GREEN" "$C_OFF" "$*"; }
warn()  { printf '%s!%s %s\n' "$C_YELLOW" "$C_OFF" "$*" >&2; }
die()   { printf '%s✘%s %s\n' "$C_RED" "$C_OFF" "$*" >&2; exit 1; }

# The directory git will actually look in. Honour an existing core.hooksPath
# (it may already point at the common hooks dir by absolute path) and fall
# back to the common git dir, which every worktree of this clone shares.
hooks_dir() {
    local configured
    if configured="$(git config --get core.hooksPath 2>/dev/null)" && [[ -n "$configured" ]]; then
        if [[ "$configured" = /* ]]; then
            printf '%s\n' "$configured"
        else
            printf '%s\n' "$repo_root/$configured"
        fi
        return
    fi
    printf '%s\n' "$(cd "$(git rev-parse --git-common-dir)" && pwd)/hooks"
}

# Did we put this hook here? True for a symlink back to the tracked file, and
# for a byte-identical copy of it.
is_ours() {
    local target="$1" src="$2"
    [[ -L "$target" && "$(readlink "$target")" == "$src" ]] && return 0
    # Content comparison in bash: hooks are small text files, and cmp/diff are
    # not present on every minimal image this repo is built in.
    [[ -f "$target" && "$(<"$target")" == "$(<"$src")" ]] && return 0
    return 1
}

install_hooks() {
    local dest; dest="$(hooks_dir)"
    mkdir -p "$dest"

    local src name target
    shopt -s nullglob
    for src in "$hooks_src"/*; do
        [[ -f "$src" ]] || continue
        name="$(basename "$src")"
        target="$dest/$name"

        if [[ -e "$target" && ! -L "$target" ]] && ! is_ours "$target" "$src"; then
            mv -f "$target" "$target.replaced-by-install-hooks"
            warn "moved the previous $name aside to $name.replaced-by-install-hooks"
        fi

        chmod +x "$src"
        if (( link_hooks )); then
            ln -sfn "$src" "$target"
            ok "installed $name -> $src"
        else
            rm -f "$target"
            cp "$src" "$target"
            chmod +x "$target"
            ok "installed $name (copied from $src)"
            warn "$main_worktree has no .githooks yet, so this is a snapshot."
            warn "Re-run this script once that branch is checked out to switch to a symlink."
        fi
    done
    shopt -u nullglob

    info "hooks directory: $dest"
}

uninstall_hooks() {
    local dest; dest="$(hooks_dir)"
    local src name target
    shopt -s nullglob
    for src in "$hooks_src"/*; do
        name="$(basename "$src")"
        target="$dest/$name"
        if is_ours "$target" "$src"; then
            rm -f "$target"
            ok "removed $name"
        fi
    done
    shopt -u nullglob
}

provision_git_gen() {
    command -v python3 >/dev/null 2>&1 || die "python3 is required to provision git-gen-utils"

    if [[ -x "$venv/bin/git-gen" && -x "$venv/bin/python3" ]]; then
        ok "git-gen-utils already provisioned at $venv"
        return
    fi

    # A half-built or relocated venv (stale shebangs) is worse than none.
    if [[ -d "$venv" && ! -x "$venv/bin/python3" ]]; then
        warn "removing a broken environment at $venv"
        rm -rf "$venv"
    fi

    info "${C_BOLD}provisioning git-gen-utils${C_OFF} into $venv (one off, a few minutes)"
    mkdir -p "$(dirname "$venv")"
    python3 -m venv "$venv"
    "$venv/bin/python3" -m pip install --upgrade --quiet pip wheel

    # --only-binary on llama-cpp-python is the whole point: if no wheel matches
    # this interpreter and platform we want a clear failure here, not a silent
    # multi-minute compile that would otherwise land inside someone's commit.
    if ! "$venv/bin/python3" -m pip install \
        --prefer-binary \
        --only-binary=llama-cpp-python \
        --extra-index-url "$LLAMA_CPP_CPU_INDEX" \
        git-gen-utils; then
        rm -rf "$venv"
        die "could not install git-gen-utils; see pip's output above.
   Platform: $(python3 -c 'import platform,sys; print(platform.machine(), "py%d.%d" % sys.version_info[:2])'), wheel index: $LLAMA_CPP_CPU_INDEX
   A 'no matching distribution' error there means no prebuilt llama-cpp-python
   wheel exists for this interpreter; we refuse to build one from source
   because that is a multi-minute C++ compile.
   Leaving it uninstalled is a supported state: the hook degrades to one
   printed line and commits stay fast."
    fi

    ok "git-gen-utils installed into $venv"

    # Prove it actually starts. The llama-cpp-python wheel links against the
    # system libstdc++, which minimal or Nix-composed images may not provide;
    # a wheel that imports cleanly on the build host can still fail here. Far
    # better to learn that now than from a hook at commit time.
    if ! "$venv/bin/git-gen" --help >/dev/null 2>&1; then
        warn "git-gen is installed but does not start on this machine. Diagnose with:"
        warn "  $venv/bin/git-gen --help"
        warn "A missing libstdc++.so.6 is the usual cause on minimal images."
        warn "Commits are unaffected: the hook notices and skips generation."
        return
    fi

    ok "git-gen starts cleanly"
    info "It downloads its model from Hugging Face on first generation. Run"
    info "  $venv/bin/git-gen --path $repo_root"
    info "once now rather than waiting for it inside a commit."
}

status() {
    local dest; dest="$(hooks_dir)"
    info "${C_BOLD}hooks directory${C_OFF} $dest"
    local src name target
    shopt -s nullglob
    for src in "$hooks_src"/*; do
        name="$(basename "$src")"; target="$dest/$name"
        if is_ours "$target" "$src"; then
            ok "$name installed"
        elif [[ -e "$target" ]]; then
            warn "$name present but not ours: $target"
        else
            warn "$name not installed"
        fi
    done
    shopt -u nullglob

    if [[ -x "$venv/bin/git-gen" ]]; then
        ok "git-gen-utils present at $venv"
    else
        info "git-gen-utils absent (message generation disabled; hooks still fine)"
    fi
}

main() {
    local with_git_gen=0 action=install
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --with-git-gen) with_git_gen=1 ;;
            --uninstall)    action=uninstall ;;
            --status)       action=status ;;
            -h|--help)      sed -n '2,25p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
            *)              die "unknown argument: $1" ;;
        esac
        shift
    done

    [[ -d "$hooks_src" ]] || die "no .githooks directory at $hooks_src"

    case "$action" in
        install)
            install_hooks
            if (( with_git_gen )); then provision_git_gen; fi
            ;;
        uninstall) uninstall_hooks ;;
        status)    status ;;
    esac
}

main "$@"
