import SwiftUI

/// Month grid view with event dots. Tap a date to navigate to day view.
struct CalendarMonthView: View {
    var viewModel: CalendarViewModel
    let onDateSelected: (Date) -> Void

    private let calendar = Calendar.current
    private let columns = Array(repeating: GridItem(.flexible(), spacing: 0), count: 7)
    private let weekdaySymbols = Calendar.current.veryShortWeekdaySymbols

    var body: some View {
        VStack(spacing: 0) {
            monthHeader
            weekdayHeader
            monthGrid
        }
    }

    // MARK: - Month Header

    private var monthHeader: some View {
        HStack {
            Button {
                withAnimation(.easeInOut(duration: 0.25)) {
                    viewModel.displayedMonth = calendar.date(byAdding: .month, value: -1, to: viewModel.displayedMonth)!
                }
            } label: {
                Image(systemName: "chevron.left")
                    .font(.body.bold())
            }

            Spacer()

            Button {
                withAnimation(.easeInOut(duration: 0.25)) {
                    viewModel.displayedMonth = calendar.startOfDay(for: Date())
                }
            } label: {
                Text(monthYearText)
                    .font(.headline)
            }
            .tint(.primary)

            Spacer()

            Button {
                withAnimation(.easeInOut(duration: 0.25)) {
                    viewModel.displayedMonth = calendar.date(byAdding: .month, value: 1, to: viewModel.displayedMonth)!
                }
            } label: {
                Image(systemName: "chevron.right")
                    .font(.body.bold())
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }

    // MARK: - Weekday Header

    private var weekdayHeader: some View {
        LazyVGrid(columns: columns, spacing: 0) {
            ForEach(weekdaySymbols, id: \.self) { symbol in
                Text(symbol)
                    .font(.caption2.bold())
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 4)
            }
        }
        .padding(.horizontal, 8)
    }

    // MARK: - Month Grid

    private var monthGrid: some View {
        let days = daysInMonth()
        return LazyVGrid(columns: columns, spacing: 2) {
            ForEach(days, id: \.self) { day in
                if let day {
                    dayCell(for: day)
                } else {
                    Color.clear
                        .frame(height: 48)
                }
            }
        }
        .padding(.horizontal, 8)
        .onChange(of: viewModel.displayedMonth) { _, newMonth in
            Task { await viewModel.loadMonth(around: newMonth) }
        }
        .task {
            await viewModel.loadMonth(around: viewModel.displayedMonth)
        }
    }

    // MARK: - Day Cell

    private func dayCell(for date: Date) -> some View {
        let isToday = calendar.isDateInToday(date)
        let isSelected = calendar.isDate(date, inSameDayAs: viewModel.selectedDate)
        let events = viewModel.events(for: date)
        let eventColors = Array(Set(events.prefix(3).map { $0.colorId ?? "default" }))

        return Button {
            onDateSelected(date)
        } label: {
            VStack(spacing: 2) {
                Text("\(calendar.component(.day, from: date))")
                    .font(.callout)
                    .fontWeight(isToday ? .bold : .regular)
                    .foregroundStyle(isSelected ? .white : isToday ? .accentColor : .primary)
                    .frame(width: 32, height: 32)
                    .background {
                        if isSelected {
                            Circle().fill(.accent)
                        } else if isToday {
                            Circle().strokeBorder(.accent, lineWidth: 1.5)
                        }
                    }

                // Event dots
                HStack(spacing: 3) {
                    ForEach(Array(eventColors.prefix(3).enumerated()), id: \.offset) { _, colorId in
                        Circle()
                            .fill(eventDotColor(colorId))
                            .frame(width: 5, height: 5)
                    }
                }
                .frame(height: 6)
                .opacity(events.isEmpty ? 0 : 1)
            }
            .frame(height: 48)
            .frame(maxWidth: .infinity)
        }
        .buttonStyle(.plain)
    }

    // MARK: - Helpers

    private var monthYearText: String {
        let formatter = DateFormatter()
        formatter.dateFormat = "MMMM yyyy"
        return formatter.string(from: viewModel.displayedMonth)
    }

    /// Generate day cells for the month grid (nil = empty leading cells).
    private func daysInMonth() -> [Date?] {
        let month = viewModel.displayedMonth
        guard let range = calendar.range(of: .day, in: .month, for: month),
              let firstOfMonth = calendar.date(from: calendar.dateComponents([.year, .month], from: month)) else {
            return []
        }

        let firstWeekday = calendar.component(.weekday, from: firstOfMonth)
        let leadingBlanks = firstWeekday - calendar.firstWeekday
        let adjustedBlanks = leadingBlanks < 0 ? leadingBlanks + 7 : leadingBlanks

        var days: [Date?] = Array(repeating: nil, count: adjustedBlanks)
        for day in range {
            if let date = calendar.date(byAdding: .day, value: day - 1, to: firstOfMonth) {
                days.append(date)
            }
        }
        return days
    }

    private func eventDotColor(_ colorId: String) -> Color {
        switch colorId {
        case "1": return Color(red: 0.47, green: 0.53, blue: 0.87)
        case "2": return Color(red: 0.13, green: 0.67, blue: 0.53)
        case "3": return Color(red: 0.54, green: 0.33, blue: 0.71)
        case "4": return Color(red: 0.91, green: 0.42, blue: 0.48)
        case "5": return Color(red: 0.94, green: 0.72, blue: 0.20)
        case "6": return Color(red: 0.94, green: 0.60, blue: 0.22)
        case "7": return Color(red: 0.02, green: 0.65, blue: 0.72)
        case "9": return Color(red: 0.26, green: 0.52, blue: 0.96)
        case "11": return Color(red: 0.86, green: 0.27, blue: 0.22)
        default: return .blue
        }
    }
}

#Preview {
    CalendarMonthView(
        viewModel: CalendarViewModel(),
        onDateSelected: { _ in }
    )
}
