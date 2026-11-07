#!/bin/sh
# Handoff runner. Run in a dedicated, standard local macOS account.
set -eu
PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
umask 077

fail() { printf '%s\n' "$1" >&2; exit 2; }
account=$(id -un)
case "$account" in
    uipilot-qa) ;;
    uipilot-qa-*)
        suffix=${account#uipilot-qa-}
        case "$suffix" in ''|*[!a-z0-9]*) fail '请在专用的 uipilot-qa 测试账户中运行。' ;; esac ;;
    *) fail '请在专用的 uipilot-qa 测试账户中运行。' ;;
esac
[ "$(uname -s)" = Darwin ] && [ "$(uname -m)" = arm64 ] || fail '需要 Apple Silicon macOS。'
[ "$(id -u)" -ne 0 ] && [ "$HOME" = "/Users/$account" ] || fail '请直接登录本机测试账户。'
case " $(id -Gn) " in *' admin '*) fail '请使用标准测试账户。' ;; esac

base=$(CDPATH= cd -P -- "$(dirname -- "$0")" && pwd -P)
app="$base/UiPilot Spotlight QA.app"
probe="$app/Contents/MacOS/uipilot-find-system-qa"
[ -x "$probe" ] || fail '缺少测试程序，请保留完整解压目录。'
codesign --verify --deep --strict "$app" || fail '测试程序签名校验失败。'

shared_identity=$("$probe" --check-workspace) || fail '测试账户或本地共享目录未通过校验。'
cd -P -- /Users/Shared
[ "$(stat -f '%d:%i' .)" = "$shared_identity" ] || fail '共享目录在准备期间发生变化。'
token=$(uuidgen | tr '[:upper:]' '[:lower:]' | tr -d '-')
case "$token" in ''|*[!0-9a-f]*) fail '无法生成样例标识。' ;; esac
[ "${#token}" -eq 32 ] || fail '样例标识长度错误。'
name="uipilot-system-qa-$token"
fixture="/Users/Shared/$name"
mkdir -m 700 "$name" || fail '无法创建独占测试目录。'
cd -P -- "$name"
identity=$(stat -f '%d:%i' .)
cleanup() {
    # Work relative to the original directory. Never recursively remove data.
    for parent in allowed outside; do
        [ ! -L "$parent" ] && [ -d "$parent" ] || continue
        rmdir -- "$parent" || return 1
    done
    cd -P -- .. && [ "$(stat -f '%d:%i' .)" = "$shared_identity" ] &&
        [ "$(stat -f '%d:%i' "$name")" = "$identity" ] && rmdir -- "$name"
}
trap 'rc=$?; trap - EXIT; cleanup || rc=1; exit "$rc"' EXIT
trap 'exit 130' INT
trap 'exit 143' HUP TERM

mkdir -m 700 allowed outside
export UIPILOT_FIND_ISOLATED_QA=1 UIPILOT_FIND_QA_SMOKE_ACCOUNT="$account"
export UIPILOT_FIND_QA_SMOKE_FIXTURE="$fixture" UIPILOT_FIND_QA_VERIFIED_ROOTS="$fixture/allowed"
printf '%s\n' '正在等待 Spotlight 收录两份同名样例，再验证全机查询、范围撤回和旧结果。'
"$probe" --run-isolated
