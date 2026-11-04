// The Rust core (src/ffi.rs): every command goes in as JSON, and what the window shows comes back as JSON.
// Server calls answer later: the core then asks for a `pump` from the main thread, which applies them.
import COnward
import Foundation

@MainActor
final class Core {
    private let core: OpaquePointer
    private let decoder: JSONDecoder = {
        let d = JSONDecoder()
        d.keyDecodingStrategy = .convertFromSnakeCase
        return d
    }()

    init() {
        core = kr_new(wake, nil)
    }

    func send(_ command: [String: Any]) -> Reply? {
        guard let data = try? JSONSerialization.data(withJSONObject: command),
              let json = String(data: data, encoding: .utf8)
        else { return nil }
        guard let raw = json.withCString({ kr_send(core, $0) }) else { return nil }
        defer { kr_free_string(raw) }
        let reply = Data(bytes: raw, count: strlen(raw))
        do {
            return try decoder.decode(Reply.self, from: reply)
        } catch {
            FileHandle.standardError.write(Data("onward: \(error)\n".utf8))
            return nil
        }
    }
}

/// Called by the core from a background thread when answers are waiting: they are applied on the main thread.
/// Outside `Core`, so it isn't taken for main-actor code. A C callback can't capture anything; the one model is
/// reached through `Model.shared`.
private func wake(_: UnsafeMutableRawPointer?) {
    DispatchQueue.main.async {
        MainActor.assumeIsolated { Model.shared.pump() }
    }
}
