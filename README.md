# Agentic OS (Rust)

The Agentic OS app rebuilt in Rust with [Dioxus](https://dioxuslabs.com) 0.7: one codebase for iPhone, desktop and web.
This first version is the Today page (morning, day and night states) running on sample data.

## Run it

```sh
cargo install dioxus-cli --version 0.7.10 --locked
dx serve --platform desktop      # or: --platform web
dx serve --platform ios          # on a Mac with Xcode, in the simulator
```

## iPhone builds

`.github/workflows/ios.yml` builds the app on a GitHub macOS runner with `dx bundle --platform ios`, then
`ios/package.sh` finishes the bundle (Info.plist, app icon, App Store signature) into an IPA and uploads it to
TestFlight. `ios/signing.py` makes a short-lived distribution certificate and profile through the
App Store Connect API for each run and removes them afterwards. The build number is the app's highest
build number in App Store Connect plus one.

To use it with your own Apple account, set these repo secrets: `APPLE_TEAM_ID`, `ASC_KEY_ID`, `ASC_ISSUER_ID`,
`ASC_KEY_P8` (the contents of an App Manager API key) and `BUNDLE_ID` (an app you created in App Store Connect).

## License

MIT
