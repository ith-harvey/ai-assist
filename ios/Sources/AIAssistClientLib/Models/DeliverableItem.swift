import Foundation
import SwiftUI

/// A deliverable produced by a task — either a document or a message draft (approval card).
///
/// Documents and message drafts are distinct models: `Document` for research/reports,
/// `ApprovalCard` (compose/reply) for messages requiring approval before sending.
/// The deliverables list merges both into a single sorted view.
public enum DeliverableItem: Identifiable {
    case document(Document)
    case message(ApprovalCard)

    public var id: String {
        switch self {
        case .document(let doc): doc.id.uuidString
        case .message(let card): card.id.uuidString
        }
    }

    /// Display title for the deliverable row.
    public var title: String {
        switch self {
        case .document(let doc):
            doc.title
        case .message(let card):
            if case .compose(_, let recipient, let subject, _, _) = card.payload {
                subject ?? "Message to \(recipient)"
            } else {
                "Reply: \(card.sourceSender)"
            }
        }
    }

    /// SF Symbol name for the row icon.
    public var iconName: String {
        switch self {
        case .document(let doc): doc.docType.iconName
        case .message: "envelope.fill"
        }
    }

    /// Icon tint color — blue for documents, orange for messages.
    public var iconColor: Color {
        switch self {
        case .document: .blue
        case .message: .orange
        }
    }

    /// Subtitle text below the title.
    public var subtitle: String {
        switch self {
        case .document(let doc):
            doc.docType.label
        case .message(let card):
            card.channel.isEmpty ? "Message" : card.channel.capitalized
        }
    }

    /// Creation date for sorting.
    public var createdAt: Date {
        switch self {
        case .document(let doc):
            doc.createdAt
        case .message(let card):
            ISO8601DateFormatter().date(from: card.createdAt) ?? Date.distantPast
        }
    }

    /// Whether this deliverable has been dismissed (only applies to messages).
    public var isDismissed: Bool {
        switch self {
        case .document: false
        case .message(let card): card.status == .dismissed
        }
    }

    /// Whether this deliverable has been sent/approved.
    public var isSent: Bool {
        switch self {
        case .document: false
        case .message(let card): card.status == .approved || card.status == .sent
        }
    }
}
