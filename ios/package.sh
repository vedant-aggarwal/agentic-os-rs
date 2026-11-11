#!/usr/bin/env bash
# Turns the .app from `dx bundle --platform ios` into a signed App Store IPA.
#   ios/package.sh <path/to/App.app> <out.ipa>
# Env: BUNDLE_ID, BUILD_NUMBER, MARKETING_VERSION, MIN_IOS, SIGN_IDENTITY, PROFILE_PATH, ENTITLEMENTS, RUNNER_TEMP
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

# Sign: nested code first, then the app with the profile embedded.
cp "$PROFILE_PATH" "$APP/embedded.mobileprovision"
rm -rf "$APP/_CodeSignature"
find "$APP" -mindepth 1 \( -name "*.framework" -o -name "*.dylib" -o -name "*.appex" \) -prune -print0 |
  while IFS= read -r -d '' f; do codesign --force --sign "$SIGN_IDENTITY" --timestamp=none "$f"; done
codesign --force --sign "$SIGN_IDENTITY" --entitlements "$ENTITLEMENTS" --generate-entitlement-der --timestamp=none "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"
codesign -d --entitlements - "$APP" 2>/dev/null | head -40 || true

# IPA = Payload/<App>.app, zipped.
STAGE="$TMP/ipa"; mkdir -p "$STAGE/Payload"
cp -R "$APP" "$STAGE/Payload/"
rm -f "$OUT"
(cd "$STAGE" && zip -qry -X "$OUT" Payload)
ls -la "$OUT"
