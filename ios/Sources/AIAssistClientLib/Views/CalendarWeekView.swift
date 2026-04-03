import SwiftUI

/// 7-day scrollable week view with compact event columns.
struct CalendarWeekView: View {
    let weekDates: [Date]
    let viewModel: CalendarViewModel

    /// Height per hour in the week timeline.
    private let hourHeight: CGFloat = 50
    /// Width of the time label column.
    private let timeColumnWidth: CGFloat = 44
    /// Total timeline height (24 hours).
    private var totalHeight: CGFloat { 24 * hourHeight }

    private let calendar = Calendar.current

    var body: some View {
        VStack(spacing: 0) {
            // Day column headers
            dayHeaders

            // Scrollable timeline grid
            ScrollViewReader { proxy in
                ScrollView(.vertical, showsIndicators: false) {
                    ZStack(alignment: .topLeading) {
                        hourGrid
                        eventColumns
                        currentTimeIndicator
                    }
                    .frame(height: totalHeight)
                }
                .onAppear {
                    let currentHour = max(0, calendar.component(.hour, from: Date()) - 1)
                    proxy.scrollTo(currentHour, anchor: .top)
                }
            }
        }
    }

    // MARK: - Day Headers

    private var dayHeaders: some View {
        HStack(spacing: 0) {
            // Spacer for time column
            Color.clear
                .frame(width: timeColumnWidth)

            ForEach(weekDates, id: \.self) { date in
                let isToday = calendar.isDateInToday(date)
                let isSelected = calendar.isDate(date, inSameDayAs: viewModel.selectedDate)

                Button {
                    withAnimation {
                        viewModel.navigateToDay(date)
                        viewModel.viewMode = .day
                    }
                } label: {
                    VStack(spacing: 2) {
                        Text(viewModel.shortDayLabel(for: date))
                            .font(.caption2)
                            .foregroundStyle(isToday ? .blue : .secondary)

                        Text(viewModel.dayNumber(for: date))
                            .font(.caption.bold())
                            .foregroundStyle(isToday ? .white : (isSelected ? .blue : .primary))
                            .frame(width: 28, height: 28)
                            .background {
                                if isToday {
                                    Circle().fill(.blue)
                                } else if isSelected {
                                    Circle().stroke(.blue, lineWidth: 1.5)
                                }
                            }
                    }
                    .frame(maxWidth: .infinity)
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.vertical, 8)
        .background(.ultraThinMaterial)
    }

    // MARK: - Hour Grid

    private var hourGrid: some View {
        VStack(spacing: 0) {
            ForEach(0..<24, id: \.self) { hour in
                HStack(alignment: .top, spacing: 0) {
                    Text(hourLabel(hour))
                        .font(.system(size: 9))
                        .foregroundStyle(.secondary)
                        .frame(width: timeColumnWidth, alignment: .trailing)
                        .padding(.trailing, 4)
                        .offset(y: -5)

                    VStack {
                        Divider()
                        Spacer()
                    }
                }
                .frame(height: hourHeight)
                .id(hour)
            }
        }
    }

    // MARK: - Event Columns

    private var eventColumns: some View {
        HStack(alignment: .top, spacing: 0) {
            Color.clear
                .frame(width: timeColumnWidth)

            ForEach(weekDates, id: \.self) { date in
                ZStack(alignment: .topLeading) {
                    let dayEvents = viewModel.timedEvents(for: date)
                    ForEach(dayEvents) { event in
                        weekEventBlock(event: event)
                            .offset(y: yOffset(for: event.start))
                    }
                }
                .frame(maxWidth: .infinity)
                .padding(.horizontal, 1)
            }
        }
    }

    private func weekEventBlock(event: CalendarEvent) -> some View {
        let color = event.memberColor(members: viewModel.householdMembers)
        let minutes = CGFloat(event.durationMinutes)
        let height = max((minutes / 60) * hourHeight, 16)

        return VStack(alignment: .leading, spacing: 0) {
            Text(event.title)
                .font(.system(size: 9, weight: .medium))
                .lineLimit(height > 30 ? 2 : 1)
                .foregroundStyle(.primary)
        }
        .padding(.horizontal, 3)
        .padding(.vertical, 2)
        .frame(maxWidth: .infinity, alignment: .leading)
        .frame(height: height)
        .background(color.opacity(0.2))
        .overlay(alignment: .leading) {
            Rectangle()
                .fill(color)
                .frame(width: 3)
        }
        .clipShape(RoundedRectangle(cornerRadius: 3))
    }

    // MARK: - Current Time Indicator

    @ViewBuilder
    private var currentTimeIndicator: some View {
        let now = Date()
        if weekDates.contains(where: { calendar.isDate($0, inSameDayAs: now) }) {
            let y = yOffset(for: now)
            HStack(spacing: 0) {
                Color.clear
                    .frame(width: timeColumnWidth - 4)
                Circle()
                    .fill(.red)
                    .frame(width: 6, height: 6)
                Rectangle()
                    .fill(.red)
                    .frame(height: 1)
            }
            .offset(y: y - 3)
        }
    }

    // MARK: - Helpers

    private func yOffset(for date: Date) -> CGFloat {
        let hour = CGFloat(calendar.component(.hour, from: date))
        let minute = CGFloat(calendar.component(.minute, from: date))
        return (hour + minute / 60) * hourHeight
    }

    private func hourLabel(_ hour: Int) -> String {
        switch hour {
        case 0: return "12a"
        case 1...11: return "\(hour)a"
        case 12: return "12p"
        default: return "\(hour - 12)p"
        }
    }
}

#Preview {
    let vm = CalendarViewModel()
    CalendarWeekView(
        weekDates: vm.currentWeekDates,
        viewModel: vm
    )
}
