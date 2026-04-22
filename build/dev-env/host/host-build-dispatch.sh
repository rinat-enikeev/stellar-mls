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
#   "ios <sha>"      → build xcframework + fastlane build_local, stream IPA on stdout
#   "jnilibs <sha>"  → run scripts/build-android.sh and tar build/android/jniLibs to stdout
#
# Build logs stream back over the SSH channel as stderr so the container
# sees them in real time. Stdout is the build artifact:
#   ios      → the raw .ipa bytes (redirect to a file on the caller side)
#   jnilibs  → a tar stream that remote-jnilibs unpacks into workspace/build/android
set -euo pipefail

# Force a UTF-8 locale. Non-interactive ssh inherits sshd's empty locale
# (C/POSIX), which makes fastlane and many Ruby tools warn and then crash
# the moment they hit a non-ASCII byte in a path or log line.
export LANG=en_US.UTF-8
export LC_ALL=en_US.UTF-8

# Homebrew bindir is NOT on the default non-interactive ssh PATH
# (sshd hands us /usr/bin:/bin:/usr/sbin:/sbin when SSH_ORIGINAL_COMMAND
# runs via authorized_keys `command="..."` — no shell rc files are sourced).
# Prepend whichever brew prefix actually exists so tools like flock,
# rbenv, xcodegen, and gh resolve.
for brew_bin in /opt/homebrew/bin /usr/local/bin; do
    [ -d "$brew_bin" ] && PATH="$brew_bin:$PATH"
done
export PATH

# rbenv bootstrap — same rationale: non-interactive ssh doesn't source
# ~/.zshrc / ~/.bashrc, so `bundle` would otherwise fall through to
# macOS system Ruby 2.6, which is too old for the bundler version our
# Gemfile.lock pins (and Bundler 2.5+ requires Ruby >= 3.0).
if command -v rbenv >/dev/null 2>&1; then
    export PATH="$HOME/.rbenv/shims:$PATH"
    eval "$(rbenv init - --no-rehash bash)"
fi

# Load fastlane/match credentials from an operator-managed env file.
# GitHub Actions (release.yml) injects these as secrets; on mac-host we
# read them from ~/.stellar-builder.env, which must be chmod 600 and
# contain at minimum:
#   MATCH_GIT_URL="git@github.com:.../match-profiles.git"
#   MATCH_PASSWORD="..."
#   MATCH_TEAM_ID="..."
# `set -a` promotes every assignment to an export so fastlane sees them.
if [ -f "$HOME/.stellar-builder.env" ]; then
    set -a
    # shellcheck disable=SC1091
    . "$HOME/.stellar-builder.env"
    set +a
fi

LOG_DIR="${HOME}/logs"
BARE="${HOME}/work/stellar-mls.git"
# Used to top up the bare mirror on demand with whatever SHA the caller asked
# for. Nothing else syncs this mirror (no webhook, no cron, no push from the
# agent containers — their SSH keys are pinned to host-build-dispatch only),
# so without this fetch the mirror is frozen at whenever the operator last
# seeded it and `git checkout "$SHA"` fails with "unable to read tree".
# Override via ~/.stellar-builder.env if the repo moves.
REMOTE_URL="${STELLAR_MLS_REMOTE_URL:-https://github.com/rinat-enikeev/stellar-mls.git}"
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

    # Ensure the bare mirror actually has $SHA. GitHub's uploadpack
    # advertises refs/heads/* and refs/pull/*/head, so fetching a PR head
    # SHA by value works as long as it's reachable from one of those.
    # Incremental after the first build per SHA — only missing objects
    # cross the wire.
    if ! git -C "$BARE" config --get remote.origin.url >/dev/null 2>&1; then
        git -C "$BARE" remote add origin "$REMOTE_URL" >&2
    fi
    git -C "$BARE" fetch --quiet origin "$SHA" >&2

    # Fresh work tree per SHA. Cheap on local SSD, avoids concurrent
    # writers stomping each other. The clone itself is local hardlinks
    # from the bare repo (no network) — the network hop is the fetch
    # above.
    rm -rf "$WORK"
    git clone --quiet "$BARE" "$WORK" >&2
    cd "$WORK"
    git checkout --quiet "$SHA" >&2

    case "$SUB" in
        ios)
            echo "==> building iOS xcframework" >&2
            ./scripts/build-xcframework.sh >&2

            # RelayerDefaults.generated.swift is gitignored. xcodegen snapshots
            # the StellarChat/ folder at project-generation time, so the file
            # must exist on disk before xcodegen runs or it won't be added to
            # the target's compile sources. The .xcodeproj's "Generate Relayer
            # Defaults" pre-build phase is too late. CI does the same in
            # pr.yml / release.yml — keep those three paths in sync.
            echo "==> pre-generating RelayerDefaults.generated.swift" >&2
            ENV_FILE="relayer/.env"
            [ -f "$ENV_FILE" ] || ENV_FILE="relayer/.env.example"
            ROOT_ENV=".env"
            [ -f "$ROOT_ENV" ] || ROOT_ENV=".env.example"
            OUT="clients/ios/StellarChat/StellarChat/RelayerDefaults.generated.swift"
            EP=""; CID=""; BIND=""; AUTH=""; DOMAIN=""
            if [ -f "$ENV_FILE" ]; then
                while IFS= read -r line || [ -n "$line" ]; do
                    case "$line" in \#*|"") continue ;; esac
                    key="${line%%=*}"; val="${line#*=}"
                    val="${val%\"}"; val="${val#\"}"
                    case "$key" in
                        RELAYER_RPC_URL) EP="$val" ;;
                        RELAYER_CONTRACT_ID) CID="$val" ;;
                        RELAYER_BIND) BIND="$val" ;;
                        RELAYER_AUTH_TOKENS) AUTH="${val%%,*}" ;;
                    esac
                done < "$ENV_FILE"
            fi
            if [ -f "$ROOT_ENV" ]; then
                while IFS= read -r line || [ -n "$line" ]; do
                    case "$line" in \#*|"") continue ;; esac
                    key="${line%%=*}"; val="${line#*=}"
                    val="${val%\"}"; val="${val#\"}"
                    case "$key" in
                        DOMAIN) DOMAIN="$val" ;;
                    esac
                done < "$ROOT_ENV"
            fi
            NOSTR=""; BLOSSOM=""; RELAY=""
            if [ -n "$DOMAIN" ]; then
                NOSTR="wss://nostr.$DOMAIN"
                BLOSSOM="https://blossom.$DOMAIN"
                RELAY="https://relay.$DOMAIN"
            elif [ -n "$BIND" ]; then
                RELAY="http://$BIND"
            fi
            cat > "$OUT" << SWIFT
// Auto-generated from .env + relayer/.env — do not edit
enum RelayerDefaults {
    static let contractEndpoint = "$EP"
    static let contractID = "$CID"
    static let relayerURL = "$RELAY"
    static let relayerAuthToken = "$AUTH"
    static let defaultNostrRelay = "$NOSTR"
    static let defaultBlossomServer = "$BLOSSOM"
}
SWIFT

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
            if [ -z "$IPA" ] || [ ! -f "$IPA" ]; then
                echo "ERROR: no IPA produced under build/ios/adhoc/" >&2
                exit 1
            fi
            echo "ARTIFACT: ${IPA}" >&2
            echo "==> streaming IPA → stdout (${IPA})" >&2
            # Stream the IPA on stdout so the n8n /build flow can redirect it
            # into a file on the qa-agent and publish it. Logs keep flowing on
            # stderr back to the SSH channel. Mirrors the jnilibs tar pattern.
            cat "$IPA"
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
