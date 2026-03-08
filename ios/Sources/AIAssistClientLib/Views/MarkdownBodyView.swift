import SwiftUI

/// Basic markdown renderer using SwiftUI's built-in AttributedString.
///
/// Handles:
/// - Inline markdown (bold, italic, links, code) via AttributedString
/// - Code blocks (triple-backtick) rendered in monospaced font with background
/// - Headers (# lines) rendered with appropriate font sizes
///
/// No external packages required.
struct MarkdownBodyView: View {
    let content: String

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            ForEach(Array(parseBlocks(content).enumerated()), id: \.offset) { _, block in
                switch block {
                case .heading(let level, let text):
                    headingView(level: level, text: text)
                case .codeBlock(let language, let code):
                    codeBlockView(language: language, code: code)
                case .paragraph(let text):
                    paragraphView(text: text)
                }
            }
        }
    }

    // MARK: - Block Types

    private enum Block {
        case heading(level: Int, text: String)
        case codeBlock(language: String?, code: String)
        case paragraph(text: String)
    }

    // MARK: - Parser

    /// Split markdown content into typed blocks.
    private func parseBlocks(_ markdown: String) -> [Block] {
        var blocks: [Block] = []
        let lines = markdown.components(separatedBy: "\n")
        var i = 0

        while i < lines.count {
            let line = lines[i]

            // Code block: ```
            if line.trimmingCharacters(in: .whitespaces).hasPrefix("```") {
                let lang = String(line.trimmingCharacters(in: .whitespaces).dropFirst(3))
                    .trimmingCharacters(in: .whitespaces)
                var codeLines: [String] = []
                i += 1
                while i < lines.count {
                    if lines[i].trimmingCharacters(in: .whitespaces).hasPrefix("```") {
                        i += 1
                        break
                    }
                    codeLines.append(lines[i])
                    i += 1
                }
                blocks.append(.codeBlock(
                    language: lang.isEmpty ? nil : lang,
                    code: codeLines.joined(separator: "\n")
                ))
                continue
            }

            // Heading: # ## ### etc.
            if let headingMatch = parseHeading(line) {
                blocks.append(.heading(level: headingMatch.0, text: headingMatch.1))
                i += 1
                continue
            }

            // Blank line — skip
            if line.trimmingCharacters(in: .whitespaces).isEmpty {
                i += 1
                continue
            }

            // Paragraph: collect consecutive non-special lines
            var paraLines: [String] = [line]
            i += 1
            while i < lines.count {
                let next = lines[i]
                if next.trimmingCharacters(in: .whitespaces).isEmpty
                    || next.trimmingCharacters(in: .whitespaces).hasPrefix("```")
                    || parseHeading(next) != nil {
                    break
                }
                paraLines.append(next)
                i += 1
            }
            blocks.append(.paragraph(text: paraLines.joined(separator: "\n")))
        }

        return blocks
    }

    /// Parse a heading line. Returns (level, text) or nil.
    private func parseHeading(_ line: String) -> (Int, String)? {
        let trimmed = line.trimmingCharacters(in: .whitespaces)
        guard trimmed.hasPrefix("#") else { return nil }

        var level = 0
        for ch in trimmed {
            if ch == "#" { level += 1 }
            else { break }
        }
        guard level >= 1, level <= 6 else { return nil }

        let text = String(trimmed.dropFirst(level)).trimmingCharacters(in: .whitespaces)
        guard !text.isEmpty else { return nil }
        return (level, text)
    }

    // MARK: - Rendering

    private func headingView(level: Int, text: String) -> some View {
        let font: Font = switch level {
        case 1: .title2.bold()
        case 2: .title3.bold()
        case 3: .headline
        default: .subheadline.bold()
        }
        return Text(renderInlineMarkdown(text))
            .font(font)
            .foregroundStyle(.primary)
            .padding(.top, level <= 2 ? 8 : 4)
    }

    @ViewBuilder
    private func codeBlockView(language: String?, code: String) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            if let lang = language {
                Text(lang)
                    .font(.system(size: 10, weight: .medium, design: .monospaced))
                    .foregroundStyle(.secondary)
            }
            Text(code)
                .font(.system(size: 12, design: .monospaced))
                .foregroundStyle(.primary)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(12)
        #if os(iOS)
        .background(Color(uiColor: .systemGray6))
        #else
        .background(Color.gray.opacity(0.1))
        #endif
        .clipShape(RoundedRectangle(cornerRadius: 8))
    }

    private func paragraphView(text: String) -> some View {
        Text(renderInlineMarkdown(text))
            .font(.body)
            .foregroundStyle(.primary)
    }

    /// Render inline markdown (bold, italic, code, links) via AttributedString.
    private func renderInlineMarkdown(_ text: String) -> AttributedString {
        if let attributed = try? AttributedString(markdown: text, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace)) {
            return attributed
        }
        return AttributedString(text)
    }
}
