import SwiftUI

struct MetadataSidebar: View {
    let metadata: [MetadataFieldItem]

    var body: some View {
        Group {
            if metadata.isEmpty {
                ContentUnavailableView {
                    Label("No Metadata", systemImage: "doc.text.magnifyingglass")
                } description: {
                    Text("This file has no frontmatter.")
                }
            } else {
                List(metadata) { field in
                    MetadataRow(field: field)
                }
                .listStyle(.sidebar)
            }
        }
        .navigationTitle("Metadata")
    }
}

private struct MetadataRow: View {
    let field: MetadataFieldItem

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(field.key.uppercased())
                .font(.caption)
                .foregroundStyle(.secondary)

            if field.isArray {
                ForEach(field.values, id: \.self) { item in
                    HStack(alignment: .firstTextBaseline, spacing: 4) {
                        Text("\u{2022}")
                            .foregroundStyle(.secondary)
                        Text(item)
                            .textSelection(.enabled)
                    }
                }
            } else {
                Text(field.value.isEmpty ? "--" : field.value)
                    .textSelection(.enabled)
                    .foregroundStyle(field.value.isEmpty ? .secondary : .primary)
            }
        }
        .padding(.vertical, 2)
    }
}

#Preview("With Metadata") {
    MetadataSidebar(metadata: [
        MetadataFieldItem(id: "title", key: "title", value: "My Entry", values: []),
        MetadataFieldItem(id: "date", key: "date", value: "2026-02-16", values: []),
        MetadataFieldItem(id: "tags", key: "tags", value: "", values: ["swift", "rust", "diary"]),
        MetadataFieldItem(id: "draft", key: "draft", value: "true", values: []),
    ])
    .frame(width: 260, height: 400)
}

#Preview("Empty") {
    MetadataSidebar(metadata: [])
        .frame(width: 260, height: 400)
}
