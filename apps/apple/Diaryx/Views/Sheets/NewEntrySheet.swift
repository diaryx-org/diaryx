import SwiftUI

struct NewEntrySheet: View {
    @Binding var entryName: String
    let onCreate: () -> Void
    let onCancel: () -> Void

    var body: some View {
        VStack(spacing: 16) {
            Text("New Entry")
                .font(.headline)

            TextField("Filename (e.g. 2026/02/16.md)", text: $entryName)
                .textFieldStyle(.roundedBorder)
                .onSubmit {
                    guard !entryName.trimmingCharacters(in: .whitespaces).isEmpty else { return }
                    onCreate()
                }

            HStack {
                Button("Cancel", role: .cancel, action: onCancel)
                    .keyboardShortcut(.cancelAction)
                Button("Create", action: onCreate)
                    .keyboardShortcut(.defaultAction)
                    .disabled(entryName.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding()
        .frame(minWidth: 320)
    }
}
