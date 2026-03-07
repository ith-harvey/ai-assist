import SwiftUI

/// Prominently displays deliverable documents linked to a completed todo.
///
/// Fetches documents via DocumentAPI, then renders each document's full
/// markdown content inline with title header and doc-type badge.
/// Shows nothing if no documents are linked.
struct DeliverableSection: View {
    let todoId: UUID

    @State private var api: DocumentAPI
    @State private var selectedDocument: Document?

    init(todoId: UUID, host: String = "localhost", port: Int = 3001) {
        self.todoId = todoId
        self._api = State(initialValue: DocumentAPI(host: host, port: port))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            if api.isLoading {
                HStack(spacing: 8) {
                    ProgressView()
                        .controlSize(.small)
                    Text("Loading deliverables…")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .padding(.vertical, 8)
            } else if !api.documents.isEmpty {
                ForEach(api.documents) { doc in
                    deliverableCard(doc)
                }
            }
            // Show nothing if no documents (no empty state needed)
        }
        .task {
            await api.fetchDocuments(forTodoId: todoId)
        }
    }

    // MARK: - Deliverable Card

    @ViewBuilder
    private func deliverableCard(_ doc: Document) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            // Title + type badge
            HStack(spacing: 8) {
                Image(systemName: doc.docType.iconName)
                    .font(.system(size: 18))
                    .foregroundStyle(.blue)

                Text(doc.title)
                    .font(.headline)
                    .foregroundStyle(.primary)
                    .lineLimit(3)

                Spacer()

                Text(doc.docType.label)
                    .font(.system(size: 11, weight: .medium))
                    .padding(.horizontal, 8)
                    .padding(.vertical, 3)
                    .background(.blue.opacity(0.12))
                    .foregroundStyle(.blue)
                    .clipShape(Capsule())
            }

            // Full markdown body
            MarkdownBodyView(content: doc.content)
                .padding(.top, 4)

            // Footer: author + date
            HStack(spacing: 8) {
                Text("by \(doc.createdBy)")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
                Spacer()
                Text(doc.createdAt, style: .date)
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }
            .padding(.top, 4)
        }
        .padding(16)
        #if os(iOS)
        .background(Color(uiColor: .systemBackground))
        #else
        .background(Color.white)
        #endif
        .clipShape(RoundedRectangle(cornerRadius: 14))
        .shadow(color: .black.opacity(0.08), radius: 6, y: 3)
    }
}
