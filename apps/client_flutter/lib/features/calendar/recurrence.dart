/// Expansion of recurring events into concrete occurrences for a date
/// range, mirroring the subset of RRULE the Rust core stores: daily,
/// weekly (with weekdays), monthly, and yearly rules with an interval and
/// an optional end by date or count, minus exception dates.
library;

import '../workspace/german_dates.dart';
import '../workspace/workspace_items.dart';

/// One concrete occurrence of an event.
class EventOccurrence {
  const EventOccurrence({
    required this.event,
    required this.startsAt,
    required this.endsAt,
  });

  final LocalCalendarItem event;
  final DateTime startsAt;
  final DateTime endsAt;

  /// Identifier of the underlying event.
  String get id => event.id;

  /// Calendar date the occurrence starts on, used for exceptions.
  DateTime get date => dateOnly(startsAt);

  /// ISO date of the occurrence, the form stored in exception lists.
  String get isoDate => formatIsoDate(startsAt);

  bool get allDay => event.allDay;

  String get title => event.title;

  /// Last calendar day the occurrence touches.
  DateTime get lastDay {
    final inclusiveEnd = allDay
        ? endsAt.subtract(const Duration(minutes: 1))
        : endsAt;
    final adjusted =
        !allDay &&
            inclusiveEnd.hour == 0 &&
            inclusiveEnd.minute == 0 &&
            inclusiveEnd.isAfter(startsAt)
        ? inclusiveEnd.subtract(const Duration(minutes: 1))
        : inclusiveEnd;
    return dateOnly(adjusted);
  }

  bool occursOn(DateTime day) {
    final target = dateOnly(day);
    return !target.isBefore(date) && !target.isAfter(lastDay);
  }

  /// Compact time range such as `09:30–10:00` or `Ganztägig`.
  String get time =>
      allDay ? 'Ganztägig' : '${formatClock(startsAt)}–${formatClock(endsAt)}';

  /// Moment at which the reminder for this occurrence is due, if any.
  DateTime? get reminderAt {
    final minutes = event.reminderMinutesBefore;
    if (minutes == null) return null;
    return startsAt.subtract(Duration(minutes: minutes));
  }

  /// Whether the user closed the reminder of this occurrence.
  bool get reminderDismissed {
    final through = event.reminderDismissedThrough;
    return through != null && !startsAt.isAfter(through);
  }
}

/// Upper bound on generated occurrences per event and range, as a guard
/// against runaway rules.
const _maxOccurrences = 1000;

DateTime _shift(DateTime value, Duration offset) => DateTime(
  value.year,
  value.month,
  value.day,
  value.hour,
  value.minute,
).add(offset);

/// Adds months while keeping the day of month where possible; a day that
/// does not exist in the target month (31 → February) is skipped by
/// returning null.
DateTime? _addMonths(DateTime start, int months) {
  final targetMonth = DateTime(start.year, start.month + months);
  final lastDay = DateTime(targetMonth.year, targetMonth.month + 1, 0).day;
  if (start.day > lastDay) return null;
  return DateTime(
    targetMonth.year,
    targetMonth.month,
    start.day,
    start.hour,
    start.minute,
  );
}

/// Expands `event` into the occurrences that touch the half-open range
/// `[rangeStart, rangeEnd)`. A single event yields at most one occurrence.
List<EventOccurrence> occurrencesBetween(
  LocalCalendarItem event,
  DateTime rangeStart,
  DateTime rangeEnd,
) {
  final duration = event.endsAt.difference(event.startsAt);
  final rule = event.recurrence;
  final exceptions = event.exceptionDates.toSet();

  bool touches(DateTime start) {
    final end = start.add(duration);
    return start.isBefore(rangeEnd) && end.isAfter(rangeStart);
  }

  EventOccurrence occurrence(DateTime start) => EventOccurrence(
    event: event,
    startsAt: start,
    endsAt: start.add(duration),
  );

  if (rule == null) {
    return touches(event.startsAt) ? [occurrence(event.startsAt)] : const [];
  }

  final result = <EventOccurrence>[];
  final interval = rule.interval < 1 ? 1 : rule.interval;
  final until = rule.until;
  final count = rule.count;
  var produced = 0;

  // Whether a candidate start belongs to the series and the range; returns
  // false when generation must stop.
  bool accept(DateTime start) {
    if (until != null && start.isAfter(until)) return false;
    if (count != null && produced >= count) return false;
    produced += 1;
    if (exceptions.contains(formatIsoDate(start))) return true;
    if (touches(start)) result.add(occurrence(start));
    return true;
  }

  switch (rule.frequency) {
    case RecurrenceFrequency.daily:
      var start = event.startsAt;
      for (var step = 0; step < _maxOccurrences; step++) {
        if (!accept(start)) break;
        if (start.isAfter(rangeEnd)) break;
        start = _shift(event.startsAt, Duration(days: interval * (step + 1)));
      }
    case RecurrenceFrequency.weekly:
      final weekdays = rule.weekdays.isEmpty
          ? [event.startsAt.weekday]
          : (rule.weekdays.toList()..sort());
      final firstWeek = startOfWeek(event.startsAt);
      // Compare at minute precision: generated starts carry no seconds.
      final origin = DateTime(
        event.startsAt.year,
        event.startsAt.month,
        event.startsAt.day,
        event.startsAt.hour,
        event.startsAt.minute,
      );
      var week = 0;
      var steps = 0;
      outer:
      while (steps < _maxOccurrences) {
        final weekStart = firstWeek.add(Duration(days: 7 * interval * week));
        for (final weekday in weekdays) {
          final day = weekStart.add(Duration(days: weekday - 1));
          final start = DateTime(
            day.year,
            day.month,
            day.day,
            event.startsAt.hour,
            event.startsAt.minute,
          );
          if (start.isBefore(origin)) continue;
          steps += 1;
          if (!accept(start)) break outer;
        }
        if (weekStart.isAfter(rangeEnd)) break;
        week += 1;
      }
    case RecurrenceFrequency.monthly:
      for (var step = 0; step < _maxOccurrences; step++) {
        final start = _addMonths(event.startsAt, interval * step);
        if (start == null) continue;
        if (!accept(start)) break;
        if (start.isAfter(rangeEnd)) break;
      }
    case RecurrenceFrequency.yearly:
      for (var step = 0; step < _maxOccurrences; step++) {
        final start = _addMonths(event.startsAt, 12 * interval * step);
        if (start == null) continue;
        if (!accept(start)) break;
        if (start.isAfter(rangeEnd)) break;
      }
  }
  return result;
}

/// Expands every event for the range, sorted by start.
List<EventOccurrence> expandEvents(
  Iterable<LocalCalendarItem> events,
  DateTime rangeStart,
  DateTime rangeEnd,
) {
  final occurrences = [
    for (final event in events)
      ...occurrencesBetween(event, rangeStart, rangeEnd),
  ]..sort((a, b) => a.startsAt.compareTo(b.startsAt));
  return occurrences;
}

/// The next occurrence starting at or after `from`, if the series has one.
EventOccurrence? nextOccurrence(LocalCalendarItem event, DateTime from) {
  final horizon = from.add(const Duration(days: 366 * 5));
  for (final occurrence in occurrencesBetween(event, from, horizon)) {
    if (!occurrence.startsAt.isBefore(from)) return occurrence;
  }
  return null;
}

/// Human-readable summary of a rule, e.g. `Alle 2 Wochen am Mo, Mi`.
String describeRecurrence(Recurrence? rule) {
  if (rule == null) return 'Keine Wiederholung';
  final interval = rule.interval;
  final base = switch (rule.frequency) {
    RecurrenceFrequency.daily =>
      interval == 1 ? 'Täglich' : 'Alle $interval Tage',
    RecurrenceFrequency.weekly =>
      interval == 1 ? 'Wöchentlich' : 'Alle $interval Wochen',
    RecurrenceFrequency.monthly =>
      interval == 1 ? 'Monatlich' : 'Alle $interval Monate',
    RecurrenceFrequency.yearly =>
      interval == 1 ? 'Jährlich' : 'Alle $interval Jahre',
  };
  final days =
      rule.frequency == RecurrenceFrequency.weekly && rule.weekdays.isNotEmpty
      ? ' am ${rule.weekdays.map((day) => germanWeekdaysShort[day - 1]).join(', ')}'
      : '';
  final end = rule.until != null
      ? ' bis ${formatDate(rule.until!)}'
      : rule.count != null
      ? ', ${rule.count} Termine'
      : '';
  return '$base$days$end';
}
