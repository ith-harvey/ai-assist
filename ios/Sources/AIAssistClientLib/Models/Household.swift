import Foundation

// MARK: - Household

/// A household — the top-level grouping for family/shared task management.
public struct Household: Identifiable, Hashable, Codable, Sendable {
    public let id: UUID
    public var name: String
    public let createdBy: String
    public let createdAt: Date
    public var updatedAt: Date

    public init(
        id: UUID = UUID(),
        name: String,
        createdBy: String,
        createdAt: Date = Date(),
        updatedAt: Date = Date()
    ) {
        self.id = id
        self.name = name
        self.createdBy = createdBy
        self.createdAt = createdAt
        self.updatedAt = updatedAt
    }

    public static func decode(from data: Data) throws -> Household {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        decoder.dateDecodingStrategy = .iso8601
        return try decoder.decode(Household.self, from: data)
    }

    public static func decodeArray(from data: Data) throws -> [Household] {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        decoder.dateDecodingStrategy = .iso8601
        return try decoder.decode([Household].self, from: data)
    }
}

// MARK: - Household Role

/// Role of a member within a household.
public enum HouseholdRole: String, Codable, Sendable, CaseIterable, Identifiable {
    case owner
    case admin
    case member

    public var id: String { rawValue }

    public var label: String {
        switch self {
        case .owner: "Owner"
        case .admin: "Admin"
        case .member: "Member"
        }
    }
}

// MARK: - Household Member

/// A member of a household.
public struct HouseholdMember: Identifiable, Hashable, Codable, Sendable {
    public let id: UUID
    public let householdId: UUID
    public let userId: String
    public var displayName: String
    public var role: HouseholdRole
    public let joinedAt: Date

    public init(
        id: UUID = UUID(),
        householdId: UUID,
        userId: String,
        displayName: String,
        role: HouseholdRole = .member,
        joinedAt: Date = Date()
    ) {
        self.id = id
        self.householdId = householdId
        self.userId = userId
        self.displayName = displayName
        self.role = role
        self.joinedAt = joinedAt
    }

    /// Initials derived from the display name (first letter of each word, max 2).
    public var initials: String {
        let parts = displayName.split(separator: " ")
        let letters = parts.prefix(2).compactMap { $0.first }
        return String(letters).uppercased()
    }

    public static func decodeArray(from data: Data) throws -> [HouseholdMember] {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        decoder.dateDecodingStrategy = .iso8601
        return try decoder.decode([HouseholdMember].self, from: data)
    }
}

// MARK: - Sample Data

extension Household {
    public static let sample = Household(
        name: "Smith Family",
        createdBy: "mom"
    )
}

extension HouseholdMember {
    public static let samples: [HouseholdMember] = {
        let householdId = Household.sample.id
        return [
            HouseholdMember(householdId: householdId, userId: "mom", displayName: "Mom", role: .owner),
            HouseholdMember(householdId: householdId, userId: "dad", displayName: "Dad", role: .admin),
            HouseholdMember(householdId: householdId, userId: "emma", displayName: "Emma", role: .member),
            HouseholdMember(householdId: householdId, userId: "jake", displayName: "Jake", role: .member),
        ]
    }()
}
