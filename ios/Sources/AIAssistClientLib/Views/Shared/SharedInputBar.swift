import SwiftUI

/// Reusable text input bar with Telegram-style mic/send swap.
///
/// Used as the primary input mechanism across the app:
/// - Brain tab (AIInputBar wraps this with a status indicator)
/// - Card refinement (ContentView)
/// - Todo follow-up instructions (TodoDetailView)
///
/// The caller provides `onSend` for text submission and optionally
/// `onVoiceTranscript` for voice input. When `onVoiceTranscript` is nil,
/// the mic button is hidden and only the send button appears.
struct SharedInputBar: View {
    @Binding var text: String
    var placeholder: String = "Message..."
    var font: Font = .body
    var lineLimit: ClosedRange<Int> = 1...5
    var showBackground: Bool = true
    var sendIconSize: CGFloat = 30
    var canSend: Bool? = nil
    let onSend: () -> Void
    var onVoiceTranscript: ((String) -> Void)? = nil

    /// Whether the text field has non-empty content.
    private var hasText: Bool {
        !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    /// Resolved send-ability: uses override if provided, else checks for text.
    private var isSendable: Bool {
        canSend ?? hasText
    }

    private var showVoiceMic: Bool {
        onVoiceTranscript != nil
    }

    var body: some View {
        HStack(alignment: .bottom, spacing: 8) {
            TextField(placeholder, text: $text, axis: .vertical)
                .textFieldStyle(.plain)
                .font(font)
                .lineLimit(lineLimit)
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .secondaryFill()
                .clipShape(RoundedRectangle(cornerRadius: 18))
                .onSubmit {
                    guard isSendable else { return }
                    onSend()
                }

            // Send / mic swap
            if showVoiceMic {
                ZStack {
                    if isSendable {
                        sendButton
                            .transition(.scale.combined(with: .opacity))
                    } else {
                        #if os(iOS)
                        VoiceMicButton { transcript in
                            onVoiceTranscript?(transcript)
                        }
                        .zIndex(1)
                        .transition(.scale.combined(with: .opacity))
                        #else
                        disabledSendButton
                        #endif
                    }
                }
                .animation(.spring(response: 0.3, dampingFraction: 0.7), value: isSendable)
            } else if hasText {
                sendButton
            }
        }
        .padding(.horizontal, showBackground ? 12 : 12)
        .padding(.vertical, showBackground ? 8 : 10)
        .background(showBackground ? AnyShapeStyle(.bar) : AnyShapeStyle(.clear))
    }

    // MARK: - Subviews

    private var sendButton: some View {
        Button {
            guard isSendable else { return }
            onSend()
        } label: {
            Image(systemName: "arrow.up.circle.fill")
                .font(.system(size: sendIconSize))
                .foregroundStyle(.blue)
        }
    }

    private var disabledSendButton: some View {
        Button {} label: {
            Image(systemName: "arrow.up.circle.fill")
                .font(.system(size: sendIconSize))
                .foregroundStyle(.gray.opacity(0.4))
        }
        .disabled(true)
    }
}
