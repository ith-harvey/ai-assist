import SwiftUI

/// Banner shown when a WebSocket is not connected.
/// Displays a spinner and "Connecting to host:port..." message.
struct ConnectionBannerView: View {
    let isConnected: Bool
    let host: String
    let port: Int

    var body: some View {
        if !isConnected {
            HStack(spacing: 6) {
                ProgressView()
                    .controlSize(.small)
                Text("Connecting to \(host):\(port)...")
                    .font(.caption)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 6)
            .background(Color.orange.opacity(0.15))
        }
    }
}
