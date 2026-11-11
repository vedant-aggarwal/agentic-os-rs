// Push notifications, notification taps and local alarms for the Dioxus app (iOS).
//
// Rust calls three @objc methods through `#[manganis::ffi]` (src/native/ios.rs). Each returns JSON
// at once and never waits on the main thread; results (the device token, taps, which alarms are
// set) are picked up by `pollJson`.
//
// The APNs device token: iOS hands it to the app delegate's
// `application:didRegisterForRemoteNotificationsWithDeviceToken:`. The app delegate here is tao's,
// which has no such method, so `addDelegateHooks` adds it (and the failure twin) to tao's delegate
// class at runtime, right after launch, before `registerForRemoteNotifications` is called.
//
// Alarms: AlarmKit on iOS 26 (rings through silent mode and Focus), else a time-sensitive local
// notification plus two follow-ups a minute apart with a 28 s chime. Ported from the old app's
// Alarms.swift / Notify.swift.

#if os(iOS)
import Foundation
import ObjectiveC
import UIKit
import UserNotifications
#if canImport(AlarmKit)
import AlarmKit
import SwiftUI
#endif

@objc(AosPushPlugin)
public class AosPushPlugin: NSObject {
    /// `{"ask": bool}`: install the hooks (idempotent); with `ask`, show the permission prompt once.
    @objc public func startJson(_ optionsJson: String) -> String {
        let opts = PushHub.parse(optionsJson) as? [String: Any] ?? [:]
        PushHub.shared.install()
        if (opts["ask"] as? Bool) == true {
            PushHub.shared.ask()
        }
        return PushHub.json(PushHub.shared.snapshot(drain: false))
    }

    /// `{status, token?, token_error?, alarm_kit, taps: [...], local?: [ids], local_error?}`.
    /// Taps and `local` are handed out once.
    @objc public func pollJson() -> String {
        return PushHub.json(PushHub.shared.snapshot(drain: true))
    }

    /// The full list of upcoming alarms: `[{id, at_ms, title, body?, kind: alarm|reminder}]`.
    /// Alarms missing from the list are cancelled. The result arrives in `pollJson().local`.
    @objc public func syncAlarmsJson(_ alarmsJson: String) -> String {
        let list = (PushHub.parse(alarmsJson) as? [[String: Any]]) ?? []
        AlarmBook.shared.sync(list)
        return PushHub.json(["accepted": list.count])
    }
}

final class PushHub: NSObject, UNUserNotificationCenterDelegate {
    static let shared = PushHub()

    private let lock = NSLock()
    private var installed = false
    private var hooked = false
    private var asked = false
    private var status = "unknown"
    private var token: String?
    private var tokenError: String?
    private var taps: [[String: Any]] = []
    private var local: [String]?
    private var localError: String?

    // MARK: JSON

    static func parse(_ s: String) -> Any? {
        guard let d = s.data(using: .utf8) else { return nil }
        return try? JSONSerialization.jsonObject(with: d)
    }

    static func json(_ v: Any) -> String {
        guard JSONSerialization.isValidJSONObject(v), let d = try? JSONSerialization.data(withJSONObject: v) else { return "{}" }
        return String(data: d, encoding: .utf8) ?? "{}"
    }

    /// `UIApplication.shared` crashes before UIApplicationMain; this returns nil instead.
    static func app() -> UIApplication? {
        return UIApplication.perform(NSSelectorFromString("sharedApplication"))?.takeUnretainedValue() as? UIApplication
    }

    private func locked<T>(_ f: () -> T) -> T {
        lock.lock()
        defer { lock.unlock() }
        return f()
    }

    func snapshot(drain: Bool) -> [String: Any] {
        return locked {
            var out: [String: Any] = ["status": status, "alarm_kit": AlarmBook.kitAvailable, "taps": taps]
            if let token { out["token"] = token }
            if let tokenError { out["token_error"] = tokenError }
            if let local { out["local"] = local }
            if let localError { out["local_error"] = localError }
            if drain {
                taps = []
                local = nil
                localError = nil
            }
            return out
        }
    }

    func setLocal(_ ids: [String], error: String?) {
        locked {
            local = ids
            localError = error
        }
    }

    // MARK: Setup

    func install() {
        let first = locked { () -> Bool in
            if installed { return false }
            installed = true
            return true
        }
        guard first else { return }
        let center = UNUserNotificationCenter.current()
        // Set before launch finishes, so a tap that launched the app still reaches us.
        center.delegate = self
        center.setNotificationCategories(PushHub.categories())
        NotificationCenter.default.addObserver(forName: UIApplication.didFinishLaunchingNotification, object: nil, queue: nil) { _ in
            PushHub.shared.launched()
        }
        // Started after launch (not from main): do the launch work now.
        DispatchQueue.main.async {
            if PushHub.app()?.delegate != nil {
                PushHub.shared.launched()
            }
        }
    }

    private func launched() {
        let first = locked { () -> Bool in
            if hooked { return false }
            hooked = true
            return true
        }
        guard first else { return }
        addDelegateHooks()
        UNUserNotificationCenter.current().delegate = self
        DispatchQueue.global(qos: .utility).async { _ = Chime.ensure() }
        refreshStatus(register: true)
    }

    /// Re-read the permission; when allowed, register for remote notifications (every launch, as Apple asks).
    private func refreshStatus(register: Bool) {
        UNUserNotificationCenter.current().getNotificationSettings { s in
            let st = PushHub.statusName(s.authorizationStatus)
            self.locked { self.status = st }
            if register && (st == "granted" || st == "provisional" || st == "ephemeral") {
                DispatchQueue.main.async { PushHub.app()?.registerForRemoteNotifications() }
            }
        }
    }

    /// The permission prompt (iOS shows it once; later calls just report the answer).
    func ask() {
        let go = locked { () -> Bool in
            if asked { return false }
            asked = true
            return true
        }
        guard go else { return }
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound, .badge]) { granted, error in
            if let error { self.locked { self.tokenError = error.localizedDescription } }
            self.refreshStatus(register: granted)
        }
    }

    static func statusName(_ s: UNAuthorizationStatus) -> String {
        switch s {
        case .authorized: return "granted"
        case .provisional: return "provisional"
        case .ephemeral: return "ephemeral"
        case .denied: return "denied"
        case .notDetermined: return "prompt"
        @unknown default: return "unknown"
        }
    }

    /// Adds the two remote-notification callbacks to the running app delegate's class (tao's).
    private func addDelegateHooks() {
        guard let delegate = PushHub.app()?.delegate, let cls: AnyClass = object_getClass(delegate) else {
            locked { tokenError = "no app delegate to receive the push token" }
            return
        }
        let didRegister: @convention(block) (AnyObject, UIApplication, NSData) -> Void = { _, _, data in
            PushHub.shared.gotToken(data as Data)
        }
        let didFail: @convention(block) (AnyObject, UIApplication, NSError) -> Void = { _, _, error in
            PushHub.shared.tokenFailed(error)
        }
        PushHub.hook(cls, "application:didRegisterForRemoteNotificationsWithDeviceToken:", imp_implementationWithBlock(didRegister))
        PushHub.hook(cls, "application:didFailToRegisterForRemoteNotificationsWithError:", imp_implementationWithBlock(didFail))
    }

    private static func hook(_ cls: AnyClass, _ name: String, _ imp: IMP) {
        let sel = NSSelectorFromString(name)
        if class_addMethod(cls, sel, imp, "v@:@@") { return }
        // A future tao that implements it itself: ours takes over (the token is all it would carry).
        if let m = class_getInstanceMethod(cls, sel) {
            method_setImplementation(m, imp)
            NSLog("[aos-push] replaced an existing %@", name)
        }
    }

    func gotToken(_ data: Data) {
        let hex = data.map { String(format: "%02x", $0) }.joined()
        locked {
            token = hex
            tokenError = nil
        }
    }

    func tokenFailed(_ error: Error) {
        locked { tokenError = error.localizedDescription }
    }

    // MARK: Buttons

    static func categories() -> Set<UNNotificationCategory> {
        let stop = UNNotificationAction(identifier: "AOS_STOP", title: "Stop", options: [.destructive])
        let snooze = UNNotificationAction(identifier: "AOS_SNOOZE", title: "Snooze 9 min", options: [])
        let answer = UNNotificationAction(identifier: "AOS_ANSWER", title: "Answer", options: [.foreground])
        let decline = UNNotificationAction(identifier: "AOS_DECLINE", title: "Decline", options: [.destructive])
        let open = UNNotificationAction(identifier: "AOS_OPEN", title: "Open", options: [.foreground])
        return [
            UNNotificationCategory(identifier: "AOS_ALARM", actions: [stop, snooze], intentIdentifiers: [], options: [.customDismissAction]),
            UNNotificationCategory(identifier: "AOS_REMINDER", actions: [open, snooze], intentIdentifiers: [], options: []),
            UNNotificationCategory(identifier: "AOS_CALL", actions: [answer, decline], intentIdentifiers: [], options: [.customDismissAction]),
            UNNotificationCategory(identifier: "AOS_APPROVAL", actions: [open], intentIdentifiers: [], options: []),
            UNNotificationCategory(identifier: "AOS_INBOX", actions: [open], intentIdentifiers: [], options: []),
            UNNotificationCategory(identifier: "AOS_PLAIN", actions: [], intentIdentifiers: [], options: []),
        ]
    }

    // MARK: UNUserNotificationCenterDelegate

    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification,
                                withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void) {
        let aos = notification.request.content.userInfo["aos"] as? [String: Any]
        // While the app is open, Nora's ring shows as the in-app ring card; no banner on top of it.
        if (aos?["kind"] as? String) == "call" {
            completionHandler([])
            return
        }
        completionHandler([.banner, .list, .sound])
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
                                withCompletionHandler completionHandler: @escaping () -> Void) {
        let content = response.notification.request.content
        let aos = content.userInfo["aos"] as? [String: Any] ?? [:]
        let action: String
        switch response.actionIdentifier {
        case "AOS_STOP": action = "stop"
        case "AOS_SNOOZE": action = "snooze"
        case "AOS_ANSWER": action = "answer"
        case "AOS_DECLINE": action = "decline"
        case UNNotificationDismissActionIdentifier: action = "dismiss"
        default: action = "open"
        }
        let kind = (aos["kind"] as? String) ?? ""
        let id = (aos["id"] as? String) ?? response.notification.request.identifier
        if kind == "alarm" || kind == "reminder" {
            AlarmBook.shared.notificationAction(id: id, action: action, content: content)
        }
        var tap: [String: Any] = ["action": action, "kind": kind, "id": id]
        if let url = aos["url"] as? String { tap["url"] = url }
        locked { taps.append(tap) }
        completionHandler()
    }
}

/// Alarms and reminders the phone rings by itself, so they work with no signal.
final class AlarmBook {
    static let shared = AlarmBook()

    struct Meta: Codable, Equatable {
        var id: String
        var at: Double          // unix seconds
        var title: String
        var body: String
        var kind: String        // alarm | reminder
        var mode: String        // alarmkit | notification
    }

    /// iOS keeps 64 pending notifications per app; an alarm uses up to 3.
    static let maxAlarms = 20
    static let snoozeMinutes = 9.0

    private let key = "aos.push.alarms"
    private let queue = DispatchQueue(label: "aos.push.alarms")
    private var running = false
    private var pending: [[String: Any]]?

    static var kitAvailable: Bool {
        #if canImport(AlarmKit)
        if #available(iOS 26.0, *) { return true }
        #endif
        return false
    }

    private var metas: [String: Meta] {
        get { UserDefaults.standard.data(forKey: key).flatMap { try? JSONDecoder().decode([String: Meta].self, from: $0) } ?? [:] }
        set { UserDefaults.standard.set(try? JSONEncoder().encode(newValue), forKey: key) }
    }

    func sync(_ list: [[String: Any]]) {
        let start = queue.sync { () -> Bool in
            if running {
                pending = list
                return false
            }
            running = true
            return true
        }
        guard start else { return }
        Task {
            var next: [[String: Any]]? = list
            while let l = next {
                await self.apply(l)
                next = self.queue.sync { () -> [[String: Any]]? in
                    let p = self.pending
                    self.pending = nil
                    if p == nil { self.running = false }
                    return p
                }
            }
        }
    }

    private func apply(_ list: [[String: Any]]) async {
        let now = Date().timeIntervalSince1970
        var wanted: [String: Meta] = [:]
        for row in list {
            guard let id = row["id"] as? String, !id.isEmpty, let ms = (row["at_ms"] as? NSNumber)?.doubleValue else { continue }
            let at = ms / 1000
            guard at > now + 5 else { continue }
            let kind = (row["kind"] as? String) == "reminder" ? "reminder" : "alarm"
            wanted[id] = Meta(id: id, at: at, title: (row["title"] as? String) ?? (kind == "reminder" ? "Reminder" : "Alarm"),
                              body: (row["body"] as? String) ?? "", kind: kind, mode: "")
        }
        let keep = wanted.values.sorted { $0.at < $1.at }.prefix(AlarmBook.maxAlarms)
        wanted = Dictionary(uniqueKeysWithValues: keep.map { ($0.id, $0) })

        var current = metas
        for (id, old) in current {
            let w = wanted[id]
            if w == nil || w!.at != old.at || w!.title != old.title || w!.kind != old.kind {
                cancel(id: id, notesOnly: old.mode != "alarmkit")
                current.removeValue(forKey: id)
            }
        }
        var lastError: String?
        for (id, meta) in wanted where current[id] == nil {
            var m = meta
            if let mode = await schedule(m, error: &lastError) {
                m.mode = mode
                current[id] = m
            }
        }
        metas = current
        PushHub.shared.setLocal(Array(current.keys).sorted(), error: lastError)
    }

    /// Returns the mode used, or nil (and sets `error`) when nothing could be scheduled.
    private func schedule(_ m: Meta, error: inout String?) async -> String? {
        #if canImport(AlarmKit)
        if m.kind == "alarm", #available(iOS 26.0, *) {
            do {
                try await Kit.schedule(id: AlarmBook.uuid(for: m.id), at: Date(timeIntervalSince1970: m.at), title: m.title)
                return "alarmkit"
            } catch {
                // Not allowed, or over the limit: fall through so it still rings as a notification.
            }
        }
        #endif
        if let e = await scheduleNotes(m) {
            error = e
            return nil
        }
        return "notification"
    }

    private func noteIds(_ id: String) -> [String] {
        ["alarm-\(id)-0", "alarm-\(id)-1", "alarm-\(id)-2", "alarm-\(id)-snooze"]
    }

    private func cancel(id: String, notesOnly: Bool) {
        let center = UNUserNotificationCenter.current()
        center.removePendingNotificationRequests(withIdentifiers: noteIds(id))
        #if canImport(AlarmKit)
        if !notesOnly, #available(iOS 26.0, *) {
            Kit.cancel(AlarmBook.uuid(for: id))
        }
        #endif
    }

    /// Below iOS 26 (or AlarmKit refused): time-sensitive notifications; alarms get two follow-ups.
    private func scheduleNotes(_ m: Meta) async -> String? {
        let center = UNUserNotificationCenter.current()
        let s = await center.notificationSettings()
        if s.authorizationStatus == .denied {
            return "Notifications are off for Agentic OS, so alarms can't ring. Turn them on in Settings."
        }
        if s.authorizationStatus == .notDetermined {
            _ = try? await center.requestAuthorization(options: [.alert, .sound, .badge])
        }
        let alarm = m.kind == "alarm"
        let sound = alarm ? Chime.ensure() : nil
        let offsets: [Double] = alarm ? [0, 60, 120] : [0]
        for (n, offset) in offsets.enumerated() {
            let c = UNMutableNotificationContent()
            c.title = m.title
            c.body = m.body.isEmpty ? (alarm ? (n == 0 ? "Alarm" : "Still ringing") : "Reminder") : m.body
            c.categoryIdentifier = alarm ? "AOS_ALARM" : "AOS_REMINDER"
            c.threadIdentifier = "aos-\(m.kind)"
            c.interruptionLevel = alarm ? .timeSensitive : .active
            c.sound = sound.map { UNNotificationSound(named: UNNotificationSoundName($0)) } ?? .default
            c.userInfo = ["aos": ["id": m.id, "kind": m.kind, "url": "aos://today"]]
            let wait = max(2, m.at + offset - Date().timeIntervalSince1970)
            let trigger = UNTimeIntervalNotificationTrigger(timeInterval: wait, repeats: false)
            do {
                try await center.add(UNNotificationRequest(identifier: "alarm-\(m.id)-\(n)", content: c, trigger: trigger))
            } catch {
                return error.localizedDescription
            }
        }
        return nil
    }

    /// Stop / snooze on a ringing notification alarm.
    func notificationAction(id: String, action: String, content: UNNotificationContent) {
        let center = UNUserNotificationCenter.current()
        let followUps = ["alarm-\(id)-1", "alarm-\(id)-2"]
        center.removePendingNotificationRequests(withIdentifiers: followUps)
        center.removeDeliveredNotifications(withIdentifiers: followUps)
        if action == "snooze", let copy = content.mutableCopy() as? UNMutableNotificationContent {
            center.add(UNNotificationRequest(identifier: "alarm-\(id)-snooze", content: copy,
                                             trigger: UNTimeIntervalNotificationTrigger(timeInterval: AlarmBook.snoozeMinutes * 60, repeats: false)))
        }
    }

    /// A stable UUID for a server id (AlarmKit wants UUIDs).
    static func uuid(for id: String) -> UUID {
        if let u = UUID(uuidString: id) { return u }
        // FNV-1a over the id, twice with different seeds, as 16 bytes; version 5 / variant bits set.
        func fnv(_ seed: UInt64) -> UInt64 {
            var h: UInt64 = 0xcbf29ce484222325 ^ seed
            for b in id.utf8 {
                h ^= UInt64(b)
                h = h &* 0x100000001b3
            }
            return h
        }
        var bytes = [UInt8](repeating: 0, count: 16)
        let a = fnv(0), b = fnv(0x9e3779b97f4a7c15)
        for i in 0..<8 {
            bytes[i] = UInt8(truncatingIfNeeded: a >> (8 * UInt64(i)))
            bytes[8 + i] = UInt8(truncatingIfNeeded: b >> (8 * UInt64(i)))
        }
        bytes[6] = (bytes[6] & 0x0F) | 0x50
        bytes[8] = (bytes[8] & 0x3F) | 0x80
        return UUID(uuid: (bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                           bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]))
    }
}

#if canImport(AlarmKit)
@available(iOS 26.0, *)
struct AosAlarmMeta: AlarmMetadata {
    var title: String
}

/// AlarmKit, same calls as the old app's Alarms.swift (which shipped on this CI's Xcode 26).
@available(iOS 26.0, *)
enum Kit {
    struct Off: Error {}

    static func schedule(id: UUID, at: Date, title: String) async throws {
        let mgr = AlarmManager.shared
        if mgr.authorizationState == .notDetermined { _ = try await mgr.requestAuthorization() }
        guard mgr.authorizationState == .authorized else { throw Off() }
        let name: LocalizedStringResource = "\(title)"
        let snoozeButton = AlarmButton(text: "Snooze", textColor: .white, systemImageName: "zzz")
        let alert: AlarmPresentation.Alert
        if #available(iOS 26.1, *) {
            alert = AlarmPresentation.Alert(title: name, secondaryButton: snoozeButton, secondaryButtonBehavior: .countdown)
        } else {
            alert = AlarmPresentation.Alert(title: name, stopButton: AlarmButton(text: "Stop", textColor: .white, systemImageName: "stop.circle"),
                                            secondaryButton: snoozeButton, secondaryButtonBehavior: .countdown)
        }
        let presentation = AlarmPresentation(alert: alert, countdown: AlarmPresentation.Countdown(title: "Snoozing"), paused: nil)
        let attributes = AlarmAttributes<AosAlarmMeta>(presentation: presentation, metadata: AosAlarmMeta(title: title),
                                                       tintColor: Color(red: 0.46, green: 0.42, blue: 0.9))
        let config = AlarmManager.AlarmConfiguration<AosAlarmMeta>(
            countdownDuration: Alarm.CountdownDuration(preAlert: nil, postAlert: AlarmBook.snoozeMinutes * 60),
            schedule: .fixed(at), attributes: attributes,
            stopIntent: nil, secondaryIntent: nil, sound: .default)
        _ = try await mgr.schedule(id: id, configuration: config)
    }

    static func cancel(_ id: UUID) {
        try? AlarmManager.shared.cancel(id: id)
    }
}
#endif

/// A 28-second chime written once to Library/Sounds (where notification sounds are looked up).
enum Chime {
    static let name = "aos-alarm.wav"

    static func ensure() -> String? {
        let fm = FileManager.default
        guard let lib = fm.urls(for: .libraryDirectory, in: .userDomainMask).first else { return nil }
        let dir = lib.appendingPathComponent("Sounds", isDirectory: true)
        let file = dir.appendingPathComponent(name)
        if fm.fileExists(atPath: file.path) { return name }
        do {
            try fm.createDirectory(at: dir, withIntermediateDirectories: true)
            try wav().write(to: file, options: .atomic)
            return name
        } catch {
            return nil
        }
    }

    /// Three rising notes, a pause, again: 16-bit mono at 22.05 kHz.
    private static func wav() -> Data {
        let rate = 22050.0, seconds = 28.0
        let total = Int(rate * seconds)
        var samples = [Int16](repeating: 0, count: total)
        let notes: [Double] = [659.25, 830.61, 987.77]
        var t = 0.0
        while t < seconds - 1 {
            for (i, f) in notes.enumerated() {
                let start = Int((t + Double(i) * 0.22) * rate)
                let len = Int(0.5 * rate)
                for k in 0..<len where start + k < total {
                    let x = Double(k) / rate
                    let env = min(1, x / 0.01) * exp(-x * 5.5)
                    let v = (sin(2 * Double.pi * f * x) + 0.3 * sin(4 * Double.pi * f * x)) * env * 0.45
                    let mixed = Double(samples[start + k]) / 32767 + v
                    samples[start + k] = Int16(max(-1, min(1, mixed)) * 32767)
                }
            }
            t += 1.6
        }
        var d = Data()
        func u32(_ v: UInt32) { var x = v.littleEndian; d.append(Data(bytes: &x, count: 4)) }
        func u16(_ v: UInt16) { var x = v.littleEndian; d.append(Data(bytes: &x, count: 2)) }
        let bytes = UInt32(total * 2)
        d.append(contentsOf: Array("RIFF".utf8)); u32(36 + bytes)
        d.append(contentsOf: Array("WAVE".utf8))
        d.append(contentsOf: Array("fmt ".utf8)); u32(16); u16(1); u16(1); u32(UInt32(rate)); u32(UInt32(rate) * 2); u16(2); u16(16)
        d.append(contentsOf: Array("data".utf8)); u32(bytes)
        samples.withUnsafeBufferPointer { p in d.append(Data(buffer: p)) }
        return d
    }
}
#endif
