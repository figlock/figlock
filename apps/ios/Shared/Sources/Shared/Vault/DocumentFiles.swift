import FigLockFFI
import Foundation
import UniformTypeIdentifiers

/// Reading a file the person picked into a document item, and saving one back out (ADR-0024).
public enum DocumentFiles {
    /// Read the file at `url` (a Files picker result: security scoped), checking its size before reading it.
    public static func read(_ url: URL, id: String) throws -> DraftFile {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        let name = fitName(url.lastPathComponent)
        if let size = try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize, size > ItemDraft.maxFileBytes {
            throw ItemDraftError.fileTooLarge(name)
        }
        guard let data = try? Data(contentsOf: url) else { throw ItemDraftError.unreadableFile(name) }
        guard data.count <= ItemDraft.maxFileBytes else { throw ItemDraftError.fileTooLarge(name) }
        return DraftFile(id: id, name: name, mime: mime(for: name), data: data)
    }

    /// The media type for a file name, from its extension; `application/octet-stream` when unknown.
    static func mime(for name: String) -> String {
        let ext = (name as NSString).pathExtension
        let mime = UTType(filenameExtension: ext)?.preferredMIMEType ?? "application/octet-stream"
        return mime.utf8.count <= 256 ? mime : "application/octet-stream"
    }

    /// A file name inside the 1 KiB limit of the field.
    static func fitName(_ name: String) -> String {
        var fitted = name
        while fitted.utf8.count > 1024 { fitted.removeLast() }
        return fitted
    }
}

/// Getting a stored file back out. The contents are a secret field, so this asks for Face ID first, as reveal does.
@MainActor
public final class FileExtraction {
    private let session: VaultSession
    private let itemId: String
    private let authenticator: any Authenticating

    public init(
        session: VaultSession, itemId: String, authenticator: any Authenticating = DeviceAuthenticator()
    ) {
        self.session = session
        self.itemId = itemId
        self.authenticator = authenticator
    }

    /// The bytes of `file`, or `nil` when the person did not pass Face ID or the stored value is damaged.
    public func contents(of file: VaultItem.AttachedFile) async -> Data? {
        let allowed = await authenticator.authenticate(reason: "Save \(file.name) from FigLock")
        guard allowed, !Task.isCancelled else { return nil }
        guard let text = try? session.revealField(itemId: itemId, field: file.dataField) else { return nil }
        return Data(base64Encoded: text)
    }
}
