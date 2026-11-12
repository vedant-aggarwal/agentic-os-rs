#!/usr/bin/env bash
# Checks an exported App Store IPA before upload (the Capacitor phone shell: app + extensions).
#   ios/check-ipa.sh <path/to/App.ipa>
# Every bundle (the app and each .appex) must be signed by Apple Distribution with the WWDR chain, carry an App Store
# profile (no ProvisionedDevices) and get-task-allow false. The app must keep aps-environment=production when its
# profile grants push. Any miss fails the run.
set -euo pipefail
IPA="$1"
PB=/usr/libexec/PlistBuddy
TMP="${RUNNER_TEMP:-/tmp}/ipa-check"; rm -rf "$TMP"; mkdir -p "$TMP"
unzip -q "$IPA" -d "$TMP/x"
APP=$(find "$TMP/x/Payload" -maxdepth 1 -name "*.app" | head -1)
[ -n "$APP" ] || { echo "No .app in the IPA"; exit 1; }
codesign --verify --deep --strict --verbose=2 "$APP"

check() {
  local b="$1" name; name=$(basename "$b")
  local sig; sig=$(codesign -dvvv "$b" 2>&1 || true)
  case "$sig" in *"Authority=Apple Distribution"*) ;; *) echo "$name: not signed by Apple Distribution"; exit 1;; esac
  case "$sig" in *"Authority=Apple Worldwide Developer Relations"*) ;; *) echo "$name: no WWDR intermediate"; exit 1;; esac
  [ -f "$b/embedded.mobileprovision" ] || { echo "$name: no embedded profile"; exit 1; }
  security cms -D -i "$b/embedded.mobileprovision" > "$TMP/p.plist"
  if $PB -c "Print :ProvisionedDevices" "$TMP/p.plist" >/dev/null 2>&1; then echo "$name: profile lists devices"; exit 1; fi
  codesign -d --entitlements :- "$b" 2>/dev/null > "$TMP/e.plist"
  [ "$($PB -c 'Print :get-task-allow' "$TMP/e.plist" 2>/dev/null)" = "false" ] || { echo "$name: get-task-allow is not false"; exit 1; }
  echo "$name: $($PB -c 'Print :CFBundleIdentifier' "$b/Info.plist" | sed -E 's/^[^.]+\.[^.]+\./BUNDLE./') ok, profile $($PB -c 'Print :Name' "$TMP/p.plist")"
  echo "  entitlements: $(plutil -p "$TMP/e.plist" | grep -oE '"[a-z][a-zA-Z0-9.-]+"( =>)' | tr -d '"=> ' | tr '\n' ' ')"
}

check "$APP"
security cms -D -i "$APP/embedded.mobileprovision" > "$TMP/app.plist"
codesign -d --entitlements :- "$APP" 2>/dev/null > "$TMP/app-e.plist"
if $PB -c "Print :Entitlements:aps-environment" "$TMP/app.plist" >/dev/null 2>&1; then
  [ "$($PB -c 'Print :aps-environment' "$TMP/app-e.plist" 2>/dev/null)" = "production" ] || { echo "aps-environment missing after export"; exit 1; }
  echo "Push entitlement: aps-environment production"
else
  echo "Warning: the app profile has no aps-environment (Push Notifications off on the App ID); this build gets no pushes"
fi

count=0
for ex in "$APP"/PlugIns/*.appex; do
  [ -d "$ex" ] || continue
  check "$ex"
  count=$((count + 1))
done
echo "Extensions checked: $count"
[ "$count" -ge "${MIN_EXTENSIONS:-3}" ] || { echo "Expected at least ${MIN_EXTENSIONS:-3} extensions (widgets, share, keyboard)"; exit 1; }
