import UIKit
import WebKit
import Tauri

// MARK: - Plugin Entry Point

@_cdecl("init_plugin_editor_toolbar")
func initPlugin() -> Plugin {
    return EditorToolbarPlugin()
}

// MARK: - Editor Toolbar Plugin

// MARK: - Image Context Menu Cache

struct CachedImageInfo {
    let nodePos: Int
    let src: String
    let alt: String
    let width: Int?
    let height: Int?
    let naturalWidth: Int?
    let naturalHeight: Int?
    let isVideo: Bool
    let isHtmlBlock: Bool
    let thumbnailImage: UIImage?
    let timestamp: TimeInterval
}

class EditorToolbarPlugin: Plugin, WKScriptMessageHandler {
    private weak var webView: WKWebView?
    private var toolbar: EditorToolbar?
    private var keyboardObserver: NSObjectProtocol?
    private var cachedImageInfo: CachedImageInfo?
    private var contextMenuInteraction: UIContextMenuInteraction?
    private var pendingImageDataCompletion: ((Data?) -> Void)?

    @objc public override func load(webview: WKWebView) {
        self.webView = webview

        let editorToolbar = EditorToolbar(webView: webview)
        self.toolbar = editorToolbar

        // Register message handler for JS -> Swift state updates.
        // Use a weak wrapper to avoid retain cycle with userContentController.
        let handler = WeakScriptMessageHandler(delegate: self)
        webview.configuration.userContentController.add(handler, name: "editorToolbar")

        // Inject state-reporting JS that runs after each page load
        let script = WKUserScript(
            source: Self.stateReportingScript,
            injectionTime: .atDocumentEnd,
            forMainFrameOnly: true
        )
        webview.configuration.userContentController.addUserScript(script)

        // Attempt swizzle once the view hierarchy is ready.
        // If WKContentView isn't available yet, retry on first keyboard show.
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { [weak self, weak webview] in
            guard let webview = webview, let toolbar = self?.toolbar else { return }
            if !Self.swizzleInputAccessoryView(for: webview, toolbar: toolbar) {
                self?.observeKeyboardForSwizzle(webview: webview, toolbar: toolbar)
            }
            self?.setupContextMenuInteraction(for: webview)
        }
    }

    // MARK: - Keyboard Observer Fallback

    /// If WKContentView wasn't found on first attempt, retry when the keyboard shows
    /// (which guarantees WKContentView exists as first responder).
    private func observeKeyboardForSwizzle(webview: WKWebView, toolbar: EditorToolbar) {
        keyboardObserver = NotificationCenter.default.addObserver(
            forName: UIResponder.keyboardWillShowNotification,
            object: nil,
            queue: .main
        ) { [weak self, weak webview] _ in
            guard let self = self, let webview = webview, let toolbar = self.toolbar else { return }
            if Self.swizzleInputAccessoryView(for: webview, toolbar: toolbar) {
                // Success — stop observing
                if let observer = self.keyboardObserver {
                    NotificationCenter.default.removeObserver(observer)
                    self.keyboardObserver = nil
                }
            }
        }
    }

    // MARK: - WKScriptMessageHandler

    func userContentController(
        _ userContentController: WKUserContentController,
        didReceive message: WKScriptMessage
    ) {
        guard let body = message.body as? [String: Any],
              let type = body["type"] as? String else { return }

        switch type {
        case "stateUpdate":
            guard let states = body["activeStates"] as? [String: Bool],
                  let canUndo = body["canUndo"] as? Bool,
                  let canRedo = body["canRedo"] as? Bool else { return }
            let editable = body["editable"] as? Bool ?? true
            toolbar?.updateState(activeStates: states, canUndo: canUndo, canRedo: canRedo, editable: editable)
        case "focusChange":
            guard let focused = body["focused"] as? Bool,
                  let webView = webView else { return }
            if let contentView = Self.findWKContentView(in: webView) {
                objc_setAssociatedObject(contentView, &Self.associatedEditorFocusedKey, focused, .OBJC_ASSOCIATION_RETAIN_NONATOMIC)
                contentView.reloadInputViews()
            }
        case "pluginCommands":
            guard let commands = body["commands"] as? [String: Any] else { return }
            toolbar?.updatePluginCommands(commands)
        case "imagePreview":
            guard let base64 = body["base64"] as? String,
                  let data = Data(base64Encoded: base64),
                  let image = UIImage(data: data) else { return }
            let name = body["name"] as? String ?? "Image"
            presentImagePreview(image: image, name: name)
        case "imageContextPrepare":
            guard let nodePos = body["nodePos"] as? Int,
                  let src = body["src"] as? String else { return }
            var thumbnail: UIImage? = nil
            if let b64 = body["thumbnailBase64"] as? String,
               let data = Data(base64Encoded: b64) {
                thumbnail = UIImage(data: data)
            }
            cachedImageInfo = CachedImageInfo(
                nodePos: nodePos,
                src: src,
                alt: body["alt"] as? String ?? "",
                width: body["width"] as? Int,
                height: body["height"] as? Int,
                naturalWidth: body["naturalWidth"] as? Int,
                naturalHeight: body["naturalHeight"] as? Int,
                isVideo: body["isVideo"] as? Bool ?? false,
                isHtmlBlock: body["isHtmlBlock"] as? Bool ?? false,
                thumbnailImage: thumbnail,
                timestamp: Date().timeIntervalSince1970
            )
        case "imageContextClear":
            cachedImageInfo = nil
        case "imageDataResult":
            let completion = pendingImageDataCompletion
            pendingImageDataCompletion = nil
            if let b64 = body["base64"] as? String, let data = Data(base64Encoded: b64) {
                completion?(data)
            } else {
                completion?(nil)
            }
        default:
            break
        }
    }

    private func presentImagePreview(image: UIImage, name: String) {
        guard let vc = toolbar?.findViewController() else { return }
        let previewVC = ImagePreviewViewController(image: image, name: name)
        previewVC.modalPresentationStyle = .fullScreen
        previewVC.modalTransitionStyle = .crossDissolve
        vc.present(previewVC, animated: true)
    }
}

// MARK: - WKContentView Swizzle

extension EditorToolbarPlugin {
    private static var associatedToolbarKey = "diaryxEditorToolbar"
    private static var associatedEditorFocusedKey = "diaryxEditorFocused"

    /// Swizzle WKContentView's inputAccessoryView to return our toolbar.
    /// Returns true if the swizzle was applied, false if WKContentView wasn't found.
    @discardableResult
    static func swizzleInputAccessoryView(for webView: WKWebView, toolbar: UIView) -> Bool {
        guard let contentView = findWKContentView(in: webView) else { return false }

        let subclassName = "Diaryx_WKContentView"

        // Only create the dynamic subclass once across the app lifetime
        if let existingClass = NSClassFromString(subclassName) {
            object_setClass(contentView, existingClass)
            objc_setAssociatedObject(contentView, &associatedToolbarKey, toolbar, .OBJC_ASSOCIATION_RETAIN_NONATOMIC)
            return true
        }

        let contentViewClass: AnyClass = type(of: contentView)
        guard let subclass = objc_allocateClassPair(contentViewClass, subclassName, 0) else { return false }

        // Override inputAccessoryView getter to return our toolbar
        let selector = #selector(getter: UIResponder.inputAccessoryView)
        guard let method = class_getInstanceMethod(UIView.self, selector),
              let typeEncoding = method_getTypeEncoding(method) else { return false }

        let block: @convention(block) (AnyObject) -> UIView? = { obj in
            let focused = objc_getAssociatedObject(obj, &EditorToolbarPlugin.associatedEditorFocusedKey) as? Bool ?? false
            guard focused else { return nil }
            return objc_getAssociatedObject(obj, &EditorToolbarPlugin.associatedToolbarKey) as? UIView
        }
        let imp = imp_implementationWithBlock(block)
        class_addMethod(subclass, selector, imp, typeEncoding)

        objc_registerClassPair(subclass)
        object_setClass(contentView, subclass)
        objc_setAssociatedObject(contentView, &associatedToolbarKey, toolbar, .OBJC_ASSOCIATION_RETAIN_NONATOMIC)
        return true
    }

    private static func findWKContentView(in webView: WKWebView) -> UIView? {
        for subview in webView.scrollView.subviews {
            if String(describing: type(of: subview)).contains("WKContentView") {
                return subview
            }
        }
        return nil
    }
}

// MARK: - Image Context Menu Interaction

extension EditorToolbarPlugin: UIContextMenuInteractionDelegate {

    func setupContextMenuInteraction(for webView: WKWebView) {
        guard let contentView = Self.findWKContentView(in: webView) else { return }
        let interaction = UIContextMenuInteraction(delegate: self)
        contentView.addInteraction(interaction)
        self.contextMenuInteraction = interaction
    }

    public func contextMenuInteraction(
        _ interaction: UIContextMenuInteraction,
        configurationForMenuAtLocation location: CGPoint
    ) -> UIContextMenuConfiguration? {
        guard let info = cachedImageInfo,
              Date().timeIntervalSince1970 - info.timestamp < 2.0 else {
            return nil
        }

        return UIContextMenuConfiguration(
            identifier: nil,
            previewProvider: { [weak self] in
                guard let img = self?.cachedImageInfo?.thumbnailImage else { return nil }
                return ImageContextPreviewController(image: img)
            },
            actionProvider: { [weak self] _ in
                self?.makeImageContextMenu(info: info)
            }
        )
    }

    public func contextMenuInteraction(
        _ interaction: UIContextMenuInteraction,
        willPerformPreviewActionForMenuWith configuration: UIContextMenuConfiguration,
        animator: UIContextMenuInteractionCommitAnimating
    ) {
        animator.addCompletion { [weak self] in
            guard let info = self?.cachedImageInfo else { return }
            self?.handleImagePreview(info: info)
        }
    }

    // MARK: Menu Builder

    private func makeImageContextMenu(info: CachedImageInfo) -> UIMenu {
        var actions: [UIMenuElement] = []

        // Preview
        actions.append(UIAction(
            title: "Preview",
            image: UIImage(systemName: "eye")
        ) { [weak self] _ in
            self?.handleImagePreview(info: info)
        })

        // Copy
        actions.append(UIAction(
            title: "Copy",
            image: UIImage(systemName: "doc.on.doc")
        ) { [weak self] _ in
            self?.handleImageCopy(info: info)
        })

        // Share
        actions.append(UIAction(
            title: "Share",
            image: UIImage(systemName: "square.and.arrow.up")
        ) { [weak self] _ in
            self?.handleImageShare(info: info)
        })

        // For HTML block images, only show Preview/Copy/Share
        if info.isHtmlBlock {
            return UIMenu(title: "", children: actions)
        }

        // Edit Alt Text (images only)
        if !info.isVideo {
            actions.append(UIAction(
                title: "Edit Alt Text",
                image: UIImage(systemName: "text.below.photo")
            ) { [weak self] _ in
                self?.handleEditAltText(info: info)
            })
        }

        // Replace
        actions.append(UIAction(
            title: "Replace",
            image: UIImage(systemName: "arrow.triangle.2.circlepath")
        ) { [weak self] _ in
            self?.handleReplace(info: info)
        })

        // Resize submenu (images only)
        if !info.isVideo {
            actions.append(makeResizeSubmenu(info: info))
        }

        // Delete (destructive)
        actions.append(UIAction(
            title: "Delete",
            image: UIImage(systemName: "trash"),
            attributes: .destructive
        ) { [weak self] _ in
            self?.handleImageDelete(info: info)
        })

        return UIMenu(title: "", children: actions)
    }

    private func makeResizeSubmenu(info: CachedImageInfo) -> UIMenu {
        let natW = info.naturalWidth ?? 0

        let presets: [(String, Double)] = [
            ("25%", 0.25), ("50%", 0.5), ("75%", 0.75), ("100% (Original)", 1.0)
        ]

        var children: [UIMenuElement] = presets.map { (label, factor) in
            UIAction(title: label) { [weak self] _ in
                if factor == 1.0 {
                    self?.applyImageResize(nodePos: info.nodePos, width: nil, height: nil)
                } else if natW > 0 {
                    self?.applyImageResize(nodePos: info.nodePos, width: Int(Double(natW) * factor), height: nil)
                }
            }
        }

        children.append(UIAction(title: "Custom\u{2026}") { [weak self] _ in
            self?.handleResizeCustom(info: info)
        })

        return UIMenu(
            title: "Resize",
            image: UIImage(systemName: "arrow.up.left.and.arrow.down.right"),
            children: children
        )
    }

    // MARK: Action Handlers

    private func handleImagePreview(info: CachedImageInfo) {
        let src = info.src.replacingOccurrences(of: "'", with: "\\'")
        webView?.evaluateJavaScript(
            "globalThis.__diaryx_nativeToolbar?.triggerPreviewMedia?.('\(src)');",
            completionHandler: nil
        )
    }

    /// Fetch full image data from a blob/http URL via JS, then call the completion handler.
    private func fetchImageData(src: String, completion: @escaping (Data?) -> Void) {
        guard let webView = webView else { completion(nil); return }
        let escaped = src.replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "\"", with: "\\\"")
        // First try canvas (works for same-origin images)
        let js = """
        (function() {
          var img = document.querySelector('img[src="\(escaped)"]');
          if (!img) return null;
          try {
            var c = document.createElement('canvas');
            c.width = img.naturalWidth; c.height = img.naturalHeight;
            var ctx = c.getContext('2d');
            ctx.drawImage(img, 0, 0);
            return c.toDataURL('image/png').split(',')[1];
          } catch(e) {
            return null;
          }
        })()
        """
        webView.evaluateJavaScript(js) { [weak self] result, _ in
            if let b64 = result as? String, let data = Data(base64Encoded: b64) {
                completion(data)
                return
            }
            // Canvas was tainted — try fetching blob URL via message handler
            self?.fetchImageDataViaBlob(src: src, completion: completion)
        }
    }

    /// Fallback: fetch blob URL, read as data URL, and post result back via message handler.
    private func fetchImageDataViaBlob(src: String, completion: @escaping (Data?) -> Void) {
        guard let webView = webView else { completion(nil); return }
        let escaped = src.replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "'", with: "\\'")

        // Store completion for the callback
        pendingImageDataCompletion = completion

        let js = """
        (function() {
          var src = '\(escaped)';
          if (!src.startsWith('blob:')) {
            window.webkit.messageHandlers.editorToolbar.postMessage({type:'imageDataResult',base64:null});
            return;
          }
          fetch(src).then(function(r){return r.blob();}).then(function(b){
            var reader = new FileReader();
            reader.onloadend = function(){
              var b64 = reader.result.split(',')[1] || null;
              window.webkit.messageHandlers.editorToolbar.postMessage({type:'imageDataResult',base64:b64});
            };
            reader.readAsDataURL(b);
          }).catch(function(){
            window.webkit.messageHandlers.editorToolbar.postMessage({type:'imageDataResult',base64:null});
          });
        })();
        """
        webView.evaluateJavaScript(js, completionHandler: nil)

        // Timeout after 5 seconds
        DispatchQueue.main.asyncAfter(deadline: .now() + 5) { [weak self] in
            if let pending = self?.pendingImageDataCompletion {
                self?.pendingImageDataCompletion = nil
                pending(nil)
            }
        }
    }

    private func handleImageCopy(info: CachedImageInfo) {
        fetchImageData(src: info.src) { data in
            guard let data = data, let image = UIImage(data: data) else { return }
            UIPasteboard.general.image = image
        }
    }

    private func handleImageShare(info: CachedImageInfo) {
        fetchImageData(src: info.src) { [weak self] data in
            guard let data = data, let image = UIImage(data: data) else { return }
            guard let vc = self?.toolbar?.findViewController() else { return }
            let activityVC = UIActivityViewController(activityItems: [image], applicationActivities: nil)
            if let popover = activityVC.popoverPresentationController {
                popover.sourceView = vc.view
                popover.sourceRect = CGRect(x: vc.view.bounds.midX, y: vc.view.bounds.midY, width: 0, height: 0)
                popover.permittedArrowDirections = []
            }
            vc.present(activityVC, animated: true)
        }
    }

    private func handleEditAltText(info: CachedImageInfo) {
        guard let vc = toolbar?.findViewController() else { return }
        let alert = UIAlertController(title: nil, message: "Alt text:", preferredStyle: .alert)
        alert.addTextField { tf in tf.text = info.alt }
        alert.addAction(UIAlertAction(title: "Cancel", style: .cancel))
        alert.addAction(UIAlertAction(title: "OK", style: .default) { [weak self] _ in
            guard let newAlt = alert.textFields?.first?.text else { return }
            let escaped = newAlt.replacingOccurrences(of: "\\", with: "\\\\")
                .replacingOccurrences(of: "'", with: "\\'")
                .replacingOccurrences(of: "\n", with: "\\n")
            let js = """
            (function() {
              var e = globalThis.__diaryx_tiptapEditor;
              if (!e) return;
              var pos = \(info.nodePos);
              var node = e.state.doc.nodeAt(pos);
              if (!node) return;
              e.chain().focus().command(function(c) {
                c.tr.setNodeMarkup(pos, undefined, Object.assign({}, node.attrs, { alt: '\(escaped)' }));
                return true;
              }).run();
            })();
            """
            self?.webView?.evaluateJavaScript(js, completionHandler: nil)
        })
        vc.present(alert, animated: true)
    }

    private func handleReplace(info: CachedImageInfo) {
        let accept = info.isVideo ? "video/*" : "image/*,video/*"
        let js = """
        (function() {
          var input = document.createElement('input');
          input.type = 'file';
          input.accept = '\(accept)';
          input.onchange = function() {
            var file = input.files && input.files[0];
            if (!file) return;
            var e = globalThis.__diaryx_tiptapEditor;
            if (!e) return;
            var pos = \(info.nodePos);
            var node = e.state.doc.nodeAt(pos);
            if (!node) return;
            // Trigger the editor's onFileDrop flow by dispatching a synthetic event
            // Instead, use the blob URL approach directly:
            var url = URL.createObjectURL(file);
            e.chain().focus().command(function(c) {
              c.tr.setNodeMarkup(pos, undefined, Object.assign({}, node.attrs, { src: url }));
              return true;
            }).run();
          };
          input.click();
        })();
        """
        webView?.evaluateJavaScript(js, completionHandler: nil)
    }

    private func handleImageDelete(info: CachedImageInfo) {
        let js = """
        (function() {
          var e = globalThis.__diaryx_tiptapEditor;
          if (!e) return;
          var pos = \(info.nodePos);
          var node = e.state.doc.nodeAt(pos);
          if (!node) return;
          e.chain().focus().command(function(c) {
            c.tr.delete(pos, pos + node.nodeSize);
            return true;
          }).run();
        })();
        """
        webView?.evaluateJavaScript(js, completionHandler: nil)
    }

    private func applyImageResize(nodePos: Int, width: Int?, height: Int?) {
        let wStr = width != nil ? String(width!) : "null"
        let hStr = height != nil ? String(height!) : "null"
        let js = """
        (function() {
          var e = globalThis.__diaryx_tiptapEditor;
          if (!e) return;
          var pos = \(nodePos);
          var node = e.state.doc.nodeAt(pos);
          if (!node) return;
          e.chain().focus().command(function(c) {
            c.tr.setNodeMarkup(pos, undefined, Object.assign({}, node.attrs, { width: \(wStr), height: \(hStr) }));
            return true;
          }).run();
        })();
        """
        webView?.evaluateJavaScript(js, completionHandler: nil)
    }

    private func handleResizeCustom(info: CachedImageInfo) {
        guard let vc = toolbar?.findViewController() else { return }
        let currentW = info.width ?? info.naturalWidth ?? 0
        let alert = UIAlertController(title: nil, message: "Enter size (width or widthxheight):", preferredStyle: .alert)
        alert.addTextField { tf in tf.text = currentW > 0 ? String(currentW) : "" }
        alert.addAction(UIAlertAction(title: "Cancel", style: .cancel))
        alert.addAction(UIAlertAction(title: "OK", style: .default) { [weak self] _ in
            guard let input = alert.textFields?.first?.text?.trimmingCharacters(in: .whitespacesAndNewlines),
                  !input.isEmpty else { return }
            // Parse WIDTHxHEIGHT or just WIDTH
            let parts = input.split(separator: "x").compactMap { Int($0) }
            guard let w = parts.first else { return }
            let h = parts.count > 1 ? parts[1] : nil
            self?.applyImageResize(nodePos: info.nodePos, width: w, height: h)
        })
        vc.present(alert, animated: true)
    }
}

// MARK: - Image Context Preview Controller

/// Lightweight view controller used for the UIContextMenuInteraction peek preview.
private class ImageContextPreviewController: UIViewController {
    private let image: UIImage

    init(image: UIImage) {
        self.image = image
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError() }

    override func viewDidLoad() {
        super.viewDidLoad()
        let imageView = UIImageView(image: image)
        imageView.contentMode = .scaleAspectFit
        imageView.clipsToBounds = true
        view = imageView

        // Size the preview to match the image aspect ratio
        let maxWidth: CGFloat = 300
        let maxHeight: CGFloat = 400
        let aspect = image.size.width / max(image.size.height, 1)
        var w = min(image.size.width, maxWidth)
        var h = w / aspect
        if h > maxHeight {
            h = maxHeight
            w = h * aspect
        }
        preferredContentSize = CGSize(width: w, height: h)
    }
}

// MARK: - Injected JavaScript

extension EditorToolbarPlugin {
    static let stateReportingScript = """
    (function() {
        var currentEditor = null;
        var pollInterval = null;
        var pluginMarkIds = [];
        var pluginCommandsSent = false;

        function reportState() {
            var editor = currentEditor;
            if (!editor) return;

            try {
                var activeStates = {
                    bold: editor.isActive('bold'),
                    italic: editor.isActive('italic'),
                    strike: editor.isActive('strike'),
                    code: editor.isActive('code'),
                    heading1: editor.isActive('heading', {level: 1}),
                    heading2: editor.isActive('heading', {level: 2}),
                    heading3: editor.isActive('heading', {level: 3}),
                    bulletList: editor.isActive('bulletList'),
                    orderedList: editor.isActive('orderedList'),
                    taskList: editor.isActive('taskList'),
                    blockquote: editor.isActive('blockquote'),
                    codeBlock: editor.isActive('codeBlock'),
                    link: editor.isActive('link')
                };

                for (var i = 0; i < pluginMarkIds.length; i++) {
                    activeStates[pluginMarkIds[i]] = editor.isActive(pluginMarkIds[i]);
                }

                var msg = {
                    type: 'stateUpdate',
                    activeStates: activeStates,
                    canUndo: editor.can().undo(),
                    canRedo: editor.can().redo(),
                    editable: editor.isEditable
                };
                window.webkit.messageHandlers.editorToolbar.postMessage(msg);
            } catch (e) {
                // Editor may have been destroyed — will re-attach on next poll
                currentEditor = null;
            }
        }

        function reportPluginCommands() {
            try {
                var bridge = globalThis.__diaryx_nativeToolbar;
                if (!bridge || !bridge.getPluginCommands) return;
                var commands = bridge.getPluginCommands();
                if (!commands) return;

                pluginMarkIds = (commands.marks || []).map(function(c) { return c.extensionId; })
                    .concat((commands.toolbarMarks || []).map(function(c) { return c.extensionId; }));

                window.webkit.messageHandlers.editorToolbar.postMessage({
                    type: 'pluginCommands',
                    commands: commands
                });
                pluginCommandsSent = true;
            } catch (e) {
                // Plugin commands not available yet
            }
        }

        function attachToEditor(editor) {
            currentEditor = editor;
            editor.on('selectionUpdate', reportState);
            editor.on('transaction', reportState);
            editor.on('focus', function() {
                window.webkit.messageHandlers.editorToolbar.postMessage({
                    type: 'focusChange', focused: true
                });
                reportState();
            });
            editor.on('blur', function() {
                window.webkit.messageHandlers.editorToolbar.postMessage({
                    type: 'focusChange', focused: false
                });
            });

            if (editor.isFocused) {
                window.webkit.messageHandlers.editorToolbar.postMessage({
                    type: 'focusChange', focused: true
                });
            }

            reportPluginCommands();
            reportState();
        }

        function poll() {
            var editor = globalThis.__diaryx_tiptapEditor;
            if (!editor) return;

            // Editor instance changed (e.g. switched entries) — re-attach
            if (editor !== currentEditor) {
                pluginCommandsSent = false;
                attachToEditor(editor);
            }

            // Retry plugin commands if bridge wasn't ready on first attach
            if (!pluginCommandsSent) {
                reportPluginCommands();
            }
        }

        // Poll continuously to handle editor recreation across entry switches
        pollInterval = setInterval(poll, 200);

        // Initial check
        poll();
    })();
    """
}

// MARK: - Weak Script Message Handler

/// Prevents retain cycle: WKUserContentController -> handler -> plugin -> webview
private class WeakScriptMessageHandler: NSObject, WKScriptMessageHandler {
    weak var delegate: WKScriptMessageHandler?

    init(delegate: WKScriptMessageHandler) {
        self.delegate = delegate
    }

    func userContentController(
        _ userContentController: WKUserContentController,
        didReceive message: WKScriptMessage
    ) {
        delegate?.userContentController(userContentController, didReceive: message)
    }
}

// MARK: - Editor Toolbar

/// Custom scrollable toolbar that serves as the WKWebView's inputAccessoryView.
/// Uses a horizontal UIScrollView with grouped UIButtons instead of UIToolbar
/// (which doesn't scroll). The background uses blur material on iOS 15-25 and
/// Liquid Glass on iOS 26+.
class EditorToolbar: UIView {
    weak var webView: WKWebView?
    private let haptics = UIImpactFeedbackGenerator(style: .light)

    private let scrollView = UIScrollView()
    private let stackView = UIStackView()
    private let dismissButton = UIButton(type: .system)

    // Button references for active state updates (keyed by state ID)
    private var buttonMap: [String: UIButton] = [:]

    // Plugin commands
    private var pluginCommands: [PluginCommand] = []
    private var pluginButtonViews: [UIView] = []
    private var pluginButtonKeys: [String] = []

    // Last known active states for block picker menu checkmarks
    private var lastActiveStates: [String: Bool] = [:]

    private static let toolbarHeight: CGFloat = 44

    override var intrinsicContentSize: CGSize {
        CGSize(width: UIView.noIntrinsicMetric, height: Self.toolbarHeight)
    }

    init(webView: WKWebView) {
        self.webView = webView
        super.init(frame: CGRect(x: 0, y: 0, width: UIScreen.main.bounds.width, height: Self.toolbarHeight))
        autoresizingMask = .flexibleWidth
        haptics.prepare()
        setupBackground()
        setupScrollView()
        buildButtons()
        setupDismissButton()
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) not implemented") }

    // MARK: - Background

    private func setupBackground() {
        let effect: UIVisualEffect
        if #available(iOS 26, *) {
            effect = UIGlassEffect()
        } else {
            effect = UIBlurEffect(style: .systemChromeMaterial)
        }
        let blurView = UIVisualEffectView(effect: effect)
        blurView.frame = bounds
        blurView.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        addSubview(blurView)

        // Top separator line
        let separator = UIView()
        separator.backgroundColor = UIColor.separator
        separator.translatesAutoresizingMaskIntoConstraints = false
        addSubview(separator)
        NSLayoutConstraint.activate([
            separator.topAnchor.constraint(equalTo: topAnchor),
            separator.leadingAnchor.constraint(equalTo: leadingAnchor),
            separator.trailingAnchor.constraint(equalTo: trailingAnchor),
            separator.heightAnchor.constraint(equalToConstant: 1.0 / UIScreen.main.scale),
        ])
    }

    // MARK: - Scroll View

    private func setupScrollView() {
        scrollView.showsHorizontalScrollIndicator = false
        scrollView.showsVerticalScrollIndicator = false
        scrollView.alwaysBounceHorizontal = true
        scrollView.translatesAutoresizingMaskIntoConstraints = false

        stackView.axis = .horizontal
        stackView.alignment = .center
        stackView.spacing = 2
        stackView.translatesAutoresizingMaskIntoConstraints = false

        addSubview(scrollView)
        scrollView.addSubview(stackView)

        NSLayoutConstraint.activate([
            // Scroll view: leave room on the right for the pinned dismiss button
            scrollView.topAnchor.constraint(equalTo: topAnchor),
            scrollView.bottomAnchor.constraint(equalTo: bottomAnchor),
            scrollView.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 4),
            scrollView.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -48),

            // Stack view fills scroll view content
            stackView.topAnchor.constraint(equalTo: scrollView.contentLayoutGuide.topAnchor),
            stackView.bottomAnchor.constraint(equalTo: scrollView.contentLayoutGuide.bottomAnchor),
            stackView.leadingAnchor.constraint(equalTo: scrollView.contentLayoutGuide.leadingAnchor, constant: 4),
            stackView.trailingAnchor.constraint(equalTo: scrollView.contentLayoutGuide.trailingAnchor, constant: -4),
            stackView.heightAnchor.constraint(equalTo: scrollView.frameLayoutGuide.heightAnchor),
        ])
    }

    // MARK: - Dismiss Button (pinned right)

    private func setupDismissButton() {
        dismissButton.setImage(UIImage(systemName: "keyboard.chevron.compact.down"), for: .normal)
        dismissButton.tintColor = .label
        dismissButton.addTarget(self, action: #selector(doDismiss), for: .touchUpInside)
        dismissButton.translatesAutoresizingMaskIntoConstraints = false
        addSubview(dismissButton)

        NSLayoutConstraint.activate([
            dismissButton.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -8),
            dismissButton.centerYAnchor.constraint(equalTo: centerYAnchor),
            dismissButton.widthAnchor.constraint(equalToConstant: 36),
            dismissButton.heightAnchor.constraint(equalToConstant: 36),
        ])
    }

    // MARK: - Build Buttons

    private func buildButtons() {
        // Block picker button (replaces FloatingMenu on iOS)
        addGroup([makeBlockPickerButton()])

        addSeparator()

        // History
        addGroup([
            makeButton(systemName: "arrow.uturn.backward", action: #selector(doUndo), id: "undo"),
            makeButton(systemName: "arrow.uturn.forward", action: #selector(doRedo), id: "redo"),
        ])

        addSeparator()

        // Inline formatting
        addGroup([
            makeButton(systemName: "bold", action: #selector(doBold), id: "bold"),
            makeButton(systemName: "italic", action: #selector(doItalic), id: "italic"),
            makeButton(systemName: "strikethrough", action: #selector(doStrike), id: "strike"),
            makeButton(systemName: "chevron.left.forwardslash.chevron.right", action: #selector(doCode), id: "code"),
            makeButton(systemName: "link", action: #selector(doLink), id: "link"),
        ])
    }

    // MARK: - Block Picker (replaces FloatingMenu)

    private var blockPickerButton: UIButton?

    private func makeBlockPickerButton() -> UIButton {
        let button = UIButton(type: .system)
        let config = UIImage.SymbolConfiguration(pointSize: 16, weight: .medium)
        button.setImage(UIImage(systemName: "paragraph", withConfiguration: config), for: .normal)
        button.tintColor = .secondaryLabel
        button.showsMenuAsPrimaryAction = true
        button.menu = buildBlockPickerMenu()
        button.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            button.widthAnchor.constraint(equalToConstant: 36),
            button.heightAnchor.constraint(equalToConstant: 36),
        ])
        blockPickerButton = button
        return button
    }

    private func buildBlockPickerMenu() -> UIMenu {
        let states = lastActiveStates

        func isOn(_ key: String) -> UIMenuElement.State {
            (states[key] ?? false) ? .on : .off
        }

        let h1 = UIAction(title: "Heading 1", state: isOn("heading1")) { [weak self] _ in
            self?.haptics.impactOccurred()
            self?.execHeading(level: 1)
        }
        let h2 = UIAction(title: "Heading 2", state: isOn("heading2")) { [weak self] _ in
            self?.haptics.impactOccurred()
            self?.execHeading(level: 2)
        }
        let h3 = UIAction(title: "Heading 3", state: isOn("heading3")) { [weak self] _ in
            self?.haptics.impactOccurred()
            self?.execHeading(level: 3)
        }
        let headings = UIMenu(title: "Heading", image: UIImage(systemName: "textformat.size"), children: [h1, h2, h3])

        let bullet = UIAction(title: "Bullet List", image: UIImage(systemName: "list.bullet"), state: isOn("bulletList")) { [weak self] _ in
            self?.haptics.impactOccurred()
            self?.execCommand("toggleBulletList")
        }
        let ordered = UIAction(title: "Numbered List", image: UIImage(systemName: "list.number"), state: isOn("orderedList")) { [weak self] _ in
            self?.haptics.impactOccurred()
            self?.execCommand("toggleOrderedList")
        }
        let task = UIAction(title: "Task List", image: UIImage(systemName: "checklist"), state: isOn("taskList")) { [weak self] _ in
            self?.haptics.impactOccurred()
            self?.execCommand("toggleTaskList")
        }
        let lists = UIMenu(title: "List", image: UIImage(systemName: "list.bullet"), children: [bullet, ordered, task])

        let blocks: [UIMenuElement] = [
            UIAction(title: "Blockquote", image: UIImage(systemName: "text.quote"), state: isOn("blockquote")) { [weak self] _ in
                self?.haptics.impactOccurred()
                self?.execCommand("toggleBlockquote")
            },
            UIAction(title: "Code Block", image: UIImage(systemName: "curlybraces"), state: isOn("codeBlock")) { [weak self] _ in
                self?.haptics.impactOccurred()
                self?.execCommand("toggleCodeBlock")
            },
            UIAction(title: "Divider", image: UIImage(systemName: "minus")) { [weak self] _ in
                self?.haptics.impactOccurred()
                self?.execCommand("setHorizontalRule")
            },
            UIAction(title: "Table", image: UIImage(systemName: "tablecells")) { [weak self] _ in
                self?.haptics.impactOccurred()
                self?.webView?.evaluateJavaScript(
                    "globalThis.__diaryx_tiptapEditor?.chain().focus().insertTable({rows:3,cols:3,withHeaderRow:true}).run();",
                    completionHandler: nil
                )
            },
            UIAction(title: "Attachment", image: UIImage(systemName: "paperclip")) { [weak self] _ in
                self?.haptics.impactOccurred()
                self?.webView?.evaluateJavaScript(
                    "globalThis.__diaryx_tiptapEditor?.commands.insertAttachmentPicker();",
                    completionHandler: nil
                )
            },
        ]

        var pluginItems: [UIMenuElement] = []
        for cmd in pluginCommands where cmd.nodeType == "blockAtom" || cmd.nodeType == "blockPickerItem" {
            let captured = cmd
            let image: UIImage? = {
                if let iconName = cmd.iconName, let sfName = Self.lucideToSFSymbol[iconName] {
                    return UIImage(systemName: sfName)
                }
                return nil
            }()
            pluginItems.append(UIAction(title: cmd.label, image: image) { [weak self] _ in
                self?.execPluginCommand(captured)
            })
        }

        var children: [UIMenuElement] = [headings, lists]
        children.append(UIMenu(title: "", options: .displayInline, children: blocks))
        if !pluginItems.isEmpty {
            children.append(UIMenu(title: "", options: .displayInline, children: pluginItems))
        }

        return UIMenu(title: "", children: children)
    }

    /// Rebuild the block picker menu to include updated plugin commands
    private func refreshBlockPickerMenu() {
        blockPickerButton?.menu = buildBlockPickerMenu()
    }

    // MARK: - Button Factories

    private func makeButton(systemName: String, action: Selector, id: String) -> UIButton {
        let button = UIButton(type: .system)
        let config = UIImage.SymbolConfiguration(pointSize: 16, weight: .medium)
        button.setImage(UIImage(systemName: systemName, withConfiguration: config), for: .normal)
        button.tintColor = .secondaryLabel
        button.addTarget(self, action: action, for: .touchUpInside)
        button.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            button.widthAnchor.constraint(equalToConstant: 36),
            button.heightAnchor.constraint(equalToConstant: 36),
        ])
        buttonMap[id] = button
        return button
    }

    private func addGroup(_ buttons: [UIButton]) {
        for button in buttons {
            stackView.addArrangedSubview(button)
        }
    }

    private func addSeparator() {
        let sep = UIView()
        sep.backgroundColor = UIColor.separator
        sep.translatesAutoresizingMaskIntoConstraints = false
        stackView.addArrangedSubview(sep)
        NSLayoutConstraint.activate([
            sep.widthAnchor.constraint(equalToConstant: 1.0 / UIScreen.main.scale),
            sep.heightAnchor.constraint(equalToConstant: 24),
        ])
        // Add spacing around separator
        stackView.setCustomSpacing(8, after: stackView.arrangedSubviews[stackView.arrangedSubviews.count - 2])
        stackView.setCustomSpacing(8, after: sep)
    }

    // MARK: - State Updates

    func updateState(activeStates: [String: Bool], canUndo: Bool, canRedo: Bool, editable: Bool) {
        isHidden = !editable

        // Track block-level states for the block picker menu checkmarks
        let blockKeys: Set<String> = [
            "heading1", "heading2", "heading3",
            "bulletList", "orderedList", "taskList",
            "blockquote", "codeBlock",
        ]
        let oldBlockStates = lastActiveStates.filter { blockKeys.contains($0.key) }
        let newBlockStates = activeStates.filter { blockKeys.contains($0.key) }
        let blockStateChanged = oldBlockStates != newBlockStates
        lastActiveStates = activeStates

        if blockStateChanged {
            refreshBlockPickerMenu()
        }

        let activeTint = tintColor ?? .systemBlue
        let inactiveTint = UIColor.secondaryLabel

        for (id, button) in buttonMap {
            switch id {
            case "undo":
                button.isEnabled = canUndo
                button.tintColor = canUndo ? activeTint : inactiveTint.withAlphaComponent(0.3)
            case "redo":
                button.isEnabled = canRedo
                button.tintColor = canRedo ? activeTint : inactiveTint.withAlphaComponent(0.3)
            default:
                let isActive = activeStates[id] ?? false
                button.tintColor = isActive ? activeTint : inactiveTint
            }
        }
    }

    // MARK: - Actions

    @objc private func doBold() {
        haptics.impactOccurred()
        execCommand("toggleBold")
    }

    @objc private func doItalic() {
        haptics.impactOccurred()
        execCommand("toggleItalic")
    }

    @objc private func doStrike() {
        haptics.impactOccurred()
        execCommand("toggleStrike")
    }

    @objc private func doCode() {
        haptics.impactOccurred()
        execCommand("toggleCode")
    }

    @objc private func doLink() {
        haptics.impactOccurred()
        promptForLink()
    }

    @objc private func doUndo() {
        haptics.impactOccurred()
        execCommand("undo")
    }

    @objc private func doRedo() {
        haptics.impactOccurred()
        execCommand("redo")
    }

    @objc private func doDismiss() {
        haptics.impactOccurred()
        webView?.resignFirstResponder()
    }

    // MARK: - JS Command Execution

    private func execCommand(_ command: String) {
        let js = "globalThis.__diaryx_tiptapEditor?.chain().focus().\(command)().run();"
        webView?.evaluateJavaScript(js, completionHandler: nil)
    }

    private func execHeading(level: Int) {
        let js = "globalThis.__diaryx_tiptapEditor?.chain().focus().toggleHeading({level:\(level)}).run();"
        webView?.evaluateJavaScript(js, completionHandler: nil)
    }

    // MARK: - Link Picker

    private func promptForLink() {
        guard let vc = findViewController(), let webView = webView else { return }

        // Check if cursor is already on a link
        let checkJs = "globalThis.__diaryx_tiptapEditor?.isActive('link') ?? false;"
        webView.evaluateJavaScript(checkJs) { [weak self] result, _ in
            let isLink = result as? Bool ?? false
            if isLink {
                self?.showLinkActionSheet(on: vc)
            } else {
                self?.presentLinkPicker(on: vc, existingHref: nil)
            }
        }
    }

    private func showLinkActionSheet(on vc: UIViewController) {
        let sheet = UIAlertController(title: "Link", message: nil, preferredStyle: .actionSheet)
        sheet.addAction(UIAlertAction(title: "Edit Link", style: .default) { [weak self] _ in
            let js = "globalThis.__diaryx_tiptapEditor?.getAttributes('link')?.href ?? '';"
            self?.webView?.evaluateJavaScript(js) { result, _ in
                let href = result as? String
                self?.presentLinkPicker(on: vc, existingHref: href)
            }
        })
        sheet.addAction(UIAlertAction(title: "Remove Link", style: .destructive) { [weak self] _ in
            self?.execCommand("unsetLink")
        })
        sheet.addAction(UIAlertAction(title: "Cancel", style: .cancel))
        vc.present(sheet, animated: true)
    }

    private func presentLinkPicker(on vc: UIViewController, existingHref: String?) {
        guard let webView = webView else { return }

        // Fetch workspace entries from JS bridge
        let entriesJs = "JSON.stringify(globalThis.__diaryx_nativeToolbar?.getEntries() ?? []);"
        webView.evaluateJavaScript(entriesJs) { result, _ in
            let entries: [[String: String]]
            if let jsonStr = result as? String,
               let data = jsonStr.data(using: .utf8),
               let parsed = try? JSONSerialization.jsonObject(with: data) as? [[String: String]] {
                entries = parsed
            } else {
                entries = []
            }

            let picker = LinkPickerViewController(
                entries: entries,
                existingHref: existingHref,
                webView: webView
            )
            let nav = UINavigationController(rootViewController: picker)
            if #available(iOS 15.0, *) {
                if let sheet = nav.sheetPresentationController {
                    sheet.detents = [.medium(), .large()]
                    sheet.prefersGrabberVisible = true
                }
            }
            vc.present(nav, animated: true)
        }
    }

    func findViewController() -> UIViewController? {
        var responder: UIResponder? = webView
        while let r = responder {
            if let vc = r as? UIViewController { return vc }
            responder = r.next
        }
        return nil
    }

    // MARK: - Plugin Commands

    func updatePluginCommands(_ dict: [String: Any]) {
        var newCommands: [PluginCommand] = []

        if let marks = dict["marks"] as? [[String: Any]] {
            for m in marks {
                guard let extId = m["extensionId"] as? String,
                      let label = m["label"] as? String else { continue }
                newCommands.append(PluginCommand(
                    extensionId: extId, label: label,
                    iconName: m["iconName"] as? String, nodeType: "mark"
                ))
            }
        }
        if let inlines = dict["inlineAtoms"] as? [[String: Any]] {
            for item in inlines {
                guard let extId = item["extensionId"] as? String,
                      let label = item["label"] as? String else { continue }
                newCommands.append(PluginCommand(
                    extensionId: extId, label: label,
                    iconName: item["iconName"] as? String, nodeType: "inlineAtom"
                ))
            }
        }
        if let blocks = dict["blockAtoms"] as? [[String: Any]] {
            for item in blocks {
                guard let extId = item["extensionId"] as? String,
                      let label = item["label"] as? String else { continue }
                newCommands.append(PluginCommand(
                    extensionId: extId, label: label,
                    iconName: item["iconName"] as? String, nodeType: "blockAtom",
                    placement: item["placement"] as? String ?? "Picker"
                ))
            }
        }
        if let bpItems = dict["blockPickerItems"] as? [[String: Any]] {
            for item in bpItems {
                guard let id = item["id"] as? String,
                      let label = item["label"] as? String,
                      let editorCommand = item["editorCommand"] as? String else { continue }
                let prompt = item["prompt"] as? [String: Any]
                newCommands.append(PluginCommand(
                    extensionId: id, label: label,
                    iconName: item["iconName"] as? String, nodeType: "blockPickerItem",
                    editorCommand: editorCommand,
                    params: item["params"] as? [String: Any],
                    promptMessage: prompt?["message"] as? String,
                    promptDefault: prompt?["defaultValue"] as? String,
                    promptParamKey: prompt?["paramKey"] as? String
                ))
            }
        }

        if let toolbarMarks = dict["toolbarMarks"] as? [[String: Any]] {
            for m in toolbarMarks {
                guard let extId = m["extensionId"] as? String,
                      let label = m["label"] as? String else { continue }
                let attr = m["attribute"] as? [String: Any]
                newCommands.append(PluginCommand(
                    extensionId: extId, label: label,
                    iconName: m["iconName"] as? String, nodeType: "toolbarMark",
                    attributeName: attr?["name"] as? String,
                    attributeDefaultValue: attr?["defaultValue"] as? String,
                    attributeValidValues: attr?["validValues"] as? [String]
                ))
            }
        }

        guard newCommands != pluginCommands else { return }
        pluginCommands = newCommands
        rebuildPluginButtons()
        refreshBlockPickerMenu()
    }

    private func rebuildPluginButtons() {
        // Remove existing plugin views from stack
        for view in pluginButtonViews {
            stackView.removeArrangedSubview(view)
            view.removeFromSuperview()
        }
        pluginButtonViews.removeAll()

        // Remove plugin entries from buttonMap
        for key in pluginButtonKeys {
            buttonMap.removeValue(forKey: key)
        }
        pluginButtonKeys.removeAll()

        // Only show standalone toolbar buttons for marks, inline atoms, and
        // blockAtom/blockPickerItem commands that explicitly request it via
        // placement == "All". Block atoms and block picker items already appear
        // in the block picker menu.
        let toolbarCommands = pluginCommands.filter { cmd in
            switch cmd.nodeType {
            case "blockAtom":
                return cmd.placement == "All"
            case "blockPickerItem":
                return false
            default:
                return true
            }
        }

        guard !toolbarCommands.isEmpty else { return }

        // Add separator before plugin buttons
        let sep = UIView()
        sep.backgroundColor = UIColor.separator
        sep.translatesAutoresizingMaskIntoConstraints = false
        stackView.addArrangedSubview(sep)
        NSLayoutConstraint.activate([
            sep.widthAnchor.constraint(equalToConstant: 1.0 / UIScreen.main.scale),
            sep.heightAnchor.constraint(equalToConstant: 24),
        ])
        if let lastBeforeSep = stackView.arrangedSubviews.dropLast().last {
            stackView.setCustomSpacing(8, after: lastBeforeSep)
        }
        stackView.setCustomSpacing(8, after: sep)
        pluginButtonViews.append(sep)

        for cmd in toolbarCommands {
            let button: UIButton
            if let sfName = Self.lucideToSFSymbol[cmd.iconName ?? ""] {
                button = UIButton(type: .system)
                let config = UIImage.SymbolConfiguration(pointSize: 16, weight: .medium)
                button.setImage(UIImage(systemName: sfName, withConfiguration: config), for: .normal)
            } else {
                // Text label fallback
                button = UIButton(type: .system)
                let truncated = String(cmd.label.prefix(3))
                button.setTitle(truncated, for: .normal)
                button.titleLabel?.font = .systemFont(ofSize: 15, weight: .semibold)
            }
            button.tintColor = .secondaryLabel
            button.accessibilityLabel = cmd.label
            button.translatesAutoresizingMaskIntoConstraints = false
            NSLayoutConstraint.activate([
                button.widthAnchor.constraint(equalToConstant: 36),
                button.heightAnchor.constraint(equalToConstant: 36),
            ])

            let captured = cmd
            if cmd.nodeType == "toolbarMark", let values = cmd.attributeValidValues, !values.isEmpty {
                // Toolbar mark with attribute picker: tap toggles default, long-press shows menu
                let attrName = cmd.attributeName ?? "value"
                let defaultVal = cmd.attributeDefaultValue ?? values.first ?? ""

                button.addAction(UIAction { [weak self] _ in
                    self?.haptics.impactOccurred()
                    let js = "globalThis.__diaryx_tiptapEditor?.chain().focus().toggleMark('\(cmd.extensionId)',{\(attrName):'\(defaultVal)'}).run();"
                    self?.webView?.evaluateJavaScript(js, completionHandler: nil)
                }, for: .touchUpInside)

                button.showsMenuAsPrimaryAction = false
                let valueActions = values.map { value in
                    UIAction(title: value.prefix(1).uppercased() + value.dropFirst()) { [weak self] _ in
                        self?.haptics.impactOccurred()
                        let js = "globalThis.__diaryx_tiptapEditor?.chain().focus().toggleMark('\(cmd.extensionId)',{\(attrName):'\(value)'}).run();"
                        self?.webView?.evaluateJavaScript(js, completionHandler: nil)
                    }
                }
                let valuesMenu = UIMenu(title: "", options: .displayInline, children: valueActions)
                let remove = UIAction(title: "Remove \(cmd.label)", image: UIImage(systemName: "xmark"), attributes: .destructive) { [weak self] _ in
                    self?.haptics.impactOccurred()
                    let js = "globalThis.__diaryx_tiptapEditor?.chain().focus().unsetMark('\(cmd.extensionId)').run();"
                    self?.webView?.evaluateJavaScript(js, completionHandler: nil)
                }
                let removeMenu = UIMenu(title: "", options: .displayInline, children: [remove])
                button.menu = UIMenu(title: cmd.label, children: [valuesMenu, removeMenu])
            } else {
                button.addAction(UIAction { [weak self] _ in
                    self?.execPluginCommand(captured)
                }, for: .touchUpInside)
            }

            stackView.addArrangedSubview(button)
            buttonMap[cmd.extensionId] = button
            pluginButtonKeys.append(cmd.extensionId)
            pluginButtonViews.append(button)
        }
    }

    private func execPluginCommand(_ cmd: PluginCommand) {
        haptics.impactOccurred()

        let js: String
        switch cmd.nodeType {
        case "mark":
            js = "globalThis.__diaryx_tiptapEditor?.chain().focus().toggleMark('\(cmd.extensionId)').run();"
        case "toolbarMark":
            let attrName = cmd.attributeName ?? "value"
            let defaultVal = cmd.attributeDefaultValue ?? ""
            js = "globalThis.__diaryx_tiptapEditor?.chain().focus().toggleMark('\(cmd.extensionId)',{\(attrName):'\(defaultVal)'}).run();"
        case "blockPickerItem":
            if let promptMsg = cmd.promptMessage, let paramKey = cmd.promptParamKey {
                promptUserInput(message: promptMsg, defaultValue: cmd.promptDefault ?? "") { [weak self] input in
                    guard let input = input else { return }
                    var params = cmd.params ?? [:]
                    params[paramKey] = input
                    let paramsJson = Self.jsonString(from: params)
                    let execJs = "globalThis.__diaryx_tiptapEditor?.chain().focus().\(cmd.editorCommand!)(\(paramsJson)).run();"
                    self?.webView?.evaluateJavaScript(execJs, completionHandler: nil)
                }
                return
            }
            let paramsJson = Self.jsonString(from: cmd.params ?? [:])
            js = "globalThis.__diaryx_tiptapEditor?.chain().focus().\(cmd.editorCommand ?? "insertContent")(\(paramsJson)).run();"
        default: // inlineAtom, blockAtom
            js = "globalThis.__diaryx_tiptapEditor?.chain().focus().insertContent({type:'\(cmd.extensionId)',attrs:{source:''}}).run();"
        }
        webView?.evaluateJavaScript(js, completionHandler: nil)
    }

    private func promptUserInput(message: String, defaultValue: String, completion: @escaping (String?) -> Void) {
        guard let vc = findViewController() else { completion(nil); return }
        let alert = UIAlertController(title: nil, message: message, preferredStyle: .alert)
        alert.addTextField { tf in tf.text = defaultValue }
        alert.addAction(UIAlertAction(title: "Cancel", style: .cancel) { _ in completion(nil) })
        alert.addAction(UIAlertAction(title: "Insert", style: .default) { _ in
            completion(alert.textFields?.first?.text?.trimmingCharacters(in: .whitespacesAndNewlines))
        })
        vc.present(alert, animated: true)
    }

    private static func jsonString(from dict: [String: Any]) -> String {
        guard let data = try? JSONSerialization.data(withJSONObject: dict),
              let str = String(data: data, encoding: .utf8) else {
            return "{}"
        }
        return str
    }

    // MARK: - Lucide → SF Symbol Mapping

    private static let lucideToSFSymbol: [String: String] = [
        "eye-off": "eye.slash",
        "eye": "eye",
        "sigma": "sum",
        "square-sigma": "sum",
        "lock": "lock",
        "unlock": "lock.open",
        "hash": "number",
        "code": "chevron.left.forwardslash.chevron.right",
        "type": "textformat",
        "at-sign": "at",
        "star": "star",
        "heart": "heart",
        "bookmark": "bookmark",
        "tag": "tag",
        "puzzle": "puzzlepiece",
        "highlighter": "highlighter",
    ]
}

// MARK: - Plugin Command

struct PluginCommand: Equatable {
    let extensionId: String
    let label: String
    let iconName: String?
    let nodeType: String  // "mark", "inlineAtom", "blockAtom", "blockPickerItem"
    var placement: String? = nil  // "Picker", "PickerAndStylePicker", "All"
    var editorCommand: String? = nil
    var params: [String: Any]? = nil
    var promptMessage: String? = nil
    var promptDefault: String? = nil
    var promptParamKey: String? = nil
    // Toolbar mark attribute fields
    var attributeName: String? = nil
    var attributeDefaultValue: String? = nil
    var attributeValidValues: [String]? = nil

    static func == (lhs: PluginCommand, rhs: PluginCommand) -> Bool {
        lhs.extensionId == rhs.extensionId &&
        lhs.label == rhs.label &&
        lhs.iconName == rhs.iconName &&
        lhs.nodeType == rhs.nodeType &&
        lhs.placement == rhs.placement &&
        lhs.editorCommand == rhs.editorCommand &&
        lhs.promptMessage == rhs.promptMessage &&
        lhs.promptDefault == rhs.promptDefault &&
        lhs.promptParamKey == rhs.promptParamKey &&
        lhs.attributeName == rhs.attributeName &&
        lhs.attributeDefaultValue == rhs.attributeDefaultValue &&
        lhs.attributeValidValues == rhs.attributeValidValues
    }
}

// MARK: - Link Picker View Controller

/// Native link picker with two tabs: Remote (URL input) and Local (workspace file picker).
/// Presented as a half-sheet from the toolbar's link button.
class LinkPickerViewController: UIViewController, UITableViewDataSource, UITableViewDelegate, UISearchBarDelegate, UITextFieldDelegate {

    struct Entry {
        let path: String
        let name: String
        let displayPath: String
    }

    private let allEntries: [Entry]
    private var filteredEntries: [Entry]
    private let existingHref: String?
    private weak var webView: WKWebView?

    private let segmentedControl = UISegmentedControl(items: ["Remote", "Local"])
    private let remoteContainer = UIView()
    private let localContainer = UIView()
    private let urlTextField = UITextField()
    private let insertButton = UIButton(type: .system)
    private let searchBar = UISearchBar()
    private let tableView = UITableView(frame: .zero, style: .insetGrouped)

    init(entries: [[String: String]], existingHref: String?, webView: WKWebView) {
        self.allEntries = entries.compactMap { dict in
            guard let path = dict["path"], let name = dict["name"] else { return nil }
            return Entry(path: path, name: name, displayPath: dict["displayPath"] ?? path)
        }
        self.filteredEntries = self.allEntries
        self.existingHref = existingHref
        self.webView = webView
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) not implemented") }

    override func viewDidLoad() {
        super.viewDidLoad()

        title = "Insert Link"
        navigationItem.leftBarButtonItem = UIBarButtonItem(
            barButtonSystemItem: .cancel,
            target: self,
            action: #selector(cancelTapped)
        )

        view.backgroundColor = .systemGroupedBackground

        setupSegmentedControl()
        setupRemoteTab()
        setupLocalTab()

        // Default to Remote tab, or Local if there are entries and no existing href
        if existingHref != nil || allEntries.isEmpty {
            segmentedControl.selectedSegmentIndex = 0
            showTab(index: 0)
        } else {
            segmentedControl.selectedSegmentIndex = 0
            showTab(index: 0)
        }
    }

    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        // Auto-focus the URL field on Remote tab
        if segmentedControl.selectedSegmentIndex == 0 {
            urlTextField.becomeFirstResponder()
        } else {
            searchBar.becomeFirstResponder()
        }
    }

    // MARK: - Segmented Control

    private func setupSegmentedControl() {
        segmentedControl.translatesAutoresizingMaskIntoConstraints = false
        segmentedControl.addTarget(self, action: #selector(segmentChanged), for: .valueChanged)
        view.addSubview(segmentedControl)

        NSLayoutConstraint.activate([
            segmentedControl.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor, constant: 12),
            segmentedControl.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 16),
            segmentedControl.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -16),
        ])
    }

    @objc private func segmentChanged() {
        showTab(index: segmentedControl.selectedSegmentIndex)
    }

    private func showTab(index: Int) {
        remoteContainer.isHidden = index != 0
        localContainer.isHidden = index != 1

        if index == 0 {
            urlTextField.becomeFirstResponder()
        } else {
            searchBar.becomeFirstResponder()
        }
    }

    // MARK: - Remote Tab

    private func setupRemoteTab() {
        remoteContainer.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(remoteContainer)

        NSLayoutConstraint.activate([
            remoteContainer.topAnchor.constraint(equalTo: segmentedControl.bottomAnchor, constant: 16),
            remoteContainer.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            remoteContainer.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            remoteContainer.bottomAnchor.constraint(equalTo: view.bottomAnchor),
        ])

        // URL text field
        urlTextField.placeholder = "https://example.com"
        urlTextField.borderStyle = .roundedRect
        urlTextField.keyboardType = .URL
        urlTextField.autocapitalizationType = .none
        urlTextField.autocorrectionType = .no
        urlTextField.returnKeyType = .done
        urlTextField.clearButtonMode = .whileEditing
        urlTextField.delegate = self
        urlTextField.translatesAutoresizingMaskIntoConstraints = false
        remoteContainer.addSubview(urlTextField)

        if let href = existingHref, !href.isEmpty {
            urlTextField.text = href
        }

        // Insert button
        insertButton.setTitle("Insert Link", for: .normal)
        insertButton.titleLabel?.font = .systemFont(ofSize: 17, weight: .semibold)
        insertButton.backgroundColor = .systemBlue
        insertButton.setTitleColor(.white, for: .normal)
        insertButton.layer.cornerRadius = 10
        insertButton.addTarget(self, action: #selector(insertRemoteTapped), for: .touchUpInside)
        insertButton.translatesAutoresizingMaskIntoConstraints = false
        remoteContainer.addSubview(insertButton)

        NSLayoutConstraint.activate([
            urlTextField.topAnchor.constraint(equalTo: remoteContainer.topAnchor, constant: 8),
            urlTextField.leadingAnchor.constraint(equalTo: remoteContainer.leadingAnchor, constant: 16),
            urlTextField.trailingAnchor.constraint(equalTo: remoteContainer.trailingAnchor, constant: -16),
            urlTextField.heightAnchor.constraint(equalToConstant: 44),

            insertButton.topAnchor.constraint(equalTo: urlTextField.bottomAnchor, constant: 16),
            insertButton.leadingAnchor.constraint(equalTo: remoteContainer.leadingAnchor, constant: 16),
            insertButton.trailingAnchor.constraint(equalTo: remoteContainer.trailingAnchor, constant: -16),
            insertButton.heightAnchor.constraint(equalToConstant: 50),
        ])
    }

    // MARK: - Local Tab

    private func setupLocalTab() {
        localContainer.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(localContainer)

        NSLayoutConstraint.activate([
            localContainer.topAnchor.constraint(equalTo: segmentedControl.bottomAnchor, constant: 12),
            localContainer.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            localContainer.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            localContainer.bottomAnchor.constraint(equalTo: view.bottomAnchor),
        ])

        // Search bar
        searchBar.placeholder = "Search files..."
        searchBar.delegate = self
        searchBar.searchBarStyle = .minimal
        searchBar.translatesAutoresizingMaskIntoConstraints = false
        localContainer.addSubview(searchBar)

        // Table view
        tableView.dataSource = self
        tableView.delegate = self
        tableView.register(UITableViewCell.self, forCellReuseIdentifier: "EntryCell")
        tableView.keyboardDismissMode = .onDrag
        tableView.translatesAutoresizingMaskIntoConstraints = false
        localContainer.addSubview(tableView)

        NSLayoutConstraint.activate([
            searchBar.topAnchor.constraint(equalTo: localContainer.topAnchor),
            searchBar.leadingAnchor.constraint(equalTo: localContainer.leadingAnchor),
            searchBar.trailingAnchor.constraint(equalTo: localContainer.trailingAnchor),

            tableView.topAnchor.constraint(equalTo: searchBar.bottomAnchor),
            tableView.leadingAnchor.constraint(equalTo: localContainer.leadingAnchor),
            tableView.trailingAnchor.constraint(equalTo: localContainer.trailingAnchor),
            tableView.bottomAnchor.constraint(equalTo: localContainer.bottomAnchor),
        ])
    }

    // MARK: - Actions

    @objc private func cancelTapped() {
        dismiss(animated: true)
        // Re-focus editor
        webView?.evaluateJavaScript("globalThis.__diaryx_tiptapEditor?.commands.focus();", completionHandler: nil)
    }

    @objc private func insertRemoteTapped() {
        insertRemoteLink()
    }

    private func insertRemoteLink() {
        guard var href = urlTextField.text?.trimmingCharacters(in: .whitespacesAndNewlines),
              !href.isEmpty else { return }

        // Auto-add https:// if no scheme
        if !href.contains("://") {
            href = "https://\(href)"
        }

        let escaped = href.replacingOccurrences(of: "'", with: "\\'")
        let js = "globalThis.__diaryx_nativeToolbar?.insertRemoteLink('\(escaped)');"
        webView?.evaluateJavaScript(js, completionHandler: nil)

        dismiss(animated: true)
    }

    // MARK: - UITextFieldDelegate

    func textFieldShouldReturn(_ textField: UITextField) -> Bool {
        insertRemoteLink()
        return true
    }

    // MARK: - UISearchBarDelegate

    func searchBar(_ searchBar: UISearchBar, textDidChange searchText: String) {
        if searchText.isEmpty {
            filteredEntries = allEntries
        } else {
            let query = searchText.lowercased()
            filteredEntries = allEntries.filter { entry in
                entry.name.lowercased().contains(query) ||
                entry.displayPath.lowercased().contains(query)
            }
        }
        tableView.reloadData()
    }

    // MARK: - UITableViewDataSource

    func tableView(_ tableView: UITableView, numberOfRowsInSection section: Int) -> Int {
        return filteredEntries.count
    }

    func tableView(_ tableView: UITableView, cellForRowAt indexPath: IndexPath) -> UITableViewCell {
        let cell = tableView.dequeueReusableCell(withIdentifier: "EntryCell", for: indexPath)
        let entry = filteredEntries[indexPath.row]

        var config = cell.defaultContentConfiguration()
        config.text = entry.name
        config.secondaryText = entry.displayPath
        config.secondaryTextProperties.color = .secondaryLabel
        config.secondaryTextProperties.font = .systemFont(ofSize: 12)
        config.image = UIImage(systemName: "doc.text")
        cell.contentConfiguration = config
        cell.accessoryType = .disclosureIndicator

        return cell
    }

    // MARK: - UITableViewDelegate

    func tableView(_ tableView: UITableView, didSelectRowAt indexPath: IndexPath) {
        tableView.deselectRow(at: indexPath, animated: true)

        let entry = filteredEntries[indexPath.row]
        let escapedPath = entry.path.replacingOccurrences(of: "'", with: "\\'")
        let escapedName = entry.name.replacingOccurrences(of: "'", with: "\\'")
        let js = "globalThis.__diaryx_nativeToolbar?.insertLocalLink('\(escapedPath)', '\(escapedName)');"
        webView?.evaluateJavaScript(js, completionHandler: nil)

        dismiss(animated: true)
    }
}

// MARK: - Native Image Preview

/// Full-screen image preview with native pinch-to-zoom, double-tap zoom, and pan.
/// Uses UIScrollView for smooth, native-feeling gesture handling.
class ImagePreviewViewController: UIViewController, UIScrollViewDelegate {
    private let image: UIImage
    private let imageName: String

    private let scrollView = UIScrollView()
    private let imageView = UIImageView()
    private let closeButton = UIButton(type: .system)
    private let titleLabel = UILabel()
    private let headerGradient = CAGradientLayer()

    init(image: UIImage, name: String) {
        self.image = image
        self.imageName = name
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) not implemented") }

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .black

        setupScrollView()
        setupImageView()
        setupHeader()
        setupGestures()
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        headerGradient.frame = CGRect(x: 0, y: 0, width: view.bounds.width, height: view.safeAreaInsets.top + 56)
        updateZoomScale()
    }

    override var prefersStatusBarHidden: Bool { true }
    override var prefersHomeIndicatorAutoHidden: Bool { true }

    // MARK: - Scroll View

    private func setupScrollView() {
        scrollView.delegate = self
        scrollView.showsHorizontalScrollIndicator = false
        scrollView.showsVerticalScrollIndicator = false
        scrollView.bouncesZoom = true
        scrollView.decelerationRate = .fast
        scrollView.contentInsetAdjustmentBehavior = .never
        scrollView.translatesAutoresizingMaskIntoConstraints = false

        view.addSubview(scrollView)
        NSLayoutConstraint.activate([
            scrollView.topAnchor.constraint(equalTo: view.topAnchor),
            scrollView.bottomAnchor.constraint(equalTo: view.bottomAnchor),
            scrollView.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            scrollView.trailingAnchor.constraint(equalTo: view.trailingAnchor),
        ])
    }

    // MARK: - Image View

    private func setupImageView() {
        imageView.image = image
        imageView.contentMode = .scaleAspectFit
        imageView.translatesAutoresizingMaskIntoConstraints = false

        scrollView.addSubview(imageView)
    }

    private func updateZoomScale() {
        let boundsSize = scrollView.bounds.size
        guard boundsSize.width > 0, boundsSize.height > 0 else { return }

        let imageSize = image.size
        guard imageSize.width > 0, imageSize.height > 0 else { return }

        let widthScale = boundsSize.width / imageSize.width
        let heightScale = boundsSize.height / imageSize.height
        let minScale = min(widthScale, heightScale)

        scrollView.minimumZoomScale = minScale
        scrollView.maximumZoomScale = max(minScale * 5, 3.0)

        // Only reset zoom if not already zoomed by user
        if scrollView.zoomScale < minScale || scrollView.zoomScale == 1.0 {
            scrollView.zoomScale = minScale
        }

        // Size the image view to the image's natural size
        imageView.frame = CGRect(origin: .zero, size: imageSize)
        scrollView.contentSize = imageSize

        centerImageView()
    }

    private func centerImageView() {
        let boundsSize = scrollView.bounds.size
        let contentSize = scrollView.contentSize

        let offsetX = max(0, (boundsSize.width - contentSize.width * scrollView.zoomScale) / 2)
        let offsetY = max(0, (boundsSize.height - contentSize.height * scrollView.zoomScale) / 2)

        scrollView.contentInset = UIEdgeInsets(top: offsetY, left: offsetX, bottom: offsetY, right: offsetX)
    }

    // MARK: - UIScrollViewDelegate

    func viewForZooming(in scrollView: UIScrollView) -> UIView? {
        imageView
    }

    func scrollViewDidZoom(_ scrollView: UIScrollView) {
        centerImageView()
    }

    // MARK: - Header

    private func setupHeader() {
        // Gradient background for header
        headerGradient.colors = [UIColor.black.withAlphaComponent(0.6).cgColor, UIColor.clear.cgColor]
        headerGradient.locations = [0, 1]
        let headerView = UIView()
        headerView.layer.addSublayer(headerGradient)
        headerView.translatesAutoresizingMaskIntoConstraints = false
        headerView.isUserInteractionEnabled = true
        view.addSubview(headerView)
        NSLayoutConstraint.activate([
            headerView.topAnchor.constraint(equalTo: view.topAnchor),
            headerView.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            headerView.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            headerView.heightAnchor.constraint(equalToConstant: 120),
        ])

        // Title
        titleLabel.text = imageName
        titleLabel.textColor = .white.withAlphaComponent(0.8)
        titleLabel.font = .systemFont(ofSize: 15, weight: .medium)
        titleLabel.lineBreakMode = .byTruncatingTail
        titleLabel.translatesAutoresizingMaskIntoConstraints = false
        headerView.addSubview(titleLabel)

        // Close button
        let config = UIImage.SymbolConfiguration(pointSize: 16, weight: .semibold)
        closeButton.setImage(UIImage(systemName: "xmark", withConfiguration: config), for: .normal)
        closeButton.tintColor = .white.withAlphaComponent(0.8)
        closeButton.backgroundColor = UIColor.white.withAlphaComponent(0.15)
        closeButton.layer.cornerRadius = 16
        closeButton.addTarget(self, action: #selector(closeTapped), for: .touchUpInside)
        closeButton.translatesAutoresizingMaskIntoConstraints = false
        headerView.addSubview(closeButton)

        NSLayoutConstraint.activate([
            titleLabel.leadingAnchor.constraint(equalTo: headerView.leadingAnchor, constant: 16),
            titleLabel.trailingAnchor.constraint(lessThanOrEqualTo: closeButton.leadingAnchor, constant: -12),
            titleLabel.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor, constant: 12),

            closeButton.trailingAnchor.constraint(equalTo: headerView.trailingAnchor, constant: -16),
            closeButton.centerYAnchor.constraint(equalTo: titleLabel.centerYAnchor),
            closeButton.widthAnchor.constraint(equalToConstant: 32),
            closeButton.heightAnchor.constraint(equalToConstant: 32),
        ])
    }

    @objc private func closeTapped() {
        dismiss(animated: true)
    }

    // MARK: - Gestures

    private func setupGestures() {
        // Double-tap to toggle zoom
        let doubleTap = UITapGestureRecognizer(target: self, action: #selector(handleDoubleTap(_:)))
        doubleTap.numberOfTapsRequired = 2
        scrollView.addGestureRecognizer(doubleTap)

        // Single-tap to toggle header visibility
        let singleTap = UITapGestureRecognizer(target: self, action: #selector(handleSingleTap))
        singleTap.numberOfTapsRequired = 1
        singleTap.require(toFail: doubleTap)
        scrollView.addGestureRecognizer(singleTap)
    }

    @objc private func handleDoubleTap(_ gesture: UITapGestureRecognizer) {
        if scrollView.zoomScale > scrollView.minimumZoomScale {
            scrollView.setZoomScale(scrollView.minimumZoomScale, animated: true)
        } else {
            // Zoom to 2x centered on the tap point
            let tapPoint = gesture.location(in: imageView)
            let targetScale = min(scrollView.minimumZoomScale * 3, scrollView.maximumZoomScale)
            let zoomRect = zoomRectForScale(targetScale, center: tapPoint)
            scrollView.zoom(to: zoomRect, animated: true)
        }
    }

    private func zoomRectForScale(_ scale: CGFloat, center: CGPoint) -> CGRect {
        let size = CGSize(
            width: scrollView.bounds.width / scale,
            height: scrollView.bounds.height / scale
        )
        return CGRect(
            x: center.x - size.width / 2,
            y: center.y - size.height / 2,
            width: size.width,
            height: size.height
        )
    }

    @objc private func handleSingleTap() {
        let isHidden = closeButton.alpha < 0.5
        UIView.animate(withDuration: 0.25) {
            let alpha: CGFloat = isHidden ? 1.0 : 0.0
            self.closeButton.alpha = alpha
            self.titleLabel.alpha = alpha
        }
    }
}
