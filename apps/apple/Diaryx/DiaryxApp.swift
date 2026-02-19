import SwiftUI

struct SaveAction {
    var save: () -> Void
}

struct SaveActionKey: FocusedValueKey {
    typealias Value = SaveAction
}

struct ToggleCommandPaletteAction {
    var toggle: () -> Void

    init(_ toggle: @escaping () -> Void) {
        self.toggle = toggle
    }
}

struct ToggleCommandPaletteKey: FocusedValueKey {
    typealias Value = ToggleCommandPaletteAction
}

extension FocusedValues {
    var saveAction: SaveAction? {
        get { self[SaveActionKey.self] }
        set { self[SaveActionKey.self] = newValue }
    }

    var toggleCommandPalette: ToggleCommandPaletteAction? {
        get { self[ToggleCommandPaletteKey.self] }
        set { self[ToggleCommandPaletteKey.self] = newValue }
    }
}

@main
struct DiaryxApp: App {
    @State private var appState = AppState()
    @State private var appSettings = AppSettings()
    @FocusedValue(\.saveAction) private var saveAction
    @FocusedValue(\.toggleCommandPalette) private var togglePalette

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(appState)
                .environment(appSettings)
                .preferredColorScheme(appSettings.theme.colorScheme)
        }
        .defaultSize(width: 1000, height: 700)
        .commands {
            CommandGroup(after: .saveItem) {
                Button("Save") {
                    saveAction?.save()
                }
                .keyboardShortcut("s", modifiers: .command)
            }
            CommandGroup(after: .textEditing) {
                Button("Command Palette") {
                    togglePalette?.toggle()
                }
                .keyboardShortcut("k", modifiers: .command)
            }
        }

        #if os(macOS)
        Settings {
            SettingsView()
                .environment(appState)
                .environment(appSettings)
        }
        #endif
    }
}
