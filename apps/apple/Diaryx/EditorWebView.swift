import SwiftUI
import WebKit

#if os(iOS)
struct EditorWebView: UIViewRepresentable {
    let initialMarkdown: String
    var onContentChanged: ((String) -> Void)?
    var onLinkClicked: ((String) -> Void)?

    func makeUIView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        config.userContentController.add(context.coordinator, name: "bridge")
        config.preferences.setValue(true, forKey: "allowFileAccessFromFileURLs")

        let webView = WKWebView(frame: .zero, configuration: config)
        webView.navigationDelegate = context.coordinator
        webView.isInspectable = true
        webView.scrollView.keyboardDismissMode = .interactive
        context.coordinator.webView = webView
        context.coordinator.pendingMarkdown = initialMarkdown

        if let distURL = Bundle.main.url(forResource: "dist", withExtension: nil),
           let indexURL = Bundle.main.url(forResource: "index", withExtension: "html", subdirectory: "dist") {
            webView.loadFileURL(indexURL, allowingReadAccessTo: distURL)
        } else {
            print("Error: Could not find editor-bundle in app bundle")
        }

        return webView
    }

    func updateUIView(_ webView: WKWebView, context: Context) {
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
}
#else
struct EditorWebView: NSViewRepresentable {
    let initialMarkdown: String
    var onContentChanged: ((String) -> Void)?
    var onLinkClicked: ((String) -> Void)?

    func makeNSView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        config.userContentController.add(context.coordinator, name: "bridge")
        config.preferences.setValue(true, forKey: "allowFileAccessFromFileURLs")

        let webView = WKWebView(frame: .zero, configuration: config)
        webView.navigationDelegate = context.coordinator
        webView.isInspectable = true
        context.coordinator.webView = webView
        context.coordinator.pendingMarkdown = initialMarkdown

        if let distURL = Bundle.main.url(forResource: "dist", withExtension: nil),
           let indexURL = Bundle.main.url(forResource: "index", withExtension: "html", subdirectory: "dist") {
            webView.loadFileURL(indexURL, allowingReadAccessTo: distURL)
        } else {
            print("Error: Could not find editor-bundle in app bundle")
        }

        return webView
    }

    func updateNSView(_ webView: WKWebView, context: Context) {
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
}
#endif

// MARK: - Shared Coordinator

extension EditorWebView {
    class Coordinator: NSObject, WKScriptMessageHandler, WKNavigationDelegate {
        weak var webView: WKWebView?
        var isReady = false
        var pendingMarkdown: String = ""
        var onContentChanged: ((String) -> Void)?
        var onLinkClicked: ((String) -> Void)?

        private var lastSetContent: String?
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
            if navigationAction.navigationType == .other ||
               navigationAction.request.url?.isFileURL == true {
                return .allow
            }

            if let url = navigationAction.request.url {
                openExternalURL(url)
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

        // MARK: - Platform Helpers

        private func openExternalURL(_ url: URL) {
            #if os(iOS)
            UIApplication.shared.open(url)
            #else
            NSWorkspace.shared.open(url)
            #endif
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
