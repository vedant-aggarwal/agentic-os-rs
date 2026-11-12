import ImageIO
import SwiftUI
import UIKit
import UniformTypeIdentifiers

/// Share to Agentic OS from any app: text, a link or a photo. A photo goes to /api/media, then the share goes to Nora
/// as one sentence (/api/nora/messages) and she files it. Offline, text and links wait in the shared queue; a photo
/// needs the server.
final class ShareViewController: UIViewController {
    private let model = ShareModel()

    override func viewDidLoad() {
        super.viewDidLoad()
        let host = UIHostingController(rootView: ShareView(model: model) { [weak self] in
            self?.extensionContext?.completeRequest(returningItems: nil)
        })
        addChild(host)
        host.view.frame = view.bounds
        host.view.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        view.addSubview(host.view)
        host.didMove(toParent: self)
        let items = (extensionContext?.inputItems as? [NSExtensionItem]) ?? []
        Task { await model.load(items) }
    }
}

@MainActor
final class ShareModel: ObservableObject {
    enum Stage: Equatable { case loading, editing, sending, done(String), queued, failed(String) }

    @Published var text = ""
    @Published var link = ""
    @Published var note = ""
    @Published var preview: UIImage?
    @Published var stage: Stage = .loading
    private var imageB64: String?
    private let id = "sh_" + String(UUID().uuidString.lowercased().replacingOccurrences(of: "-", with: "").prefix(20))

    func load(_ items: [NSExtensionItem]) async {
        for item in items {
            if text.isEmpty, let t = item.attributedContentText?.string.trimmingCharacters(in: .whitespacesAndNewlines), !t.isEmpty { text = t }
            for p in item.attachments ?? [] {
                if link.isEmpty, p.hasItemConformingToTypeIdentifier(UTType.url.identifier),
                   let u = try? await p.loadItem(forTypeIdentifier: UTType.url.identifier) as? URL, !u.isFileURL {
                    link = u.absoluteString
                } else if imageB64 == nil, p.hasItemConformingToTypeIdentifier(UTType.image.identifier),
                          let data = await ShareModel.imageData(p) {
                    if let encoded = ShareModel.encode(data) {
                        imageB64 = encoded.0
                        preview = encoded.1
                    }
                } else if text.isEmpty, p.hasItemConformingToTypeIdentifier(UTType.plainText.identifier),
                          let raw = try? await p.loadItem(forTypeIdentifier: UTType.plainText.identifier) {
                    text = (raw as? String) ?? (raw as? Data).flatMap { String(data: $0, encoding: .utf8) } ?? ""
                }
            }
        }
        // A shared link often arrives as text too.
        if text == link { text = "" }
        stage = .editing
    }

    var hasImage: Bool { imageB64 != nil }
    var canSend: Bool {
        guard stage == .editing || isFailed else { return false }
        return hasImage || !link.isEmpty || !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }
    private var isFailed: Bool { if case .failed = stage { return true }; return false }

    func send() async {
        guard Shared.token != nil else { stage = .failed("Open Agentic OS and sign in once, then share again."); return }
        stage = .sending
        var body: [String: Any] = ["id": id]
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        let n = note.trimmingCharacters(in: .whitespacesAndNewlines)
        if !t.isEmpty { body["text"] = t }
        if !link.isEmpty { body["url"] = link }
        if !n.isEmpty { body["note"] = n }
        // The new server has no share inbox yet (needed: POST /api/inbox/share). Until then a photo goes to
        // /api/media and the share goes to Nora as one sentence, and she files it with her tools.
        var mediaId: String?
        if let imageB64, let jpeg = Data(base64Encoded: imageB64) {
            let (code, data) = await Shared.request("/media", method: "POST", data: jpeg, type: "image/jpeg", timeout: 60)
            guard (200..<300).contains(code), let mid = Shared.json(data)?["id"] as? String else {
                stage = .failed(code == 0 ? "Couldn't reach your server. A photo needs it on; try again." :
                                code == 401 ? "Signed out. Open Agentic OS and pair again." : "The photo didn't upload (\(code)). Try again.")
                return
            }
            mediaId = mid
        }
        let sentence = "Shared with me: " + [t, link, n.isEmpty ? "" : "(note: \(n))", mediaId.map { "[image \($0)]" } ?? ""]
            .filter { !$0.isEmpty }.joined(separator: " ")
        body["text"] = sentence
        let (code, data) = await Shared.send("/nora/messages", body: ["text": sentence], timeout: 20)
        switch code {
        case 200, 201, 202:
            stage = .done("Sent to Nora to file.")
        case 0, 500...599:
            Shared.enqueue("/nora/messages", body: ["text": sentence])
            stage = .queued
        case 401:
            stage = .failed("Signed out. Open Agentic OS and pair again.")
        default:
            stage = .failed(ShareModel.message(data) ?? "Your server didn't take it (\(code)). Try again.")
        }
    }

    /// What she did with it: "Reel idea → Content · Ideas", or what it is and where it waits.
    static func said(_ item: [String: Any]?) -> String {
        guard let item else { return "Filed. Undo is in the app." }
        let title = (item["title"] as? String).flatMap { $0.isEmpty ? nil : $0 }
        if let filed = item["filed"] as? [String: Any], let where_ = filed["where"] as? String {
            return title.map { "\($0) → \(where_)" } ?? "Filed in \(where_)"
        }
        if let reply = item["reply"] as? String, !reply.isEmpty { return reply }
        if let title { return "\(title): ready to file in the app." }
        return "Got it. It's in Agentic OS."
    }

    static func message(_ data: Data?) -> String? {
        (Shared.json(data)?["error"] as? String).flatMap { $0.isEmpty ? nil : $0 }
    }

    static func imageData(_ p: NSItemProvider) async -> Data? {
        await withCheckedContinuation { (cont: CheckedContinuation<Data?, Never>) in
            _ = p.loadDataRepresentation(forTypeIdentifier: UTType.image.identifier) { data, _ in cont.resume(returning: data) }
        }
    }

    /// A JPEG small enough for the PC's 200 KB JSON limit (base64 included), made with ImageIO so a 48 MP photo never
    /// decodes in full inside the extension. Also returns a thumbnail for the sheet.
    static func encode(_ data: Data) -> (String, UIImage)? {
        guard let src = CGImageSourceCreateWithData(data as CFData, nil) else { return nil }
        for (side, quality) in [(1400, 0.6), (1100, 0.55), (900, 0.5), (700, 0.45), (520, 0.4)] {
            let opts: [CFString: Any] = [kCGImageSourceCreateThumbnailFromImageAlways: true,
                                         kCGImageSourceCreateThumbnailWithTransform: true,
                                         kCGImageSourceThumbnailMaxPixelSize: side]
            guard let cg = CGImageSourceCreateThumbnailAtIndex(src, 0, opts as CFDictionary) else { continue }
            let img = UIImage(cgImage: cg)
            guard let jpeg = img.jpegData(compressionQuality: CGFloat(quality)) else { continue }
            let b64 = jpeg.base64EncodedString()
            if b64.count < 180_000 { return (b64, img) }
        }
        return nil
    }
}

struct ShareView: View {
    @ObservedObject var model: ShareModel
    let close: () -> Void
    @FocusState var focused: Bool

    var body: some View {
        NavigationStack {
            Form {
                if model.stage == .loading {
                    Section { HStack { ProgressView(); Text("Reading what you shared…").foregroundStyle(.secondary) } }
                } else {
                    Section {
                        if let img = model.preview {
                            Image(uiImage: img).resizable().scaledToFit().frame(maxHeight: 180)
                                .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                                .listRowInsets(EdgeInsets(top: 8, leading: 8, bottom: 8, trailing: 8))
                        }
                        if !model.link.isEmpty {
                            Label(model.link, systemImage: "link").font(.footnote).foregroundStyle(.secondary).lineLimit(2)
                        }
                        if model.hasImage || !model.link.isEmpty {
                            if !model.text.isEmpty {
                                Text(model.text).font(.footnote).foregroundStyle(.secondary).lineLimit(3)
                            }
                            TextField("Add a note (optional)", text: $model.note, axis: .vertical)
                                .lineLimit(1...4)
                                .focused($focused)
                        } else {
                            TextField("What is it?", text: $model.text, axis: .vertical)
                                .lineLimit(2...8)
                                .focused($focused)
                        }
                    } footer: {
                        Text("Your agent files it where it belongs and tells you. Undo is in the app.")
                    }
                }
                switch model.stage {
                case .done(let said):
                    Section { Label(said, systemImage: "checkmark.circle.fill").foregroundStyle(.green) }
                case .queued:
                    Section { Label("Saved on the phone. It's filed when your PC is reachable.", systemImage: "tray.and.arrow.down.fill").foregroundStyle(.orange) }
                case .failed(let why):
                    Section { Text(why).foregroundStyle(.red) }
                default:
                    EmptyView()
                }
            }
            .navigationTitle("Agentic OS")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel", action: close) }
                ToolbarItem(placement: .confirmationAction) {
                    if model.stage == .sending {
                        HStack(spacing: 6) { ProgressView(); Text("Filing").foregroundStyle(.secondary) }
                    } else if isFinished {
                        Button("Done", action: close).bold()
                    } else {
                        Button("Share") {
                            Task {
                                await model.send()
                                if isFinished {
                                    try? await Task.sleep(nanoseconds: 1_600_000_000)
                                    close()
                                }
                            }
                        }
                        .bold()
                        .disabled(!model.canSend)
                    }
                }
            }
            .onAppear { focused = true }
        }
    }

    private var isFinished: Bool {
        switch model.stage {
        case .done, .queued: return true
        default: return false
        }
    }
}
