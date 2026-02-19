import SwiftUI

struct EditorDetailView: View {
    let selectedPath: String?
    let loadedMarkdown: String
    let onContentChanged: (String) -> Void
    let onLinkClicked: (String) -> Void

    var body: some View {
        if selectedPath != nil {
            EditorWebView(
                initialMarkdown: loadedMarkdown,
                onContentChanged: onContentChanged,
                onLinkClicked: onLinkClicked
            )
        } else {
            ContentUnavailableView {
                Label("No File Selected", systemImage: "doc.text")
            } description: {
                Text("Select a Markdown file from the sidebar to start editing.")
            }
        }
    }
}
