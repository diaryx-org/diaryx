import SwiftUI
import WebKit

struct EditorWebView: NSViewRepresentable {
    let initialMarkdown: String
    var onContentChanged: ((String) -> Void)?
    var onLinkClicked: ((String) -> Void)?

    func makeNSView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        config.userContentController.add(context.coordinator, name: "bridge")

        // Allow file access for loading local resources
        config.preferences.setValue(true, forKey: "allowFileAccessFromFileURLs")

        let webView = WKWebView(frame: .zero, configuration: config)
        webView.navigationDelegate = context.coordinator
        webView.isInspectable = true  // Enable Safari Web Inspector (Develop menu)
        context.coordinator.webView = webView
        context.coordinator.pendingMarkdown = initialMarkdown

        // Load the bundled editor HTML from editor-bundle/dist
        if let distURL = Bundle.main.url(forResource: "dist", withExtension: nil),
           let indexURL = Bundle.main.url(forResource: "index", withExtension: "html", subdirectory: "dist") {
            webView.loadFileURL(indexURL, allowingReadAccessTo: distURL)
        } else {
            print("Error: Could not find editor-bundle in app bundle")
        }

        return webView
    }

        func updateNSView(_ webView: WKWebView, context: Context) {
            // Only push content if it changed externally (e.g., file switch)
            if context.coordinator.pendingMarkdown != initialMarkdown {
                context.coordinator.pendingMarkdown = initialMarkdown
                if context.coordinator.isReady {
                    context.coordinator.setMarkdown(initialMarkdown)
                }
            }
        }

    func makeCoordinator() -> Coordinator {
        Coordinator(onContentChanged: onContentChanged, onLinkClicked: onLinkClicked)
    }

    class Coordinator: NSObject, WKScriptMessageHandler, WKNavigationDelegate {
        weak var webView: WKWebView?
        var isReady = false
        var pendingMarkdown: String = ""
        var onContentChanged: ((String) -> Void)?
        var onLinkClicked: ((String) -> Void)?

        // Track the last content we sent TO the editor to avoid echo loops
        private var lastSetContent: String?
        // Track the last content we received FROM the editor
        private var lastReceivedContent: String?

        init(onContentChanged: ((String) -> Void)?, onLinkClicked: ((String) -> Void)?) {
            self.onContentChanged = onContentChanged
            self.onLinkClicked = onLinkClicked
        }

        // MARK: - WKScriptMessageHandler

        func userContentController(
            _ userContentController: WKUserContentController,
            didReceive message: WKScriptMessage
        ) {
            guard let body = message.body as? [String: Any],
                  let type = body["type"] as? String else { return }

            switch type {
            case "ready":
                isReady = true
                if !pendingMarkdown.isEmpty {
                    setMarkdown(pendingMarkdown)
                }

            case "contentChanged":
                if let markdown = body["markdown"] as? String {
                    // Don't fire callback if this is just echoing back what we set
                    if markdown != lastSetContent {
                        lastReceivedContent = markdown
                        onContentChanged?(markdown)
                    }
                }

            case "linkClicked":
                if let href = body["href"] as? String {
                    onLinkClicked?(href)
                }

            default:
                break
            }
        }

        // MARK: - WKNavigationDelegate

        @MainActor
        func webView(
            _ webView: WKWebView,
            decidePolicyFor navigationAction: WKNavigationAction
        ) async -> WKNavigationActionPolicy {
            // Allow loading the initial page and file:// resources
            if navigationAction.navigationType == .other ||
               navigationAction.request.url?.isFileURL == true {
                return .allow
            }

            // Open external links in the default browser
            if let url = navigationAction.request.url {
                NSWorkspace.shared.open(url)
            }
            return .cancel
        }

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            print("[EditorWebView] Page loaded successfully")
        }

        func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
            print("[EditorWebView] Navigation failed: \(error)")
        }

        func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
            print("[EditorWebView] Provisional navigation failed: \(error)")
        }

        // MARK: - Bridge Methods

        private func quotedForJavaScript(_ value: String) -> String {
            guard let data = try? JSONEncoder().encode(value),
                  let encoded = String(data: data, encoding: .utf8) else {
                return "\"\""
            }
            return encoded
        }

        func setMarkdown(_ markdown: String) {
            guard let webView = webView else { return }
            lastSetContent = markdown
            let escaped = quotedForJavaScript(markdown)
            webView.evaluateJavaScript("editorBridge.setMarkdown(\(escaped))") { [weak self] _, error in
                if let error = error {
                    print("Error setting markdown: \(error)")
                } else {
                    // Read back TipTap's normalized output so the echo guard
                    // baseline matches what the editor actually produces.
                    // Without this, TipTap's whitespace normalization makes the
                    // initial content look "changed" and triggers a false save.
                    self?.getMarkdown { normalized in
                        if let normalized = normalized {
                            self?.lastSetContent = normalized
                        }
                    }
                }
            }
        }

        func getMarkdown(completion: @escaping (String?) -> Void) {
            guard let webView = webView else {
                completion(nil)
                return
            }
            webView.evaluateJavaScript("editorBridge.getMarkdown()") { result, error in
                if let error = error {
                    print("Error getting markdown: \(error)")
                    completion(nil)
                } else {
                    completion(result as? String)
                }
            }
        }

        func setJSON(_ json: String) {
            guard let webView = webView else { return }
            let escaped = quotedForJavaScript(json)
            webView.evaluateJavaScript("editorBridge.setJSON(\(escaped))") { _, error in
                if let error = error {
                    print("Error setting editor JSON: \(error)")
                }
            }
        }

        func getJSON(completion: @escaping (String?) -> Void) {
            guard let webView = webView else {
                completion(nil)
                return
            }
            webView.evaluateJavaScript("editorBridge.getJSON()") { result, error in
                if let error = error {
                    print("Error getting editor JSON: \(error)")
                    completion(nil)
                } else {
                    completion(result as? String)
                }
            }
        }
    }
}
