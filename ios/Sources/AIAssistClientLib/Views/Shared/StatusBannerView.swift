import SwiftUI

/// A colored banner with icon, title, and optional summary text.
/// Used for completed, failed, and transcript banners in TodoDetailView.
struct StatusBannerView: View {
    let icon: String
    let title: String
    var summary: String = ""
    let color: Color
    var summaryLineLimit: Int? = 3

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Image(systemName: icon)
                    .font(.system(size: 20))
                    .foregroundStyle(color)
                Text(title)
                    .font(.headline)
                    .foregroundStyle(color)
            }
            if !summary.isEmpty {
                Text(summary)
                    .font(.subheadline)
                    .foregroundStyle(summary == title ? .secondary : .primary)
                    .lineLimit(summaryLineLimit)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(14)
        .background(
            RoundedRectangle(cornerRadius: 14)
                .fill(color.opacity(0.1))
                .overlay(
                    RoundedRectangle(cornerRadius: 14)
                        .strokeBorder(color.opacity(0.3), lineWidth: 1)
                )
        )
        .padding(.top, 4)
    }
}
