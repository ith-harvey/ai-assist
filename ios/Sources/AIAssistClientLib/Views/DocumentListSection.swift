import SwiftUI

/// A section showing documents linked to a todo.
/// Embeds into TodoDetailView as a collapsible section.
public struct DocumentListSection: View {
    let todoId: UUID

    @State private var api: DocumentAPI
    @State private var selectedDocument: Document?

    public init(todoId: UUID, host: String = "localhost", port: Int = 3001) {
        self.todoId = todoId
        self._api = State(initialValue: DocumentAPI(host: host, port: port))
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if api.isLoading {
                HStack {
                    ProgressView()
                        .controlSize(.small)
                    Text("Loading documents…")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .padding(.vertical, 4)
            } else if let error = api.error {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
            } else if api.documents.isEmpty {
                // Show nothing if no documents
            } else {
                // Section header
                HStack {
                    Image(systemName: "doc.text.fill")
                        .foregroundStyle(.blue)
                    Text("Documents")
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(.primary)
                    Spacer()
                    Text("\(api.documents.count)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(.fill.tertiary, in: Capsule())
                }

                ForEach(api.documents) { doc in
                    Button {
                        selectedDocument = doc
                    } label: {
                        DocumentRow(document: doc)
                    }
                    .buttonStyle(.plain)
                }
            }
        }
        .task {
            await api.fetchDocuments(forTodoId: todoId)
        }
        .sheet(item: $selectedDocument) { doc in
            DocumentDetailView(document: doc)
        }
    }
}

/// A single document row in the list.
private struct DocumentRow: View {
    let document: Document

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: document.docType.iconName)
                .font(.title3)
                .foregroundStyle(.blue)
                .frame(width: 28)

            VStack(alignment: .leading, spacing: 2) {
                Text(document.title)
                    .font(.subheadline.weight(.medium))
                    .foregroundStyle(.primary)
                    .lineLimit(1)

                HStack(spacing: 6) {
                    Text(document.docType.label)
                        .font(.caption2)
                        .foregroundStyle(.secondary)

                    Text("·")
                        .foregroundStyle(.secondary)

                    Text(document.createdAt, style: .relative)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }

            Spacer()

            Image(systemName: "chevron.right")
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .padding(.vertical, 6)
        .padding(.horizontal, 10)
        #if os(iOS)
        .background(Color(.secondarySystemBackground), in: RoundedRectangle(cornerRadius: 8))
        #else
        .background(Color.gray.opacity(0.1), in: RoundedRectangle(cornerRadius: 8))
        #endif
    }
}

/// Full-screen document viewer.
struct DocumentDetailView: View {
    let document: Document
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    // Metadata header
                    VStack(alignment: .leading, spacing: 4) {
                        HStack(spacing: 6) {
                            Image(systemName: document.docType.iconName)
                                .foregroundStyle(.blue)
                            Text(document.docType.label)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                            Spacer()
                            Text(document.createdAt, style: .date)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }

                        Text("by \(document.createdBy)")
                            .font(.caption)
                            .foregroundStyle(.tertiary)
                    }
                    .padding(.horizontal)

                    Divider()

                    // Content body with markdown rendering
                    MarkdownBodyView(content: document.content)
                        .textSelection(.enabled)
                        .padding(.horizontal)
                }
                .padding(.vertical)
            }
            .navigationTitle(document.title)
            #if os(iOS)
            .navigationBarTitleDisplayMode(.large)
            #endif
            .toolbar {
                #if os(iOS)
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
                #else
                ToolbarItem(placement: .automatic) {
                    Button("Done") { dismiss() }
                }
                #endif
            }
        }
    }
}
