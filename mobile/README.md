# Phone shell (Capacitor)

The iPhone and Android apps. Capacitor 8 wraps the React web build (`../web/dist`) and adds the native pieces the
web view can't do. App id `com.example.agenticos` in this public copy; CI sets the real one from the `BUNDLE_ID`
secret.

```
npm ci
npm run ios        # web/dist (or the placeholder) -> ios/App/App/public, Package.swift
npm run android    # same for android/
npm run apk        # debug APK: android/app/build/outputs/apk/debug/app-debug.apk
```

Without a web build, `scripts/ensure-web.mjs` copies `placeholder/index.html` into `web/dist`, so the shell
always builds.

## iOS targets

| Target | Bundle id | What it is |
|---|---|---|
| App | id | Web view + native plugins in `ios/App/App/` |
| AgenticWidgets | `.widgets` | Widgets, Live Activities, Control Center controls |
| AgenticShare | `.share` | Share sheet |
| AgenticKeyboard | `.keyboard` | Keyboard with dictation |
| AgenticMonitor, AgenticShield, AgenticShieldAction | `.monitor`, `.shield`, `.shieldaction` | Screen Time lock (only with `family_controls`) |

The committed project has only the App target. `../ios/add_targets.rb` adds the extensions and every Swift file
on the CI Mac. **A new Swift file builds only once it is listed there.** The targets share data through a
keychain group (`<team>.<id>.shared`), so no App Group is needed.

## What the web app has to call

The native side never knows the server address by itself. After pairing (`POST /api/pair/claim`) the web app
calls:

```js
Capacitor.Plugins.Native.share({ server: 'https://host', token: deviceToken, snapshot: JSON.stringify(snap) })
```

- `server` and `token` go to the shared keychain. The keyboard, share sheet, widgets and Siri use them for
  `/api/...` calls, and the APNs token is then sent to `POST /api/push/register`.
- `snapshot` feeds the widgets and the Day Live Activity (shape: `Snapshot` in `ios/App/Shared/Shared.swift`).
  Send it again when the day changes. Habit ids in `streaks` must be the server's habit ids (widget check
  buttons call `habit_log` with them).
- `Native.signOut()` on unpair. `Native.session()` returns `{ server, paired, apnsToken? }`.
- `Native.registerPush()` asks for notification permission.
- Taps on pushes and quick actions arrive as `aos://...` (server pushes) or `agenticos://...` links through the
  App plugin's `appUrlOpen`. Push button presses also arrive as the `notify` event on `Native`.
- Alarms the phone rings itself: `AosAlarm` plugin (`authorize`, `schedule`, `cancel`, `list`; AlarmKit on
  iOS 26, notifications before). The web app schedules from `alarm_list` and reports the ids it holds to
  `POST /api/push/local`, so the server doesn't push those alarms too.

Other plugins (JS name, file): `AosHaptics` Haptics.swift, `AosAuth` Auth.swift (Face ID), `AosScan` Scan.swift,
`AosLive` Live.swift, `AosAlarm` Alarms.swift, `AosNotify` Notify.swift, `AosVoice` Voice.swift (on-device speech),
`AosHealth` Health.swift, `AosSystem` System.swift, `AosPersonal` Personal.swift, `AosLLM` LLM.swift,
`AosScreenTime` ScreenTime.swift, `Speech`. Android: `AosAudio` (call audio route).
