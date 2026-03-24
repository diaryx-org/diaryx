import UIKit
import Tauri
import SwiftRs

// MARK: - Argument types

class TriggerDownloadArgs: Decodable {
    let path: String
}

class MigrateArgs: Decodable {
    let sourcePath: String
    let destPath: String
}

// MARK: - iCloud Plugin

private let kContainerIdentifier = "iCloud.org.diaryx.app"

class ICloudPlugin: Plugin {
    private var metadataQuery: NSMetadataQuery?
    private var queryObserver: NSObjectProtocol?

    // MARK: - Commands

    @objc public func checkIcloudAvailable(_ invoke: Invoke) {
        let token = FileManager.default.ubiquityIdentityToken
        invoke.resolve([
            "isAvailable": token != nil,
        ])
    }

    @objc public func getIcloudContainerUrl(_ invoke: Invoke) async throws {
        // url(forUbiquityContainerIdentifier:) can block; run on background queue
        let result: [String: String]? = await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                guard let containerUrl = FileManager.default.url(
                    forUbiquityContainerIdentifier: kContainerIdentifier
                ) else {
                    continuation.resume(returning: nil)
                    return
                }
                let documentsUrl = containerUrl.appendingPathComponent("Documents")

                // Ensure Documents subdirectory exists
                try? FileManager.default.createDirectory(
                    at: documentsUrl,
                    withIntermediateDirectories: true
                )

                continuation.resume(returning: [
                    "containerUrl": containerUrl.path,
                    "documentsUrl": documentsUrl.path,
                ])
            }
        }

        guard let result = result else {
            invoke.reject("iCloud container not available. Ensure iCloud Drive is enabled and the app has the correct entitlements.")
            return
        }
        invoke.resolve(result)
    }

    @objc public func triggerDownload(_ invoke: Invoke) async throws {
        let args = try invoke.parseArgs(TriggerDownloadArgs.self)
        let fileManager = FileManager.default
        let url = URL(fileURLWithPath: args.path)

        // Check for .icloud placeholder pattern: .filename.icloud
        let parentDir = url.deletingLastPathComponent()
        let filename = url.lastPathComponent
        let placeholderName = ".\(filename).icloud"
        let placeholderUrl = parentDir.appendingPathComponent(placeholderName)

        let targetUrl: URL
        if fileManager.fileExists(atPath: placeholderUrl.path) {
            targetUrl = placeholderUrl
        } else if fileManager.fileExists(atPath: url.path) {
            // File already downloaded
            invoke.resolve(["success": true])
            return
        } else {
            // Try the path as-is (might be a directory)
            targetUrl = url
        }

        do {
            try fileManager.startDownloadingUbiquitousItem(at: targetUrl)
            invoke.resolve(["success": true])
        } catch {
            invoke.reject("Failed to trigger download: \(error.localizedDescription)")
        }
    }

    @objc public func getSyncStatus(_ invoke: Invoke) {
        guard let query = metadataQuery else {
            invoke.resolve([
                "totalItems": 0,
                "uploading": 0,
                "downloading": 0,
                "upToDate": true,
            ])
            return
        }

        query.disableUpdates()
        let results = query.results as? [NSMetadataItem] ?? []

        var uploading = 0
        var downloading = 0

        for item in results {
            if let status = item.value(forAttribute: NSMetadataUbiquitousItemUploadingErrorKey) as? String,
               status == "uploading" {
                uploading += 1
            } else if let uploadPercent = item.value(forAttribute: NSMetadataUbiquitousItemPercentUploadedKey) as? Double,
                      uploadPercent < 100 {
                uploading += 1
            }

            if let downloadStatus = item.value(forAttribute: NSMetadataUbiquitousItemDownloadingStatusKey) as? String,
               downloadStatus != NSMetadataUbiquitousItemDownloadingStatusCurrent {
                downloading += 1
            }
        }

        let total = results.count
        let upToDate = uploading == 0 && downloading == 0

        query.enableUpdates()

        invoke.resolve([
            "totalItems": total,
            "uploading": uploading,
            "downloading": downloading,
            "upToDate": upToDate,
        ])
    }

    @objc public func startStatusMonitoring(_ invoke: Invoke) {
        // Stop any existing query first
        stopQuery()

        let query = NSMetadataQuery()
        query.searchScopes = [NSMetadataQueryUbiquitousDocumentsScope]
        query.predicate = NSPredicate(format: "%K LIKE '*'", NSMetadataItemFSNameKey)

        queryObserver = NotificationCenter.default.addObserver(
            forName: .NSMetadataQueryDidUpdate,
            object: query,
            queue: .main
        ) { [weak self] _ in
            self?.emitSyncStatusEvent()
        }

        metadataQuery = query
        query.start()

        invoke.resolve(["success": true])
    }

    @objc public func stopStatusMonitoring(_ invoke: Invoke) {
        stopQuery()
        invoke.resolve(["success": true])
    }

    @objc public func migrateToIcloud(_ invoke: Invoke) async throws {
        let args = try invoke.parseArgs(MigrateArgs.self)
        let sourceUrl = URL(fileURLWithPath: args.sourcePath)
        let destUrl = URL(fileURLWithPath: args.destPath)

        let result = await withCheckedContinuation { (continuation: CheckedContinuation<Result<Int, Error>, Never>) in
            DispatchQueue.global(qos: .userInitiated).async {
                let fm = FileManager.default
                var count = 0

                do {
                    // Ensure destination exists
                    try fm.createDirectory(at: destUrl, withIntermediateDirectories: true)

                    // Enumerate all items in source
                    guard let enumerator = fm.enumerator(
                        at: sourceUrl,
                        includingPropertiesForKeys: [.isDirectoryKey],
                        options: [.skipsHiddenFiles]
                    ) else {
                        continuation.resume(returning: .success(0))
                        return
                    }

                    for case let fileUrl as URL in enumerator {
                        let relativePath = fileUrl.path.replacingOccurrences(
                            of: sourceUrl.path + "/",
                            with: ""
                        )
                        let destFileUrl = destUrl.appendingPathComponent(relativePath)

                        let resourceValues = try fileUrl.resourceValues(forKeys: [.isDirectoryKey])
                        if resourceValues.isDirectory == true {
                            try fm.createDirectory(at: destFileUrl, withIntermediateDirectories: true)
                        } else {
                            // Create parent directory if needed
                            let parent = destFileUrl.deletingLastPathComponent()
                            try fm.createDirectory(at: parent, withIntermediateDirectories: true)

                            // Use setUbiquitous for proper iCloud awareness
                            if fm.fileExists(atPath: destFileUrl.path) {
                                try fm.removeItem(at: destFileUrl)
                            }
                            try fm.setUbiquitous(true, itemAt: fileUrl, destinationURL: destFileUrl)
                            count += 1
                        }
                    }

                    continuation.resume(returning: .success(count))
                } catch {
                    continuation.resume(returning: .failure(error))
                }
            }
        }

        switch result {
        case .success(let count):
            invoke.resolve(["filesMigrated": count])
        case .failure(let error):
            invoke.reject("Migration to iCloud failed: \(error.localizedDescription)")
        }
    }

    @objc public func migrateFromIcloud(_ invoke: Invoke) async throws {
        let args = try invoke.parseArgs(MigrateArgs.self)
        let sourceUrl = URL(fileURLWithPath: args.sourcePath)
        let destUrl = URL(fileURLWithPath: args.destPath)

        let result = await withCheckedContinuation { (continuation: CheckedContinuation<Result<Int, Error>, Never>) in
            DispatchQueue.global(qos: .userInitiated).async {
                let fm = FileManager.default
                var count = 0

                do {
                    // Ensure destination exists
                    try fm.createDirectory(at: destUrl, withIntermediateDirectories: true)

                    // Enumerate all items in source
                    guard let enumerator = fm.enumerator(
                        at: sourceUrl,
                        includingPropertiesForKeys: [.isDirectoryKey],
                        options: [.skipsHiddenFiles]
                    ) else {
                        continuation.resume(returning: .success(0))
                        return
                    }

                    for case let fileUrl as URL in enumerator {
                        let relativePath = fileUrl.path.replacingOccurrences(
                            of: sourceUrl.path + "/",
                            with: ""
                        )
                        let destFileUrl = destUrl.appendingPathComponent(relativePath)

                        let resourceValues = try fileUrl.resourceValues(forKeys: [.isDirectoryKey])
                        if resourceValues.isDirectory == true {
                            try fm.createDirectory(at: destFileUrl, withIntermediateDirectories: true)
                        } else {
                            let parent = destFileUrl.deletingLastPathComponent()
                            try fm.createDirectory(at: parent, withIntermediateDirectories: true)

                            if fm.fileExists(atPath: destFileUrl.path) {
                                try fm.removeItem(at: destFileUrl)
                            }
                            try fm.copyItem(at: fileUrl, to: destFileUrl)
                            count += 1
                        }
                    }

                    continuation.resume(returning: .success(count))
                } catch {
                    continuation.resume(returning: .failure(error))
                }
            }
        }

        switch result {
        case .success(let count):
            invoke.resolve(["filesMigrated": count])
        case .failure(let error):
            invoke.reject("Migration from iCloud failed: \(error.localizedDescription)")
        }
    }

    // MARK: - Private helpers

    private func stopQuery() {
        if let observer = queryObserver {
            NotificationCenter.default.removeObserver(observer)
            queryObserver = nil
        }
        metadataQuery?.stop()
        metadataQuery = nil
    }

    private func emitSyncStatusEvent() {
        guard let query = metadataQuery else { return }

        query.disableUpdates()
        let results = query.results as? [NSMetadataItem] ?? []

        var uploading = 0
        var downloading = 0

        for item in results {
            if let uploadPercent = item.value(forAttribute: NSMetadataUbiquitousItemPercentUploadedKey) as? Double,
               uploadPercent < 100 {
                uploading += 1
            }

            if let downloadStatus = item.value(forAttribute: NSMetadataUbiquitousItemDownloadingStatusKey) as? String,
               downloadStatus != NSMetadataUbiquitousItemDownloadingStatusCurrent {
                downloading += 1
            }
        }

        let total = results.count
        let upToDate = uploading == 0 && downloading == 0
        query.enableUpdates()

        // Emit Tauri event to the frontend
        self.trigger("icloud-sync-status-changed", data: [
            "totalItems": total,
            "uploading": uploading,
            "downloading": downloading,
            "upToDate": upToDate,
        ])
    }

    deinit {
        stopQuery()
    }
}

// MARK: - Plugin entry point

@_cdecl("init_plugin_icloud")
func initPlugin() -> Plugin {
    return ICloudPlugin()
}
