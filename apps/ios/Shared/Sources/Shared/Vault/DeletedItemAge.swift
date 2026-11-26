import Foundation

/// The line under an item in "Recently deleted": how long ago it was deleted, counted in calendar days.
public enum DeletedItemAge {
    public static func text(deleted: Date, now: Date, calendar: Calendar = .current) -> String {
        let days = max(
            0,
            calendar.dateComponents([.day], from: calendar.startOfDay(for: deleted), to: calendar.startOfDay(for: now))
                .day ?? 0)
        switch days {
        case 0: return "Deleted today"
        case 1: return "Deleted yesterday"
        default: return "Deleted \(days) days ago"
        }
    }
}
