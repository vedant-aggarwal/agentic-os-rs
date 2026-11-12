#!/usr/bin/env bash
# Turns the .app from `dx bundle --platform ios` into a signed App Store IPA.
#   ios/package.sh <path/to/App.app> <out.ipa>
# Env: BUNDLE_ID, APPLE_TEAM_ID, BUILD_NUMBER, MARKETING_VERSION, MIN_IOS, SIGN_IDENTITY, SIGN_KEYCHAIN,
#      PROFILE_PATH, PROFILE_NAME, ENTITLEMENTS, RUNNER_TEMP
# dx writes a generic Info.plist and leaves the app unsigned; App Store validation also wants the
# compiled app icon, the SDK/Xcode keys Xcode normally adds, and iPhone-only platform values.
set -euo pipefail
APP="$1"; mkdir -p "$(dirname "$2")"; OUT="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"
HERE="$(cd "$(dirname "$0")" && pwd)"
TMP="${RUNNER_TEMP:-/tmp}/pkg"; rm -rf "$TMP"; mkdir -p "$TMP"
PL="$APP/Info.plist"
PB=/usr/libexec/PlistBuddy
plutil -convert xml1 "$PL"

set_key() { $PB -c "Delete :$1" "$PL" >/dev/null 2>&1 || true; $PB -c "Add :$1 $2 $3" "$PL"; }

set_key CFBundleIdentifier string "$BUNDLE_ID"
set_key CFBundleVersion string "$BUILD_NUMBER"
set_key CFBundleShortVersionString string "$MARKETING_VERSION"
set_key CFBundleDisplayName string "Agentic OS"
set_key CFBundlePackageType string APPL
set_key CFBundleInfoDictionaryVersion string 6.0
set_key MinimumOSVersion string "$MIN_IOS"
set_key ITSAppUsesNonExemptEncryption bool false
# AlarmKit (iOS 26) asks for this text when it first schedules an alarm.
set_key NSAlarmKitUsageDescription string "Agentic OS rings the alarms you and Nora set, even in silent mode."

# iPhone only (an iPad build would need every orientation and a launch storyboard).
$PB -c "Delete :UIDeviceFamily" "$PL" >/dev/null 2>&1 || true
$PB -c "Add :UIDeviceFamily array" -c "Add :UIDeviceFamily:0 integer 1" "$PL"
$PB -c "Delete :CFBundleSupportedPlatforms" "$PL" >/dev/null 2>&1 || true
$PB -c "Add :CFBundleSupportedPlatforms array" -c "Add :CFBundleSupportedPlatforms:0 string iPhoneOS" "$PL"
$PB -c "Delete :UISupportedInterfaceOrientations~ipad" "$PL" >/dev/null 2>&1 || true
$PB -c "Delete :UISupportedInterfaceOrientations" "$PL" >/dev/null 2>&1 || true
$PB -c "Add :UISupportedInterfaceOrientations array" -c "Add :UISupportedInterfaceOrientations:0 string UIInterfaceOrientationPortrait" "$PL"
# No storyboard ships in the bundle; an empty UILaunchScreen gives a full-screen launch.
$PB -c "Delete :UILaunchStoryboardName" "$PL" >/dev/null 2>&1 || true
$PB -c "Delete :UILaunchScreen" "$PL" >/dev/null 2>&1 || true
$PB -c "Add :UILaunchScreen dict" "$PL"

# What Xcode stamps into every build; App Store Connect checks the SDK and Xcode versions.
SDK_VER=$(xcrun --sdk iphoneos --show-sdk-version)
SDK_BUILD=$(xcrun --sdk iphoneos --show-sdk-build-version)
XC_VER=$(xcodebuild -version | awk 'NR==1{print $2}')
XC_BUILD=$(xcodebuild -version | awk 'NR==2{print $3}')
DTXCODE=$(echo "$XC_VER" | awk -F. '{printf "%02d%d%d", $1, $2+0, $3+0}')
set_key DTPlatformName string iphoneos
set_key DTPlatformVersion string "$SDK_VER"
set_key DTPlatformBuild string "$SDK_BUILD"
set_key DTSDKName string "iphoneos$SDK_VER"
set_key DTSDKBuild string "$SDK_BUILD"
set_key DTXcode string "$DTXCODE"
set_key DTXcodeBuild string "$XC_BUILD"
set_key DTCompiler string com.apple.compilers.llvm.clang.1_0
set_key BuildMachineOSBuild string "$(sw_vers -buildVersion)"

# App icon: compile the asset catalog into the bundle and merge the icon keys it reports.
xcrun actool "$HERE/Assets.xcassets" --compile "$APP" --platform iphoneos \
  --minimum-deployment-target "$MIN_IOS" --target-device iphone --app-icon AppIcon \
  --output-partial-info-plist "$TMP/icons.plist" --output-format human-readable-text --notices --warnings --errors
$PB -c "Merge $TMP/icons.plist" "$PL"

plutil -lint "$PL"
echo "Info.plist:"; plutil -p "$PL"
EXE=$($PB -c "Print :CFBundleExecutable" "$PL")
echo "Executable: $EXE"; lipo -info "$APP/$EXE"; otool -l "$APP/$EXE" | grep -A4 LC_BUILD_VERSION | head -6 || true

# Find any other Mach-O in the bundle (dx can ship helpers); each is signed before the app.
echo "Mach-O files in the bundle:"
find "$APP" -type f -print0 | while IFS= read -r -d '' f; do
  case "$(file -b "$f")" in *Mach-O*) echo "  ${f#$APP/}";; esac
done

# The push / alarm plugin (src/ios/push) is built by dx into this framework. Without it the app has no
# push token and no local alarms, so stop here instead of shipping a build that looks fine.
if [ ! -d "$APP/Frameworks/DioxusSwiftPlugins.framework" ] && [ "${ALLOW_NO_PUSH:-}" != "1" ]; then
  echo "Frameworks/DioxusSwiftPlugins.framework is missing: dx did not build src/ios/push (set ALLOW_NO_PUSH=1 to ship without push)"
  ls -la "$APP" "$APP/Frameworks" 2>/dev/null || true
  exit 1
fi

# Apple rejects (ITMS-90208) an embedded framework whose Info.plist lacks MinimumOSVersion or asks
# for a different one than the app. dx's framework plist is minimal, so make each one complete.
for fw in "$APP"/Frameworks/*.framework; do
  [ -d "$fw" ] || continue
  name=$(basename "$fw" .framework)
  fpl="$fw/Info.plist"
  [ -f "$fpl" ] || plutil -create xml1 "$fpl" 2>/dev/null || echo '<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict/></plist>' > "$fpl"
  plutil -convert xml1 "$fpl"
  fset() { $PB -c "Delete :$1" "$fpl" >/dev/null 2>&1 || true; $PB -c "Add :$1 $2 $3" "$fpl"; }
  $PB -c "Print :CFBundleExecutable" "$fpl" >/dev/null 2>&1 || fset CFBundleExecutable string "$name"
  $PB -c "Print :CFBundleIdentifier" "$fpl" >/dev/null 2>&1 || fset CFBundleIdentifier string "$BUNDLE_ID.$name"
  fset CFBundlePackageType string FMWK
  fset CFBundleShortVersionString string "$MARKETING_VERSION"
  fset CFBundleVersion string "$BUILD_NUMBER"
  fset MinimumOSVersion string "$MIN_IOS"
  $PB -c "Delete :CFBundleSupportedPlatforms" "$fpl" >/dev/null 2>&1 || true
  $PB -c "Add :CFBundleSupportedPlatforms array" -c "Add :CFBundleSupportedPlatforms:0 string iPhoneOS" "$fpl"
  echo "framework $name: MinimumOSVersion=$MIN_IOS"
  xcrun vtool -show-build "$fw/$name" 2>/dev/null | grep -E 'minos|platform' || true
done

# 1) Sign with the run's Apple Distribution identity, inside-out, profile embedded.
cp "$PROFILE_PATH" "$APP/embedded.mobileprovision"
rm -rf "$APP/_CodeSignature"
find "$APP" -mindepth 1 \( -name "*.framework" -o -name "*.dylib" -o -name "*.appex" \) -prune -print0 |
  while IFS= read -r -d '' f; do codesign --force --sign "$SIGN_IDENTITY" --keychain "$SIGN_KEYCHAIN" "$f"; done
codesign --force --sign "$SIGN_IDENTITY" --keychain "$SIGN_KEYCHAIN" --entitlements "$ENTITLEMENTS" --generate-entitlement-der "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"

# 2) Wrap it in an .xcarchive and let Xcode export it for App Store Connect, the way an Xcode archive ships:
#    Xcode re-signs everything with the distribution certificate and the App Store profile.
ARCHIVE="$TMP/AgenticOS.xcarchive"
mkdir -p "$ARCHIVE/Products/Applications"
cp -R "$APP" "$ARCHIVE/Products/Applications/"
APPNAME=$(basename "$APP")
cat > "$ARCHIVE/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>ApplicationProperties</key><dict>
    <key>ApplicationPath</key><string>Applications/$APPNAME</string>
    <key>Architectures</key><array><string>arm64</string></array>
    <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
    <key>CFBundleShortVersionString</key><string>$MARKETING_VERSION</string>
    <key>CFBundleVersion</key><string>$BUILD_NUMBER</string>
    <key>SigningIdentity</key><string>Apple Distribution</string>
    <key>Team</key><string>$APPLE_TEAM_ID</string>
  </dict>
  <key>ArchiveVersion</key><integer>2</integer>
  <key>CreationDate</key><date>$(date -u +%Y-%m-%dT%H:%M:%SZ)</date>
  <key>Name</key><string>AgenticOS</string>
  <key>SchemeName</key><string>AgenticOS</string>
</dict></plist>
PLIST
cat > "$TMP/export.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>method</key><string>app-store-connect</string>
  <key>destination</key><string>export</string>
  <key>teamID</key><string>$APPLE_TEAM_ID</string>
  <key>signingStyle</key><string>manual</string>
  <key>signingCertificate</key><string>Apple Distribution</string>
  <key>provisioningProfiles</key><dict><key>$BUNDLE_ID</key><string>$PROFILE_NAME</string></dict>
  <key>uploadSymbols</key><false/>
  <key>manageAppVersionAndBuildNumber</key><false/>
</dict></plist>
PLIST
xcodebuild -exportArchive -archivePath "$ARCHIVE" -exportOptionsPlist "$TMP/export.plist" -exportPath "$TMP/export"
IPA=$(find "$TMP/export" -maxdepth 1 -name "*.ipa" | head -1)
[ -n "$IPA" ] || { echo "xcodebuild exported no IPA"; ls -la "$TMP/export"; exit 1; }

# 3) Check the exported IPA before it goes anywhere: distribution authority, App Store profile, entitlements.
CHECK="$TMP/check"; rm -rf "$CHECK"; mkdir -p "$CHECK"
unzip -q "$IPA" -d "$CHECK"
CAPP=$(find "$CHECK/Payload" -maxdepth 1 -name "*.app" | head -1)
codesign --verify --deep --strict --verbose=2 "$CAPP"
SIG=$(codesign -dvvv "$CAPP" 2>&1 || true)
echo "$SIG" | grep -E "^(Authority|Format|CodeDirectory|Signature size)" || true
case "$SIG" in *"Authority=Apple Distribution"*) ;; *) echo "Not signed by Apple Distribution"; exit 1;; esac
case "$SIG" in *"Authority=Apple Worldwide Developer Relations"*) ;; *) echo "Signature has no WWDR intermediate"; exit 1;; esac
security cms -D -i "$CAPP/embedded.mobileprovision" > "$TMP/profile.plist"
if $PB -c "Print :ProvisionedDevices" "$TMP/profile.plist" >/dev/null 2>&1; then echo "Profile lists devices: not an App Store profile"; exit 1; fi
echo "Profile: $($PB -c 'Print :Name' "$TMP/profile.plist"), get-task-allow $($PB -c 'Print :Entitlements:get-task-allow' "$TMP/profile.plist")"
codesign -d --entitlements :- "$CAPP" 2>/dev/null > "$TMP/ent.plist"
echo "Signed entitlements:"; plutil -p "$TMP/ent.plist" | sed -E 's/[A-Z0-9]{10}\./TEAM./g'
[ "$($PB -c 'Print :get-task-allow' "$TMP/ent.plist" 2>/dev/null)" = "false" ] || { echo "get-task-allow is not false"; exit 1; }
# Push: when the profile allows it, the exported app must still carry aps-environment = production.
if $PB -c "Print :Entitlements:aps-environment" "$TMP/profile.plist" >/dev/null 2>&1; then
  [ "$($PB -c 'Print :aps-environment' "$TMP/ent.plist" 2>/dev/null)" = "production" ] || { echo "aps-environment missing after export"; exit 1; }
  echo "Push entitlement: aps-environment production"
else
  echo "Warning: the profile has no aps-environment (Push Notifications off on the App ID); this build gets no pushes"
fi

cp "$IPA" "$OUT"
ls -la "$OUT"
