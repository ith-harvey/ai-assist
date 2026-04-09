import Foundation
import Testing
@testable import AIAssistClientLib

@Suite("TodoItem Tests")
struct TodoItemTests {

    // MARK: - TodoType

    @Test("TodoType label returns correct display names")
    func todoTypeLabels() {
        #expect(TodoType.deliverable.label == "Deliverable")
        #expect(TodoType.research.label == "Research")
        #expect(TodoType.errand.label == "Errand")
        #expect(TodoType.learning.label == "Learning")
        #expect(TodoType.administrative.label == "Admin")
        #expect(TodoType.creative.label == "Creative")
        #expect(TodoType.review.label == "Review")
    }

    @Test("TodoType color returns correct string values")
    func todoTypeColors() {
        #expect(TodoType.deliverable.color == "blue")
        #expect(TodoType.research.color == "purple")
        #expect(TodoType.errand.color == "orange")
        #expect(TodoType.learning.color == "green")
        #expect(TodoType.administrative.color == "gray")
        #expect(TodoType.creative.color == "pink")
        #expect(TodoType.review.color == "yellow")
    }

    @Test("TodoType has all 7 cases")
    func todoTypeCaseIterable() {
        #expect(TodoType.allCases.count == 7)
    }

    @Test("TodoType raw values round-trip through Codable")
    func todoTypeCodable() throws {
        for todoType in TodoType.allCases {
            let data = try JSONEncoder().encode(todoType)
            let decoded = try JSONDecoder().decode(TodoType.self, from: data)
            #expect(decoded == todoType)
        }
    }

    // MARK: - TodoBucket

    @Test("TodoBucket decodes from snake_case raw values")
    func todoBucketDecoding() throws {
        let agentData = "\"agent_startable\"".data(using: .utf8)!
        let humanData = "\"human_only\"".data(using: .utf8)!
        #expect(try JSONDecoder().decode(TodoBucket.self, from: agentData) == .agentStartable)
        #expect(try JSONDecoder().decode(TodoBucket.self, from: humanData) == .humanOnly)
    }

    // MARK: - TodoStatus

    @Test("TodoStatus label returns correct display names")
    func todoStatusLabels() {
        #expect(TodoStatus.drafting.label == "Drafting")
        #expect(TodoStatus.created.label == "Created")
        #expect(TodoStatus.agentQueued.label == "Queued")
        #expect(TodoStatus.agentWorking.label == "Agent working")
        #expect(TodoStatus.awaitingApproval.label == "Awaiting approval")
        #expect(TodoStatus.readyForReview.label == "Ready for review")
        #expect(TodoStatus.waitingOnYou.label == "Waiting on you")
        #expect(TodoStatus.snoozed.label == "Snoozed")
        #expect(TodoStatus.completed.label == "Completed")
    }

    @Test("TodoStatus iconName is non-empty for all variants")
    func todoStatusIconNames() {
        let allStatuses: [TodoStatus] = [
            .drafting, .created, .agentQueued, .agentWorking,
            .awaitingApproval, .readyForReview, .waitingOnYou,
            .snoozed, .completed,
        ]
        for status in allStatuses {
            #expect(!status.iconName.isEmpty, "iconName for \(status) should not be empty")
        }
    }

    @Test("TodoStatus isActive returns false only for completed and snoozed")
    func todoStatusIsActive() {
        #expect(TodoStatus.drafting.isActive == true)
        #expect(TodoStatus.created.isActive == true)
        #expect(TodoStatus.agentQueued.isActive == true)
        #expect(TodoStatus.agentWorking.isActive == true)
        #expect(TodoStatus.awaitingApproval.isActive == true)
        #expect(TodoStatus.readyForReview.isActive == true)
        #expect(TodoStatus.waitingOnYou.isActive == true)
        #expect(TodoStatus.snoozed.isActive == false)
        #expect(TodoStatus.completed.isActive == false)
    }

    @Test("TodoStatus decodes snake_case raw values from JSON")
    func todoStatusSnakeCaseDecoding() throws {
        let cases: [(String, TodoStatus)] = [
            ("\"drafting\"", .drafting),
            ("\"created\"", .created),
            ("\"agent_queued\"", .agentQueued),
            ("\"agent_working\"", .agentWorking),
            ("\"awaiting_approval\"", .awaitingApproval),
            ("\"ready_for_review\"", .readyForReview),
            ("\"waiting_on_you\"", .waitingOnYou),
            ("\"snoozed\"", .snoozed),
            ("\"completed\"", .completed),
        ]
        for (json, expected) in cases {
            let data = json.data(using: .utf8)!
            let decoded = try JSONDecoder().decode(TodoStatus.self, from: data)
            #expect(decoded == expected)
        }
    }

    // MARK: - TodoItem JSON Decoding

    private func todoJSON(
        id: String = "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
        title: String = "Test todo",
        todoType: String = "deliverable",
        bucket: String = "human_only",
        status: String = "created",
        priority: Int = 3,
        dueDate: String? = nil,
        description: String? = nil,
        context: String? = nil,
        sourceCardId: String? = nil,
        snoozedUntil: String? = nil,
        parentId: String? = nil
    ) -> String {
        var fields = """
        "id": "\(id)",
        "title": "\(title)",
        "todo_type": "\(todoType)",
        "bucket": "\(bucket)",
        "status": "\(status)",
        "priority": \(priority),
        "created_at": "2026-03-01T10:00:00Z",
        "updated_at": "2026-03-01T10:00:00Z"
        """
        if let dueDate { fields += ",\n\"due_date\": \"\(dueDate)\"" }
        if let description { fields += ",\n\"description\": \"\(description)\"" }
        if let context { fields += ",\n\"context\": \"\(context)\"" }
        if let sourceCardId { fields += ",\n\"source_card_id\": \"\(sourceCardId)\"" }
        if let snoozedUntil { fields += ",\n\"snoozed_until\": \"\(snoozedUntil)\"" }
        if let parentId { fields += ",\n\"parent_id\": \"\(parentId)\"" }
        return "{\(fields)}"
    }

    @Test("Decode TodoItem from snake_case JSON")
    func decodeTodoItem() throws {
        let json = todoJSON(
            title: "Fix the bug",
            todoType: "deliverable",
            bucket: "agent_startable",
            status: "agent_working",
            priority: 1,
            description: "Important fix",
            context: "Release blocker"
        )
        let data = json.data(using: .utf8)!
        let todo = try TodoItem.decode(from: data)

        #expect(todo.id == UUID(uuidString: "a1b2c3d4-e5f6-7890-abcd-ef1234567890")!)
        #expect(todo.title == "Fix the bug")
        #expect(todo.todoType == .deliverable)
        #expect(todo.bucket == .agentStartable)
        #expect(todo.status == .agentWorking)
        #expect(todo.priority == 1)
        #expect(todo.description == "Important fix")
        #expect(todo.context == "Release blocker")
    }

    @Test("Decode TodoItem array")
    func decodeTodoItemArray() throws {
        let todo1 = todoJSON(id: "a1b2c3d4-e5f6-7890-abcd-ef1234567890", title: "First")
        let todo2 = todoJSON(id: "b2c3d4e5-f6a7-8901-bcde-f12345678901", title: "Second")
        let json = "[\(todo1), \(todo2)]"
        let data = json.data(using: .utf8)!
        let todos = try TodoItem.decodeArray(from: data)
        #expect(todos.count == 2)
        #expect(todos[0].title == "First")
        #expect(todos[1].title == "Second")
    }

    @Test("TodoItem with optional fields nil decodes correctly")
    func decodeTodoItemMinimalFields() throws {
        let json = todoJSON()
        let data = json.data(using: .utf8)!
        let todo = try TodoItem.decode(from: data)
        #expect(todo.description == nil)
        #expect(todo.dueDate == nil)
        #expect(todo.context == nil)
        #expect(todo.sourceCardId == nil)
        #expect(todo.snoozedUntil == nil)
        #expect(todo.parentId == nil)
    }

    @Test("TodoItem with all optional fields populated decodes correctly")
    func decodeTodoItemAllFields() throws {
        let json = todoJSON(
            dueDate: "2026-03-15T10:00:00Z",
            description: "A description",
            context: "Some context",
            sourceCardId: "11111111-1111-1111-1111-111111111111",
            snoozedUntil: "2026-03-02T14:00:00Z",
            parentId: "22222222-2222-2222-2222-222222222222"
        )
        let data = json.data(using: .utf8)!
        let todo = try TodoItem.decode(from: data)
        #expect(todo.description == "A description")
        #expect(todo.dueDate != nil)
        #expect(todo.context == "Some context")
        #expect(todo.sourceCardId == UUID(uuidString: "11111111-1111-1111-1111-111111111111")!)
        #expect(todo.snoozedUntil != nil)
        #expect(todo.parentId == UUID(uuidString: "22222222-2222-2222-2222-222222222222")!)
    }

    // MARK: - isOverdue

    @Test("TodoItem isOverdue returns true when due date is in the past and not completed")
    func isOverduePastDate() {
        let pastDate = Calendar.current.date(byAdding: .day, value: -1, to: Date())!
        let todo = TodoItem(title: "Overdue", status: .created, dueDate: pastDate)
        #expect(todo.isOverdue == true)
    }

    @Test("TodoItem isOverdue returns false when completed even if past due")
    func isOverdueCompleted() {
        let pastDate = Calendar.current.date(byAdding: .day, value: -1, to: Date())!
        let todo = TodoItem(title: "Done", status: .completed, dueDate: pastDate)
        #expect(todo.isOverdue == false)
    }

    @Test("TodoItem isOverdue returns false when due date is in the future")
    func isOverdueFutureDate() {
        let futureDate = Calendar.current.date(byAdding: .day, value: 7, to: Date())!
        let todo = TodoItem(title: "Not yet", status: .created, dueDate: futureDate)
        #expect(todo.isOverdue == false)
    }

    @Test("TodoItem isOverdue returns false when no due date")
    func isOverdueNoDueDate() {
        let todo = TodoItem(title: "No date")
        #expect(todo.isOverdue == false)
    }

    // MARK: - Hashable / Identifiable

    @Test("TodoItem equality based on all fields")
    func todoItemEquality() {
        let id = UUID()
        let date = Date()
        let a = TodoItem(id: id, title: "Same", createdAt: date, updatedAt: date)
        let b = TodoItem(id: id, title: "Same", createdAt: date, updatedAt: date)
        #expect(a == b)
    }

    // MARK: - Sample Data

    @Test("TodoItem samples are non-empty")
    func sampleDataExists() {
        #expect(!TodoItem.samples.isEmpty)
        #expect(TodoItem.samples.count == 10)
    }

    @Test("TodoItem samples have unique IDs")
    func sampleDataUniqueIds() {
        let ids = Set(TodoItem.samples.map(\.id))
        #expect(ids.count == TodoItem.samples.count)
    }
}
