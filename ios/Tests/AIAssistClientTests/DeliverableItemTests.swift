import Foundation
import Testing
@testable import AIAssistClientLib

@Suite("DeliverableItem Tests")
struct DeliverableItemTests {

    // MARK: - Helpers

    private func makeDocument(
        id: UUID = UUID(),
        todoId: UUID = UUID(),
        title: String = "Research Doc",
        docType: DocumentType = .research
    ) -> Document {
        Document(
            id: id,
            todoId: todoId,
            title: title,
            content: "Content here",
            docType: docType
        )
    }

    private func makeReplyCard(
        id: String = "550e8400-e29b-41d4-a716-446655440000",
        sender: String = "Alice",
        channel: String = "telegram",
        status: String = "pending"
    ) throws -> ApprovalCard {
        let json = """
        {
            "id": "\(id)",
            "card_type": "reply",
            "silo": "messages",
            "payload": {
                "channel": "\(channel)",
                "source_sender": "\(sender)",
                "source_message": "Hello",
                "suggested_reply": "Hi!",
                "confidence": 0.9,
                "conversation_id": "chat_1"
            },
            "status": "\(status)",
            "created_at": "2026-03-15T10:00:00Z",
            "expires_at": "2026-03-15T10:15:00Z",
            "updated_at": "2026-03-15T10:00:00Z"
        }
        """
        return try ApprovalCard.decode(from: json.data(using: .utf8)!)
    }

    private func makeComposeCard(
        id: String = "660e8400-e29b-41d4-a716-446655440000",
        recipient: String = "bob@example.com",
        subject: String? = "Meeting notes",
        status: String = "pending"
    ) throws -> ApprovalCard {
        let subjectField = subject.map { "\"subject\": \"\($0)\"," } ?? ""
        let json = """
        {
            "id": "\(id)",
            "card_type": "compose",
            "silo": "messages",
            "payload": {
                "channel": "email",
                "recipient": "\(recipient)",
                \(subjectField)
                "draft_body": "Here are the notes.",
                "confidence": 0.85
            },
            "status": "\(status)",
            "created_at": "2026-03-15T10:00:00Z",
            "expires_at": null,
            "updated_at": "2026-03-15T10:00:00Z"
        }
        """
        return try ApprovalCard.decode(from: json.data(using: .utf8)!)
    }

    // MARK: - Identity

    @Test("Document deliverable uses document ID")
    func documentId() {
        let docId = UUID()
        let doc = makeDocument(id: docId)
        let item = DeliverableItem.document(doc)
        #expect(item.id == docId.uuidString)
    }

    @Test("Message deliverable uses card ID")
    func messageId() throws {
        let card = try makeReplyCard()
        let item = DeliverableItem.message(card)
        #expect(item.id == "550E8400-E29B-41D4-A716-446655440000")
    }

    // MARK: - Title

    @Test("Document title uses document title")
    func documentTitle() {
        let doc = makeDocument(title: "My Research")
        let item = DeliverableItem.document(doc)
        #expect(item.title == "My Research")
    }

    @Test("Reply message title shows sender")
    func replyTitle() throws {
        let card = try makeReplyCard(sender: "Bob")
        let item = DeliverableItem.message(card)
        #expect(item.title == "Reply: Bob")
    }

    @Test("Compose message title uses subject when available")
    func composeTitleWithSubject() throws {
        let card = try makeComposeCard(subject: "Meeting notes")
        let item = DeliverableItem.message(card)
        #expect(item.title == "Meeting notes")
    }

    @Test("Compose message title falls back to recipient when no subject")
    func composeTitleNoSubject() throws {
        let card = try makeComposeCard(subject: nil)
        let item = DeliverableItem.message(card)
        #expect(item.title == "Message to bob@example.com")
    }

    // MARK: - Icon

    @Test("Document icon uses docType iconName")
    func documentIcon() {
        let doc = makeDocument(docType: .research)
        let item = DeliverableItem.document(doc)
        #expect(item.iconName == "magnifyingglass.circle")
    }

    @Test("Message icon is envelope.fill")
    func messageIcon() throws {
        let card = try makeReplyCard()
        let item = DeliverableItem.message(card)
        #expect(item.iconName == "envelope.fill")
    }

    // MARK: - Subtitle

    @Test("Document subtitle uses docType label")
    func documentSubtitle() {
        let doc = makeDocument(docType: .notes)
        let item = DeliverableItem.document(doc)
        #expect(item.subtitle == "Notes")
    }

    @Test("Message subtitle uses capitalized channel name")
    func messageSubtitle() throws {
        let card = try makeReplyCard(channel: "telegram")
        let item = DeliverableItem.message(card)
        #expect(item.subtitle == "Telegram")
    }

    // MARK: - Status flags

    @Test("Document is never dismissed")
    func documentNotDismissed() {
        let doc = makeDocument()
        let item = DeliverableItem.document(doc)
        #expect(item.isDismissed == false)
    }

    @Test("Document is never sent")
    func documentNotSent() {
        let doc = makeDocument()
        let item = DeliverableItem.document(doc)
        #expect(item.isSent == false)
    }

    @Test("Dismissed message card reports isDismissed")
    func dismissedMessage() throws {
        let card = try makeReplyCard(status: "dismissed")
        let item = DeliverableItem.message(card)
        #expect(item.isDismissed == true)
        #expect(item.isSent == false)
    }

    @Test("Approved message card reports isSent")
    func approvedMessage() throws {
        let card = try makeReplyCard(status: "approved")
        let item = DeliverableItem.message(card)
        #expect(item.isSent == true)
        #expect(item.isDismissed == false)
    }

    @Test("Sent message card reports isSent")
    func sentMessage() throws {
        let card = try makeReplyCard(status: "sent")
        let item = DeliverableItem.message(card)
        #expect(item.isSent == true)
    }

    @Test("Pending message card is neither dismissed nor sent")
    func pendingMessage() throws {
        let card = try makeReplyCard(status: "pending")
        let item = DeliverableItem.message(card)
        #expect(item.isDismissed == false)
        #expect(item.isSent == false)
    }

    // MARK: - CreatedAt

    @Test("Document createdAt uses document date")
    func documentCreatedAt() {
        let date = Date(timeIntervalSince1970: 1000000)
        let doc = Document(todoId: UUID(), title: "T", content: "C", createdAt: date, updatedAt: date)
        let item = DeliverableItem.document(doc)
        #expect(item.createdAt == date)
    }

    @Test("Message createdAt parses ISO8601 from card")
    func messageCreatedAt() throws {
        let card = try makeReplyCard()
        let item = DeliverableItem.message(card)
        // Should parse "2026-03-15T10:00:00Z" successfully
        #expect(item.createdAt != Date.distantPast)
    }
}
