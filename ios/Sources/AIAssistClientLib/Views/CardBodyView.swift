import SwiftUI

// MARK: - Card Body Dispatcher

/// Routes to the correct body view based on card type.
/// Used inside SwipeCardContainer as the card's main content.
struct CardBodyView: View {
    let card: ApprovalCard

    var body: some View {
        switch card.payload {
        case .reply:
            ReplyCardBody(card: card)
        case .compose:
            ComposeCardBody(card: card)
        case .action:
            ActionCardBody(card: card)
        case .decision:
            DecisionCardBody(card: card)
        }
    }
}

// MARK: - Reply Card Body

/// The existing reply card layout: channel header → message thread → draft bubble.
/// This is the same layout that was in ContentView, relocated here.
struct ReplyCardBody: View {
    let card: ApprovalCard

    var body: some View {
        VStack(spacing: 0) {
            cardHeader
            MessageThreadView(card: card)
        }
    }

    private var cardHeader: some View {
        HStack(spacing: 10) {
            Image(systemName: channelIcon(for: card.channel))
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(.white)

            Text(card.sourceSender)
                .font(.caption.bold())
                .foregroundStyle(.white)
                .lineLimit(1)

            Text("·")
                .font(.caption)
                .foregroundStyle(.white.opacity(0.6))

            Text(channelLabel(for: card))
                .font(.caption)
                .foregroundStyle(.white.opacity(0.8))
                .lineLimit(1)

            Spacer()
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .background(channelColor(for: card.channel))
        .clipShape(
            UnevenRoundedRectangle(
                topLeadingRadius: 20,
                bottomLeadingRadius: 0,
                bottomTrailingRadius: 0,
                topTrailingRadius: 20
            )
        )
    }
}

// MARK: - Action Card Body

/// Shows a tool/action name with optional parameters detail.
struct ActionCardBody: View {
    let card: ApprovalCard

    private var description: String {
        if case .action(let desc, _) = card.payload { return desc }
        return ""
    }

    private var actionDetail: String? {
        if case .action(_, let detail) = card.payload { return detail }
        return nil
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            // Header
            HStack(spacing: 8) {
                Image(systemName: "bolt.circle.fill")
                    .font(.system(size: 20))
                    .foregroundStyle(.orange)
                Text("Action")
                    .font(.headline)
                    .foregroundStyle(.primary)
                Spacer()
                siloTag
            }

            // Description
            Text(description)
                .font(.body)
                .foregroundStyle(.primary)

            // Detail (collapsible JSON/params)
            if let detail = actionDetail, !detail.isEmpty {
                Text(detail)
                    .font(.system(size: 12, design: .monospaced))
                    .foregroundStyle(.secondary)
                    .padding(10)
                    #if os(iOS)
                    .background(Color(uiColor: .systemGray6))
                    #else
                    .background(Color.gray.opacity(0.08))
                    #endif
                    .clipShape(RoundedRectangle(cornerRadius: 8))
            }
        }
        .padding(16)
    }

    private var siloTag: some View {
        Text(card.silo.rawValue.capitalized)
            .font(.system(size: 10, weight: .semibold))
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(Color.orange.opacity(0.15))
            .foregroundStyle(.orange)
            .clipShape(Capsule())
    }
}

// MARK: - Compose Card Body

/// Shows a draft message to a recipient with optional subject line.
struct ComposeCardBody: View {
    let card: ApprovalCard

    private var recipient: String {
        if case .compose(_, let r, _, _, _) = card.payload { return r }
        return ""
    }

    private var subject: String? {
        if case .compose(_, _, let s, _, _) = card.payload { return s }
        return nil
    }

    private var draftBody: String {
        if case .compose(_, _, _, let body, _) = card.payload { return body }
        return ""
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            // Header
            HStack(spacing: 8) {
                Image(systemName: channelIcon(for: card.channel))
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(.white)
                Text("New Message")
                    .font(.headline)
                    .foregroundStyle(.white)
                Spacer()
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .background(channelColor(for: card.channel))
            .clipShape(
                UnevenRoundedRectangle(
                    topLeadingRadius: 20,
                    bottomLeadingRadius: 0,
                    bottomTrailingRadius: 0,
                    topTrailingRadius: 20
                )
            )

            VStack(alignment: .leading, spacing: 8) {
                // To
                HStack(spacing: 4) {
                    Text("To:")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Text(recipient)
                        .font(.caption.bold())
                        .foregroundStyle(.primary)
                }

                // Subject
                if let subject, !subject.isEmpty {
                    HStack(spacing: 4) {
                        Text("Subject:")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        Text(subject)
                            .font(.caption)
                            .foregroundStyle(.primary)
                    }
                }

                Divider()

                // Draft body
                Text(draftBody)
                    .font(.body)
                    .foregroundStyle(.primary)
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 16)
        }
    }
}

// MARK: - Decision Card Body

/// Shows a question with context and option pills.
struct DecisionCardBody: View {
    let card: ApprovalCard

    private var question: String {
        if case .decision(let q, _, _) = card.payload { return q }
        return ""
    }

    private var context: String {
        if case .decision(_, let c, _) = card.payload { return c }
        return ""
    }

    private var options: [String] {
        if case .decision(_, _, let opts) = card.payload { return opts }
        return []
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            // Header
            HStack(spacing: 8) {
                Image(systemName: "questionmark.circle.fill")
                    .font(.system(size: 20))
                    .foregroundStyle(.purple)
                Text("Decision")
                    .font(.headline)
                    .foregroundStyle(.primary)
                Spacer()
            }

            // Question
            Text(question)
                .font(.body)
                .fontWeight(.medium)
                .foregroundStyle(.primary)

            // Context
            if !context.isEmpty {
                Text(context)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }

            // Options
            if !options.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(options, id: \.self) { option in
                        HStack(spacing: 8) {
                            Circle()
                                .fill(.purple.opacity(0.3))
                                .frame(width: 6, height: 6)
                            Text(option)
                                .font(.subheadline)
                                .foregroundStyle(.primary)
                        }
                    }
                }
            }
        }
        .padding(16)
    }
}
