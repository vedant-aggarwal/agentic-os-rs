import AVFoundation
import CallKit
import Intents
import PushKit
import UIKit

/// Nora on the iPhone's own call screen. A VoIP push from the server rings CallKit, even when the phone is locked.
/// With the new server the call itself runs in the web view over WebRTC (POST /api/nora/call/start, see call.js),
/// so CallKit only rings: answering hands the ring to the web layer (aos://call/ring/<id>) and declining posts
/// /api/nora/call/decline. Server needs (not built yet): VoIP token registration and VoIP pushes for rings; until
/// then rings arrive as time-sensitive alert pushes (category AOS_CALL) and this class stays idle.
final class CallManager: NSObject, CXProviderDelegate, PKPushRegistryDelegate {
    static let shared = CallManager()
    private let provider: CXProvider
    private let controller = CXCallController()
    private var registry: PKPushRegistry?
    private var voipToken: String?
    private var calls: [UUID: (incomingId: String?, reason: String, voice: String?)] = [:]
    private var relay: CallAudio?
    private var activeCall: UUID?
    private var ringTimer: Timer?
    var voipReady: Bool { voipToken != nil }

    override init() {
        let config = CXProviderConfiguration()
        config.supportsVideo = false
        config.maximumCallGroups = 1
        config.maximumCallsPerCallGroup = 1
        config.supportedHandleTypes = [.generic]
        config.includesCallsInRecents = true
        provider = CXProvider(configuration: config)
        super.init()
        provider.setDelegate(self, queue: nil)
    }

    func start() {
        guard registry == nil else { return }
        let r = PKPushRegistry(queue: .main)
        r.delegate = self
        r.desiredPushTypes = [.voIP]
        registry = r
    }

    /// The VoIP token goes to the server only once it can send VoIP pushes (platform "ios-voip"); sending it to
    /// /api/push/register today would make the server send alert pushes to a VoIP token, which APNs rejects.
    static let serverTakesVoip = false
    /// True while the call audio lives in the web view. The native CallLine (old relay) stays for a later native call.
    static let webCalls = true

    func repost() {
        if CallManager.serverTakesVoip, let t = voipToken { NativeHub.shared.postDevice("ios-voip", t) }
    }

    /// The call button outside the app: Recents, or the app asking for a native call.
    func startOutgoing(reason: String = "") {
        guard activeCall == nil else { return }
        let uuid = UUID()
        calls[uuid] = (nil, reason, nil)
        let action = CXStartCallAction(call: uuid, handle: CXHandle(type: .generic, value: "nora"))
        action.isVideo = false
        controller.request(CXTransaction(action: action)) { error in
            if error != nil { DispatchQueue.main.async { self.calls.removeValue(forKey: uuid) } }
        }
    }

    /// Tapping Nora in Recents opens the app with a start-call intent.
    func handle(_ activity: NSUserActivity) -> Bool {
        guard activity.activityType == "INStartCallIntent" || activity.interaction?.intent is INStartCallIntent else { return false }
        DispatchQueue.main.async { self.startOutgoing() }
        return true
    }

    // MARK: PushKit

    func pushRegistry(_ registry: PKPushRegistry, didUpdate credentials: PKPushCredentials, for type: PKPushType) {
        voipToken = credentials.token.hex
        repost()
    }

    func pushRegistry(_ registry: PKPushRegistry, didInvalidatePushTokenFor type: PKPushType) {
        voipToken = nil
    }

    func pushRegistry(_ registry: PKPushRegistry, didReceiveIncomingPushWith payload: PKPushPayload,
                      for type: PKPushType, completion: @escaping () -> Void) {
        let p = payload.dictionaryPayload
        let incomingId = (p["ring_id"] as? String) ?? (p["ring_id"] as? NSNumber)?.stringValue ?? (p["incomingId"] as? String)
        let reason = p["reason"] as? String ?? ""
        let voice = p["voice"] as? String
        // iOS stops delivering VoIP pushes to apps that don't show a call for each one, so always report it.
        let uuid = UUID()
        let update = CXCallUpdate()
        update.remoteHandle = CXHandle(type: .generic, value: "nora")
        update.localizedCallerName = p["name"] as? String ?? "Nora"
        update.hasVideo = false
        update.supportsHolding = false
        update.supportsGrouping = false
        update.supportsUngrouping = false
        update.supportsDTMF = false
        provider.reportNewIncomingCall(with: uuid, update: update) { error in
            DispatchQueue.main.async {
                if error == nil {
                    self.calls[uuid] = (incomingId, reason, voice)
                    self.ringTimer?.invalidate()
                    self.ringTimer = Timer.scheduledTimer(withTimeInterval: 45, repeats: false) { [weak self] _ in self?.missed(uuid) }
                    NativeHub.shared.emit("callState", ["state": "ringing", "incomingId": incomingId ?? ""])
                }
                completion()
            }
        }
    }

    private func missed(_ uuid: UUID) {
        guard calls[uuid] != nil, activeCall != uuid else { return }
        provider.reportCall(with: uuid, endedAt: Date(), reason: .unanswered)
        calls.removeValue(forKey: uuid)
    }

    // MARK: CallKit

    func providerDidReset(_ provider: CXProvider) {
        relay?.stop()
        relay = nil
        activeCall = nil
        calls.removeAll()
    }

    func provider(_ provider: CXProvider, perform action: CXStartCallAction) {
        configureAudio()
        let update = CXCallUpdate()
        update.remoteHandle = action.handle
        update.localizedCallerName = "Nora"
        provider.reportCall(with: action.callUUID, updated: update)
        provider.reportOutgoingCall(with: action.callUUID, startedConnectingAt: Date())
        begin(action.callUUID, incomingId: nil, reason: calls[action.callUUID]?.reason ?? "", voice: nil, outgoing: true)
        action.fulfill()
    }

    func provider(_ provider: CXProvider, perform action: CXAnswerCallAction) {
        ringTimer?.invalidate()
        configureAudio()
        let info = calls[action.callUUID]
        begin(action.callUUID, incomingId: info?.incomingId, reason: info?.reason ?? "", voice: info?.voice, outgoing: false)
        action.fulfill()
    }

    func provider(_ provider: CXProvider, perform action: CXEndCallAction) {
        ringTimer?.invalidate()
        if activeCall == action.callUUID {
            relay?.stop()
            relay = nil
            activeCall = nil
        } else if let id = calls[action.callUUID]?.incomingId {
            Task { await Shared.send("/nora/call/decline", body: ["ring_id": (Int(id) as Any?) ?? id]) }
        }
        calls.removeValue(forKey: action.callUUID)
        NativeHub.shared.emit("callState", ["state": "ended"])
        action.fulfill()
    }

    func provider(_ provider: CXProvider, perform action: CXSetMutedCallAction) {
        relay?.muted = action.isMuted
        action.fulfill()
    }

    func provider(_ provider: CXProvider, didActivate audioSession: AVAudioSession) {
        relay?.audioReady()
    }

    func provider(_ provider: CXProvider, didDeactivate audioSession: AVAudioSession) {
        relay?.audioStopped()
    }

    private func configureAudio() {
        try? AVAudioSession.sharedInstance().setCategory(.playAndRecord, mode: .voiceChat, options: [.allowBluetooth, .allowBluetoothA2DP])
    }

    private func begin(_ uuid: UUID, incomingId: String?, reason: String, voice: String?, outgoing: Bool) {
        // New server: the web view runs the call (WebRTC). End the CallKit call and hand over the ring or the start.
        if CallManager.webCalls {
            calls.removeValue(forKey: uuid)
            provider.reportCall(with: uuid, endedAt: Date(), reason: .answeredElsewhere)
            NativeHub.shared.openLink(incomingId.map { "aos://call/ring/\($0)" } ?? "aos://call/start")
            return
        }
        DispatchQueue.main.async {
            FlowRecorder.shared.release()
            if Breath.shared.running { Breath.shared.stop(reason: "call") }
        }
        relay?.stop()
        activeCall = uuid
        let r = CallLine(incomingId: incomingId, reason: reason, hint: voice)
        r.onReady = { [weak self] in
            if outgoing { self?.provider.reportOutgoingCall(with: uuid, connectedAt: Date()) }
        }
        r.onEnd = { [weak self] problem in
            guard let self, self.activeCall == uuid else { return }
            self.relay = nil
            self.activeCall = nil
            self.calls.removeValue(forKey: uuid)
            self.provider.reportCall(with: uuid, endedAt: Date(), reason: problem == nil ? .remoteEnded : .failed)
            NativeHub.shared.emit("callState", ["state": "ended"])
            if let problem { NativeHub.shared.notice("Nora couldn't connect", problem) }
        }
        relay = r
        r.connect()
        NativeHub.shared.emit("callState", ["state": "live"])
    }
}
