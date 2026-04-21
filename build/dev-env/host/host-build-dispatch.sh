#!/bin/bash
# host-build-dispatch — forced-command dispatcher for the Mac mini.
#
# Deployed to /Users/stellar-builder/bin/host-build-dispatch and pinned as
# the `command=` for every container SSH key in stellar-builder's
# authorized_keys:
#
#   command="/Users/stellar-builder/bin/host-build-dispatch",restrict ssh-ed25519 AAAA... qa-agent
#
# That `command=` + `restrict` combo means a compromised container can only
# run this script; no shell, no scp, no port forward. The script parses
# $SSH_ORIGINAL_COMMAND into subcommand + sha + (optional) args:
#
#   "ios <sha>"      → build iOS XCFramework + fastlane build_local lane
#   "jnilibs <sha>"  → run scripts/build-android.sh and tar build/android/jniLibs to stdout
#
# Build logs stream back over the SSH channel as stderr so the container
# sees them in real time. For jnilibs, stdout is the tar stream that
# remote-jnilibs unpacks into the container's workspace/build/android.
set -euo pipefail

# rbenv bootstrap. ssh with SSH_ORIGINAL_COMMAND is a non-interactive,
# non-login shell — ~/.zshrc / ~/.bashrc aren't sourced — so rbenv is
# absent by default and `bundle` would fall through to macOS system Ruby
# 2.6, which is too old for the bundler version our Gemfile.lock pins.
if [ -d "$HOME/.rbenv" ]; then
    export PATH="$HOME/.rbenv/bin:$HOME/.rbenv/shims:$PATH"
    eval "$(rbenv init - --no-rehash bash)"
fi

LOG_DIR="${HOME}/logs"
BARE="${HOME}/work/stellar-mls.git"
WORK_ROOT="/tmp/stellar-builds"
LOCK="${WORK_ROOT}/.lock"

mkdir -p "$LOG_DIR" "$WORK_ROOT"

TS=$(date -u +%Y%m%dT%H%M%SZ)
LOG="${LOG_DIR}/build-${TS}.log"

# All stderr → both the SSH channel and the on-host log file, via tee.
exec 2> >(tee -a "$LOG" >&2)

# Parse SSH_ORIGINAL_COMMAND. Word-split safely: the only inputs we accept
# are the subcommand name and a git SHA (40 hex chars, or a short prefix).
# shellcheck disable=SC2086
set -- ${SSH_ORIGINAL_COMMAND:-}
SUB="${1:-}"
SHA="${2:-}"

case "$SUB" in
    ios|jnilibs) ;;
    "") echo "ERROR: missing subcommand (expected: ios|jnilibs <sha>)" >&2; exit 2 ;;
    *)  echo "ERROR: unknown subcommand '$SUB'" >&2; exit 2 ;;
esac

if ! [[ "$SHA" =~ ^[0-9a-f]{7,40}$ ]]; then
    echo "ERROR: '$SHA' is not a valid git sha" >&2
    exit 2
fi

WORK="${WORK_ROOT}/${SHA}"

echo ">>> $(date -u +%FT%TZ) sub=$SUB sha=$SHA log=$LOG" >&2

(
    flock -x 200

    # Fresh work tree per SHA. Cheap on local SSD, avoids concurrent
    # writers stomping each other. The git clone from the bare repo is
    # local-disk fast (no network).
    rm -rf "$WORK"
    git clone --quiet "$BARE" "$WORK" >&2
    cd "$WORK"
    git checkout --quiet "$SHA" >&2

    case "$SUB" in
        ios)
            echo "==> building iOS xcframework" >&2
            ./scripts/build-xcframework.sh >&2

            echo "==> xcodegen + fastlane build_local" >&2
            (cd clients/ios/StellarChat && xcodegen generate) >&2
            cd clients
            # Install the exact bundler version pinned in Gemfile.lock's
            # BUNDLED WITH footer if it's missing. Skipping this step leaves
            # `bundle exec` crashing with Gem::GemNotFoundException whenever
            # the lockfile bumps bundler faster than mac-host does.
            BUNDLER_VERSION=$(awk '/^BUNDLED WITH/{getline; gsub(/^[[:space:]]+/,""); print; exit}' Gemfile.lock)
            if [ -n "$BUNDLER_VERSION" ] && ! gem list -i bundler -v "$BUNDLER_VERSION" >/dev/null 2>&1; then
                echo "==> installing bundler ${BUNDLER_VERSION} (pinned in Gemfile.lock)" >&2
                gem install bundler -v "$BUNDLER_VERSION" --no-document >&2
            fi
            bundle install --path vendor/bundle >&2
            bundle exec fastlane ios build_local >&2

            IPA=$(ls build/ios/adhoc/*.ipa 2>/dev/null | head -1 || true)
            echo "ARTIFACT: ${IPA:-none}" >&2
            ;;

        jnilibs)
            echo "==> building Android JNI libs" >&2
            # build-android.sh derives NDK toolchain from uname (darwin-arm64
            # on the Mac mini) and writes to build/android/jniLibs/<abi>/.
            ./scripts/build-android.sh --out-dir build/android >&2

            if [ ! -d build/android/jniLibs ]; then
                echo "ERROR: build/android/jniLibs not produced" >&2
                exit 1
            fi

            echo "==> streaming jniLibs tar → stdout" >&2
            # Tar on stdout so the container wrapper can pipe into tar -x.
            tar -c -C build/android jniLibs
            ;;
    esac

) 200>"$LOCK"

echo "<<< $(date -u +%FT%TZ) done sub=$SUB sha=$SHA" >&2
