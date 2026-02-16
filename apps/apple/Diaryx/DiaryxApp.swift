import SwiftUI

struct SaveAction {
    var save: () -> Void
}

struct SaveActionKey: FocusedValueKey {
    typealias Value = SaveAction
}

extension FocusedValues {
    var saveAction: SaveAction? {
        get { self[SaveActionKey.self] }
        set { self[SaveActionKey.self] = newValue }
    }
}

@main
struct DiaryxApp: App {
    @FocusedValue(\.saveAction) private var saveAction

    var body: some Scene {
        WindowGroup {
            ContentView()
        }
        .defaultSize(width: 1000, height: 700)
        .commands {
            CommandGroup(after: .saveItem) {
                Button("Save") {
                    saveAction?.save()
                }
                .keyboardShortcut("s", modifiers: .command)
            }
        }
    }
}
