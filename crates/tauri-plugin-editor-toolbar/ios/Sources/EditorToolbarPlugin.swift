import UIKit
import WebKit
import Tauri

// MARK: - Plugin Entry Point

@_cdecl("init_plugin_editor_toolbar")
func initPlugin() -> Plugin {
    return EditorToolbarPlugin()
}

// MARK: - Editor Toolbar Plugin

class EditorToolbarPlugin: Plugin, WKScriptMessageHandler {
    private weak var webView: WKWebView?
    private var toolbar: EditorToolbar?
    private var keyboardObserver: NSObjectProtocol?

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
        default:
            break
        }
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

                pluginMarkIds = (commands.marks || []).map(function(c) { return c.extensionId; });

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

    private func findViewController() -> UIViewController? {
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
                    iconName: item["iconName"] as? String, nodeType: "blockAtom"
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

        guard !pluginCommands.isEmpty else { return }

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

        for cmd in pluginCommands {
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
            button.addAction(UIAction { [weak self] _ in
                self?.execPluginCommand(captured)
            }, for: .touchUpInside)

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
    ]
}

// MARK: - Plugin Command

struct PluginCommand: Equatable {
    let extensionId: String
    let label: String
    let iconName: String?
    let nodeType: String  // "mark", "inlineAtom", "blockAtom", "blockPickerItem"
    var editorCommand: String? = nil
    var params: [String: Any]? = nil
    var promptMessage: String? = nil
    var promptDefault: String? = nil
    var promptParamKey: String? = nil

    static func == (lhs: PluginCommand, rhs: PluginCommand) -> Bool {
        lhs.extensionId == rhs.extensionId &&
        lhs.label == rhs.label &&
        lhs.iconName == rhs.iconName &&
        lhs.nodeType == rhs.nodeType &&
        lhs.editorCommand == rhs.editorCommand &&
        lhs.promptMessage == rhs.promptMessage &&
        lhs.promptDefault == rhs.promptDefault &&
        lhs.promptParamKey == rhs.promptParamKey
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
