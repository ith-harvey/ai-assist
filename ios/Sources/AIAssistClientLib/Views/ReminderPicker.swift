import SwiftUI

/// Picker for selecting a reminder interval when creating/editing events.
struct ReminderPicker: View {
    @Binding var selectedReminder: ReminderOption

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Label("Reminder", systemImage: "bell")
                .font(.subheadline.bold())
                .foregroundStyle(.primary)

            Picker("Reminder", selection: $selectedReminder) {
                ForEach(ReminderOption.allCases) { option in
                    Text(option.label).tag(option)
                }
            }
            .pickerStyle(.menu)
            .tint(.blue)
        }
    }
}

#Preview {
    @Previewable @State var reminder: ReminderOption = .fifteenMinutes
    ReminderPicker(selectedReminder: $reminder)
        .padding()
}
