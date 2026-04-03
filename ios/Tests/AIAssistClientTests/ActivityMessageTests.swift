import Foundation
import Testing
@testable import AIAssistClientLib

@Suite("ActivityMessage Tests")
struct ActivityMessageTests {

    private let jobId = "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
    private let todoId = "b2c3d4e5-f6a7-8901-bcde-f12345678901"
    private let cardId = "c3d4e5f6-a7b8-9012-cdef-123456789012"

    // MARK: - Decoding all variants

    @Test("Decode started message")
    func decodeStarted() throws {
        let json = """
        {"type": "started", "job_id": "\(jobId)", "todo_id": "\(todoId)"}
        """
        let msg = try ActivityMessage.decode(from: json.data(using: .utf8)!)
        guard case .started(let jid, let tid) = msg else {
            Issue.record("Expected started, got \(msg)")
            return
        }
        #expect(jid == UUID(uuidString: jobId)!)
        #expect(tid == UUID(uuidString: todoId)!)
    }

    @Test("Decode started message without todoId")
    func decodeStartedNoTodo() throws {
        let json = """
        {"type": "started", "job_id": "\(jobId)"}
        """
        let msg = try ActivityMessage.decode(from: json.data(using: .utf8)!)
        guard case .started(_, let tid) = msg else {
            Issue.record("Expected started")
            return
        }
        #expect(tid == nil)
    }

    @Test("Decode thinking message")
    func decodeThinking() throws {
        let json = """
        {"type": "thinking", "job_id": "\(jobId)", "iteration": 3}
        """
        let msg = try ActivityMessage.decode(from: json.data(using: .utf8)!)
        guard case .thinking(_, let iteration) = msg else {
            Issue.record("Expected thinking")
            return
        }
        #expect(iteration == 3)
    }

    @Test("Decode tool_completed message")
    func decodeToolCompleted() throws {
        let json = """
        {"type": "tool_completed", "job_id": "\(jobId)", "tool_name": "shell", "success": true, "summary": "Ran ls"}
        """
        let msg = try ActivityMessage.decode(from: json.data(using: .utf8)!)
        guard case .toolCompleted(_, let toolName, let success, let summary) = msg else {
            Issue.record("Expected toolCompleted")
            return
        }
        #expect(toolName == "shell")
        #expect(success == true)
        #expect(summary == "Ran ls")
    }

    @Test("Decode reasoning message")
    func decodeReasoning() throws {
        let json = """
        {"type": "reasoning", "job_id": "\(jobId)", "content": "Analyzing the data..."}
        """
        let msg = try ActivityMessage.decode(from: json.data(using: .utf8)!)
        guard case .reasoning(_, let content) = msg else {
            Issue.record("Expected reasoning")
            return
        }
        #expect(content == "Analyzing the data...")
    }

    @Test("Decode agent_response message")
    func decodeAgentResponse() throws {
        let json = """
        {"type": "agent_response", "job_id": "\(jobId)", "content": "Here are the results."}
        """
        let msg = try ActivityMessage.decode(from: json.data(using: .utf8)!)
        guard case .agentResponse(_, let content) = msg else {
            Issue.record("Expected agentResponse")
            return
        }
        #expect(content == "Here are the results.")
    }

    @Test("Decode completed message")
    func decodeCompleted() throws {
        let json = """
        {"type": "completed", "job_id": "\(jobId)", "summary": "Task done successfully"}
        """
        let msg = try ActivityMessage.decode(from: json.data(using: .utf8)!)
        guard case .completed(_, let summary) = msg else {
            Issue.record("Expected completed")
            return
        }
        #expect(summary == "Task done successfully")
    }

    @Test("Decode failed message")
    func decodeFailed() throws {
        let json = """
        {"type": "failed", "job_id": "\(jobId)", "error": "Connection timeout"}
        """
        let msg = try ActivityMessage.decode(from: json.data(using: .utf8)!)
        guard case .failed(_, let error) = msg else {
            Issue.record("Expected failed")
            return
        }
        #expect(error == "Connection timeout")
    }

    @Test("Decode transcript message")
    func decodeTranscript() throws {
        let json = """
        {
            "type": "transcript",
            "job_id": "\(jobId)",
            "messages": [
                {"role": "user", "content": "Do the thing"},
                {"role": "assistant", "content": "Done", "tool_name": "shell", "tool_args": "ls -la"}
            ]
        }
        """
        let msg = try ActivityMessage.decode(from: json.data(using: .utf8)!)
        guard case .transcript(_, let messages) = msg else {
            Issue.record("Expected transcript")
            return
        }
        #expect(messages.count == 2)
        #expect(messages[0].role == "user")
        #expect(messages[0].toolName == nil)
        #expect(messages[1].role == "assistant")
        #expect(messages[1].toolName == "shell")
        #expect(messages[1].toolArgs == "ls -la")
    }

    @Test("Decode approval_needed message")
    func decodeApprovalNeeded() throws {
        let json = """
        {
            "type": "approval_needed",
            "job_id": "\(jobId)",
            "card_id": "\(cardId)",
            "tool_name": "send_email",
            "description": "Send reply to Alice"
        }
        """
        let msg = try ActivityMessage.decode(from: json.data(using: .utf8)!)
        guard case .approvalNeeded(_, let cid, let toolName, let desc) = msg else {
            Issue.record("Expected approvalNeeded")
            return
        }
        #expect(cid == UUID(uuidString: cardId)!)
        #expect(toolName == "send_email")
        #expect(desc == "Send reply to Alice")
    }

    @Test("Decode approval_resolved message")
    func decodeApprovalResolved() throws {
        let json = """
        {"type": "approval_resolved", "job_id": "\(jobId)", "card_id": "\(cardId)", "approved": false}
        """
        let msg = try ActivityMessage.decode(from: json.data(using: .utf8)!)
        guard case .approvalResolved(_, let cid, let approved) = msg else {
            Issue.record("Expected approvalResolved")
            return
        }
        #expect(cid == UUID(uuidString: cardId)!)
        #expect(approved == false)
    }

    @Test("Decode user_message")
    func decodeUserMessage() throws {
        let json = """
        {"type": "user_message", "todo_id": "\(todoId)", "content": "Please prioritize this"}
        """
        let msg = try ActivityMessage.decode(from: json.data(using: .utf8)!)
        guard case .userMessage(let tid, let content) = msg else {
            Issue.record("Expected userMessage")
            return
        }
        #expect(tid == UUID(uuidString: todoId)!)
        #expect(content == "Please prioritize this")
    }

    @Test("Unknown type throws DecodingError")
    func decodeUnknownType() {
        let json = """
        {"type": "some_future_type", "job_id": "\(jobId)"}
        """
        #expect(throws: DecodingError.self) {
            _ = try ActivityMessage.decode(from: json.data(using: .utf8)!)
        }
    }

    // MARK: - Decode array

    @Test("Decode ActivityMessage array")
    func decodeArray() throws {
        let json = """
        [
            {"type": "started", "job_id": "\(jobId)"},
            {"type": "completed", "job_id": "\(jobId)", "summary": "Done"}
        ]
        """
        let messages = try ActivityMessage.decodeArray(from: json.data(using: .utf8)!)
        #expect(messages.count == 2)
    }

    // MARK: - Computed Properties

    @Test("jobId returns correct UUID for all variants")
    func jobIdProperty() throws {
        let uuid = UUID(uuidString: jobId)!
        let startedMsg = try ActivityMessage.decode(from:
            "{\"type\":\"started\",\"job_id\":\"\(jobId)\"}".data(using: .utf8)!)
        #expect(startedMsg.jobId == uuid)
    }

    @Test("jobId returns zero UUID for userMessage")
    func jobIdUserMessage() throws {
        let msg = try ActivityMessage.decode(from:
            "{\"type\":\"user_message\",\"todo_id\":\"\(todoId)\",\"content\":\"hi\"}".data(using: .utf8)!)
        #expect(msg.jobId == UUID(uuidString: "00000000-0000-0000-0000-000000000000")!)
    }

    @Test("isTerminal returns true only for completed, failed, transcript")
    func isTerminalProperty() throws {
        let terminal: [(String, Bool)] = [
            ("{\"type\":\"started\",\"job_id\":\"\(jobId)\"}", false),
            ("{\"type\":\"thinking\",\"job_id\":\"\(jobId)\",\"iteration\":1}", false),
            ("{\"type\":\"completed\",\"job_id\":\"\(jobId)\",\"summary\":\"ok\"}", true),
            ("{\"type\":\"failed\",\"job_id\":\"\(jobId)\",\"error\":\"err\"}", true),
            ("{\"type\":\"transcript\",\"job_id\":\"\(jobId)\",\"messages\":[]}", true),
        ]
        for (json, expected) in terminal {
            let msg = try ActivityMessage.decode(from: json.data(using: .utf8)!)
            #expect(msg.isTerminal == expected, "isTerminal mismatch for \(json)")
        }
    }

    // MARK: - Identity

    @Test("id is unique across different variant types")
    func uniqueIds() throws {
        let messages: [ActivityMessage] = try [
            "{\"type\":\"started\",\"job_id\":\"\(jobId)\"}",
            "{\"type\":\"thinking\",\"job_id\":\"\(jobId)\",\"iteration\":1}",
            "{\"type\":\"completed\",\"job_id\":\"\(jobId)\",\"summary\":\"done\"}",
            "{\"type\":\"failed\",\"job_id\":\"\(jobId)\",\"error\":\"err\"}",
        ].map { try ActivityMessage.decode(from: $0.data(using: .utf8)!) }

        let ids = Set(messages.map(\.id))
        #expect(ids.count == messages.count)
    }

    @Test("thinking messages with different iterations have different ids")
    func thinkingUniqueByIteration() throws {
        let m1 = try ActivityMessage.decode(from:
            "{\"type\":\"thinking\",\"job_id\":\"\(jobId)\",\"iteration\":1}".data(using: .utf8)!)
        let m2 = try ActivityMessage.decode(from:
            "{\"type\":\"thinking\",\"job_id\":\"\(jobId)\",\"iteration\":2}".data(using: .utf8)!)
        #expect(m1.id != m2.id)
    }

    // MARK: - TranscriptMessage

    @Test("TranscriptMessage id is stable")
    func transcriptMessageId() {
        let tm = TranscriptMessage(role: "user", content: "hello", toolName: nil, toolArgs: nil)
        let id1 = tm.id
        let id2 = tm.id
        #expect(id1 == id2)
    }

    // MARK: - Encode/Decode roundtrip

    @Test("ActivityMessage encode then decode round-trips")
    func encodeDecode() throws {
        let original = try ActivityMessage.decode(from:
            "{\"type\":\"tool_completed\",\"job_id\":\"\(jobId)\",\"tool_name\":\"shell\",\"success\":true,\"summary\":\"ok\"}"
            .data(using: .utf8)!)

        let encoder = JSONEncoder()
        let data = try encoder.encode(original)
        let decoded = try JSONDecoder().decode(ActivityMessage.self, from: data)

        #expect(decoded.id == original.id)
        #expect(decoded.jobId == original.jobId)
    }
}
