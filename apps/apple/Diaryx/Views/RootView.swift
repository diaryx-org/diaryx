import SwiftUI

struct RootView: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        switch appState.activeView {
        case .welcome:
            WelcomeView()
        case .workspace(let workspaceState):
            WorkspaceView(workspace: workspaceState)
        }
    }
}
