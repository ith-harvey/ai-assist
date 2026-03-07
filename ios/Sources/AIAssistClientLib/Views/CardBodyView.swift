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
        case .multipleChoice:
            // Rendered via MultipleChoiceCardBody in ContentView directly
            EmptyView()
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
    @State private var isDetailExpanded = false

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
                VStack(alignment: .leading, spacing: 6) {
                    HStack(spacing: 4) {
                        Image(systemName: isDetailExpanded ? "chevron.down" : "chevron.right")
                            .font(.system(size: 10))
                            .foregroundStyle(.tertiary)
                        Text("Raw JSON")
                            .font(.system(size: 11, weight: .medium))
                            .foregroundStyle(.tertiary)
                        Spacer()
                    }
                    .contentShape(Rectangle())
                    .onTapGesture {
                        withAnimation(.spring(response: 0.25, dampingFraction: 0.8)) {
                            isDetailExpanded.toggle()
                        }
                    }

                    if isDetailExpanded {
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
                            .transition(.asymmetric(
                                insertion: .opacity.combined(with: .move(edge: .top)),
                                removal: .opacity
                            ))
                    }
                }
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

// MARK: - Multiple Choice Card Body

/// Shows a question with swipeable A/B/C option rows.
/// Each option can be swiped right to select it.
struct MultipleChoiceCardBody: View {
    let card: ApprovalCard
    let socket: CardWebSocket

    private var question: String {
        if case .multipleChoice(let q, _) = card.payload { return q }
        return ""
    }

    private var options: [String] {
        if case .multipleChoice(_, let opts) = card.payload { return opts }
        return []
    }

    private let labels = ["A", "B", "C"]

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            // Header
            HStack(spacing: 8) {
                Image(systemName: "list.bullet.circle.fill")
                    .font(.system(size: 20))
                    .foregroundStyle(.blue)
                Text("Question")
                    .font(.headline)
                    .foregroundStyle(.primary)
                Spacer()
            }

            // Question
            Text(question)
                .font(.body)
                .fontWeight(.medium)
                .foregroundStyle(.primary)

            // Swipeable options
            VStack(spacing: 8) {
                ForEach(Array(options.enumerated()), id: \.offset) { index, option in
                    SwipeOptionRow(
                        label: labels[index],
                        text: option,
                        onSelect: {
                            socket.selectOption(cardId: card.id, selectedIndex: index)
                        }
                    )
                }
            }
        }
        .padding(16)
    }
}

// MARK: - Swipeable Option Row

/// A single multiple-choice option that can be swiped right to select.
struct SwipeOptionRow: View {
    let label: String
    let text: String
    let onSelect: () -> Void

    @State private var dragOffset: CGFloat = 0
    @State private var isDragging = false

    private let swipeThreshold: CGFloat = 80

    var body: some View {
        ZStack {
            // Background reveal on swipe
            HStack {
                if dragOffset > 10 {
                    HStack(spacing: 4) {
                        Image(systemName: "checkmark.circle.fill")
                            .font(.system(size: 14, weight: .bold))
                        Text("Confirmed")
                            .font(.caption.bold())
                    }
                    .foregroundStyle(.white)
                    .padding(.leading, 12)
                    .opacity(min(1.0, Double(dragOffset - 10) / 60))
                }
                Spacer()
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Color.green.opacity(min(0.85, Double(max(0, dragOffset - 10)) / 150)))
            .clipShape(RoundedRectangle(cornerRadius: 12))

            // Option pill
            HStack(spacing: 10) {
                Text(label)
                    .font(.system(size: 14, weight: .bold, design: .rounded))
                    .foregroundStyle(.white)
                    .frame(width: 28, height: 28)
                    .background(Circle().fill(.blue))

                Text(text)
                    .font(.subheadline)
                    .foregroundStyle(.primary)
                    .lineLimit(2)

                Spacer()

                Image(systemName: "chevron.right")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(.tertiary)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
            .background(
                RoundedRectangle(cornerRadius: 12)
                    #if os(iOS)
                    .fill(Color(uiColor: .secondarySystemBackground))
                    #else
                    .fill(Color.gray.opacity(0.08))
                    #endif
            )
            .offset(x: dragOffset)
        }
        .gesture(
            DragGesture(minimumDistance: 15)
                .onChanged { value in
                    let horizontal = abs(value.translation.width)
                    let vertical = abs(value.translation.height)

                    if !isDragging && horizontal > vertical && horizontal > 15 {
                        isDragging = true
                    }

                    if isDragging {
                        // Only allow right swipe (positive)
                        dragOffset = max(0, value.translation.width)
                    }
                }
                .onEnded { value in
                    guard isDragging else {
                        isDragging = false
                        return
                    }

                    let width = value.translation.width
                    let velocityX = value.predictedEndTranslation.width - width
                    let effectiveWidth = width + velocityX * 0.15

                    if effectiveWidth > swipeThreshold {
                        withAnimation(.easeOut(duration: 0.18)) {
                            dragOffset = 400
                        }
                        DispatchQueue.main.asyncAfter(deadline: .now() + 0.18) {
                            onSelect()
                        }
                    } else {
                        withAnimation(.spring(response: 0.3, dampingFraction: 0.7)) {
                            dragOffset = 0
                        }
                    }
                    isDragging = false
                }
        )
    }
}
