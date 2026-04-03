import Foundation
import Testing
@testable import AIAssistClientLib

@Suite("Document Tests")
struct DocumentTests {

    // MARK: - DocumentType

    @Test("DocumentType has all 7 cases")
    func documentTypeCaseCount() {
        #expect(DocumentType.allCases.count == 7)
    }

    @Test("DocumentType labels are correct")
    func documentTypeLabels() {
        #expect(DocumentType.research.label == "Research")
        #expect(DocumentType.instructions.label == "Instructions")
        #expect(DocumentType.notes.label == "Notes")
        #expect(DocumentType.report.label == "Report")
        #expect(DocumentType.design.label == "Design")
        #expect(DocumentType.summary.label == "Summary")
        #expect(DocumentType.other.label == "Other")
    }

    @Test("DocumentType iconNames are non-empty SF Symbol names")
    func documentTypeIconNames() {
        for docType in DocumentType.allCases {
            #expect(!docType.iconName.isEmpty, "iconName for \(docType) should not be empty")
        }
    }

    @Test("DocumentType round-trips through Codable")
    func documentTypeCodable() throws {
        for docType in DocumentType.allCases {
            let data = try JSONEncoder().encode(docType)
            let decoded = try JSONDecoder().decode(DocumentType.self, from: data)
            #expect(decoded == docType)
        }
    }

    // MARK: - Document JSON Decoding

    private func documentJSON(
        id: String = "11111111-1111-1111-1111-111111111111",
        todoId: String = "22222222-2222-2222-2222-222222222222",
        title: String = "Research findings",
        content: String = "# Results\n\nKey findings here.",
        docType: String = "research",
        createdBy: String = "agent"
    ) -> String {
        return """
        {
            "id": "\(id)",
            "todo_id": "\(todoId)",
            "title": "\(title)",
            "content": "\(content)",
            "doc_type": "\(docType)",
            "created_by": "\(createdBy)",
            "created_at": "2026-03-15T10:00:00Z",
            "updated_at": "2026-03-15T11:00:00Z"
        }
        """
    }

    @Test("Decode Document from snake_case JSON")
    func decodeDocument() throws {
        let json = documentJSON()
        let data = json.data(using: .utf8)!
        let doc = try Document.decode(from: data)

        #expect(doc.id == UUID(uuidString: "11111111-1111-1111-1111-111111111111")!)
        #expect(doc.todoId == UUID(uuidString: "22222222-2222-2222-2222-222222222222")!)
        #expect(doc.title == "Research findings")
        #expect(doc.docType == .research)
        #expect(doc.createdBy == "agent")
    }

    @Test("Decode Document array")
    func decodeDocumentArray() throws {
        let d1 = documentJSON(
            id: "11111111-1111-1111-1111-111111111111",
            title: "First"
        )
        let d2 = documentJSON(
            id: "33333333-3333-3333-3333-333333333333",
            title: "Second",
            docType: "notes"
        )
        let json = "[\(d1), \(d2)]"
        let data = json.data(using: .utf8)!
        let docs = try Document.decodeArray(from: data)
        #expect(docs.count == 2)
        #expect(docs[0].title == "First")
        #expect(docs[1].title == "Second")
        #expect(docs[1].docType == .notes)
    }

    @Test("Decode all DocumentType variants from JSON")
    func decodeAllDocTypes() throws {
        let types = ["research", "instructions", "notes", "report", "design", "summary", "other"]
        for typeName in types {
            let json = documentJSON(docType: typeName)
            let data = json.data(using: .utf8)!
            let doc = try Document.decode(from: data)
            #expect(doc.docType.rawValue == typeName)
        }
    }

    // MARK: - Hashable / Identifiable

    @Test("Document Hashable conformance works")
    func documentHashable() {
        let id = UUID()
        let todoId = UUID()
        let date = Date()
        let a = Document(id: id, todoId: todoId, title: "Same", content: "Same", createdAt: date, updatedAt: date)
        let b = Document(id: id, todoId: todoId, title: "Same", content: "Same", createdAt: date, updatedAt: date)
        #expect(a == b)
        #expect(a.hashValue == b.hashValue)
    }

    @Test("Document init uses default values")
    func documentDefaults() {
        let todoId = UUID()
        let doc = Document(todoId: todoId, title: "Quick note", content: "Content")
        #expect(doc.docType == .other)
        #expect(doc.createdBy == "agent")
    }
}
