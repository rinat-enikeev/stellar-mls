#!/bin/bash
# stellar-builder-bootstrap.sh
#
# COPY-PASTE PLAYBOOK — not intended to be run top-to-bottom as a script.
# Each block is a step you perform once on the Mac mini to provision the
# dedicated `stellar-builder` macOS user that owns all iOS + Android JNI
# builds delegated from the dev-env containers.
#
# Run each block as the indicated user. Stop if anything fails.
set -e

cat <<'EOF'
============================================================
stellar-builder bootstrap — run each step in order
============================================================
EOF

#--- 1. Create the dedicated macOS user (run as admin) --------------------
# sudo dscl . -create /Users/stellar-builder
# sudo dscl . -create /Users/stellar-builder UserShell /bin/bash
# sudo dscl . -create /Users/stellar-builder RealName "Stellar Builder"
# sudo dscl . -create /Users/stellar-builder UniqueID 600
# sudo dscl . -create /Users/stellar-builder PrimaryGroupID 20
# sudo dscl . -create /Users/stellar-builder NFSHomeDirectory /Users/stellar-builder
# sudo mkdir -p /Users/stellar-builder
# sudo chown stellar-builder:staff /Users/stellar-builder
# sudo dscl . -passwd /Users/stellar-builder   # set a password you will keep
#
# (Or: System Settings → Users & Groups → Add User → Standard.)

#--- 2. First login as stellar-builder, accept Xcode license --------------
# su - stellar-builder
# xcode-select -p                     # confirms Xcode path
# sudo xcodebuild -license accept     # requires admin

#--- 3. Install Homebrew + tooling (as stellar-builder) -------------------
#
# 3a. If /opt/homebrew does NOT exist yet, install it fresh:
#   /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
#
# 3b. If /opt/homebrew ALREADY exists (admin user installed it earlier),
#     the installer will fail with "change the ownership of these
#     directories". Do NOT re-run the installer. Pick one of:
#
#     Option A — hand brew to stellar-builder (simplest; admin loses write):
#       # run from your admin account
#       sudo chown -R stellar-builder:staff /opt/homebrew
#
#     Option B — share brew between admin + stellar-builder via a group:
#       # run from your admin account
#       sudo dseditgroup -o create brew
#       sudo dseditgroup -o edit -a <admin-username> -t user brew
#       sudo dseditgroup -o edit -a stellar-builder -t user brew
#       sudo chgrp -R brew /opt/homebrew
#       sudo chmod -R g+rwX /opt/homebrew
#       sudo find /opt/homebrew -type d -exec chmod g+s {} \;
#
# 3c. As stellar-builder, put brew on PATH and install tooling:
#   echo 'eval "$(/opt/homebrew/bin/brew shellenv)"' >> ~/.zprofile
#   eval "$(/opt/homebrew/bin/brew shellenv)"
#   brew --version
#   brew install xcodegen fastlane cocoapods gh sshguard rustup-init
#   rustup-init -y --default-toolchain 1.94.1 --profile minimal
#   rustup target add aarch64-linux-android x86_64-linux-android
#   rustup target add aarch64-apple-ios x86_64-apple-ios aarch64-apple-ios-sim aarch64-apple-darwin x86_64-apple-darwin

#--- 4. Install Android SDK + NDK -----------------------------------------
# NOTE: sdkmanager's "ndk;27.2.12479018" bundle ships a darwin-x86_64
# prebuilt only — there is no darwin-arm64 subdirectory at this version.
# On Apple Silicon the x86_64 clang runs under Rosetta 2, and
# scripts/build-android.sh already falls back to darwin-x86_64/ when
# the native arch dir is missing. This is fine: Rust codegen is still
# native arm64, only the linker step passes through Rosetta.
#
# sudo softwareupdate --install-rosetta --agree-to-license
#
# export ANDROID_HOME="$HOME/Library/Android/sdk"
# mkdir -p "$ANDROID_HOME/cmdline-tools"
# curl -sSLo /tmp/cmdline.zip \
#     "https://dl.google.com/android/repository/commandlinetools-mac-11076708_latest.zip"
# unzip -q /tmp/cmdline.zip -d "$ANDROID_HOME/cmdline-tools"
# mv "$ANDROID_HOME/cmdline-tools/cmdline-tools" "$ANDROID_HOME/cmdline-tools/latest"
# export PATH="$ANDROID_HOME/cmdline-tools/latest/bin:$PATH"
# yes | sdkmanager --licenses >/dev/null
# sdkmanager --install "platform-tools" "platforms;android-34" "build-tools;34.0.0" "ndk;27.2.12479018"
# export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/27.2.12479018"
# ls "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/darwin-x86_64/bin/clang" \
#   && echo "NDK (darwin-x86_64 under Rosetta): OK"
#
# Persist these exports in ~/.zprofile:
# echo 'export ANDROID_HOME="$HOME/Library/Android/sdk"' >> ~/.zprofile
# echo 'export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/27.2.12479018"' >> ~/.zprofile
# echo 'export PATH="$ANDROID_HOME/cmdline-tools/latest/bin:$PATH"' >> ~/.zprofile

#--- 5. Import fastlane Match credentials --------------------------------
# The repo is private. Authenticate gh first — it registers a git
# credential helper so plain `git clone` works afterwards. No SSH key
# needed.
#
# gh auth login
# # GitHub.com → HTTPS → Login with a web browser. Paste the 8-char code.
#
# gh repo clone rinat-enikeev/stellar-mls ~/src/stellar-mls
# cd ~/src/stellar-mls/clients
#
# # macOS system Ruby is 2.6 (EOL, read-only, non-admin can't gem install).
# # Install a modern Ruby via brew instead. stellar-builder already owns
# # /opt/homebrew from step 3.
# brew install ruby
# echo 'export PATH="/opt/homebrew/opt/ruby/bin:$PATH"' >> ~/.zprofile
# export PATH="/opt/homebrew/opt/ruby/bin:$PATH"
#
# # Gemfile.lock pins BUNDLED WITH 2.5.3 — install that exact bundler.
# gem install bundler:2.5.3
# bundle install --path vendor/bundle
# bundle exec fastlane match development --readonly false    # once, to cache
# # Confirm the signing certs and provisioning profiles land in the login
# # keychain for stellar-builder. Containers never see these creds.

#--- 6. Create the bare repo containers push to ---------------------------
# mkdir -p ~/work
# git init --bare ~/work/stellar-mls.git

#--- 7. Install the forced-command dispatcher -----------------------------
# mkdir -p ~/bin ~/logs
# # From the stellar-mls checkout:
# cp build/dev-env/host/host-build-dispatch.sh ~/bin/host-build-dispatch
# chmod +x ~/bin/host-build-dispatch

#--- 8. Enable + harden Remote Login (run as admin) ----------------------
# sudo systemsetup -setremotelogin on
# # Restrict SSH access to stellar-builder only:
# sudo dseditgroup -o create -q com.apple.access_ssh 2>/dev/null || true
# sudo dseditgroup -o edit -a stellar-builder -t user com.apple.access_ssh
#
# # Drop a hardening override (move off port 22, keys only):
# sudo tee /etc/ssh/sshd_config.d/100-stellar.conf >/dev/null <<'CONF'
# Port 2222
# PermitRootLogin no
# PasswordAuthentication no
# KbdInteractiveAuthentication no
# ChallengeResponseAuthentication no
# UsePAM yes
# AllowUsers stellar-builder
# AuthenticationMethods publickey
# MaxAuthTries 3
# LoginGraceTime 20
# ClientAliveInterval 60
# ClientAliveCountMax 3
# CONF
# sudo launchctl kickstart -k system/com.openssh.sshd

#--- 9. Brute-force protection (run as admin) -----------------------------
# brew services start sshguard
# # Optional: verify with `sudo pfctl -s all | grep sshguard`

#--- 10. Open only port 2222 at the firewall (run as admin) ---------------
# Application Firewall + pf. If using macOS built-in firewall, allow
# "sshd" incoming. For pf, add a rule in /etc/pf.anchors/stellar allowing
# tcp to port 2222 and block everything else inbound on the WAN interface.

#--- 11. Authorise container SSH keys (run as stellar-builder) ------------
# mkdir -p ~/.ssh && chmod 700 ~/.ssh
# touch ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys
#
# # After first-run of the dev-env compose stack, each container prints its
# # new public key in `docker logs`. Fetch and paste each into the file below
# # prefixed exactly like this (all one line):
# #
# #   command="/Users/stellar-builder/bin/host-build-dispatch",restrict ssh-ed25519 AAAA... qa-agent
# #   command="/Users/stellar-builder/bin/host-build-dispatch",restrict ssh-ed25519 AAAA... release-agent

#--- 12. Your personal (iPhone) key for shell access ---------------------
# Also add your iPhone's public key WITHOUT the command= restriction —
# this is for you to ProxyJump through stellar-builder into the containers:
#
#   ssh-ed25519 AAAA... iphone
#
# You'll want a separate macOS user (your normal admin account) for day-to-day
# Mac use; stellar-builder stays dedicated to builds.

#--- 13. Optional: auto-start compose at boot -----------------------------
# cp build/dev-env/host/launchd/com.stellar.devenv.plist \
#     ~/Library/LaunchAgents/com.stellar.devenv.plist
# # Edit the plist to point at your actual clone path, then:
# launchctl load -w ~/Library/LaunchAgents/com.stellar.devenv.plist
# # Requires Docker Desktop to auto-start at login (or use OrbStack).

echo
echo "All 13 steps are commented above — run each block one at a time."
echo "Then from your normal user: cd build/dev-env && ./bin/up.sh"
