import 'package:flutter/material.dart';

import '../../app_theme.dart';
import '../workspace/double_click.dart';
import '../workspace/german_dates.dart';
import '../workspace/workspace_items.dart';

/// Layouts offered by the calendar module.
enum CalendarView {
  day('Tag'),
  workWeek('Arbeitswoche'),
  week('Woche'),
  month('Monat');

  const CalendarView(this.label);

  final String label;
}

/// Days shown for `view` around `focused`, Monday first for the week views.
List<DateTime> visibleDays(CalendarView view, DateTime focused) {
  final day = dateOnly(focused);
  switch (view) {
    case CalendarView.day:
      return [day];
    case CalendarView.workWeek:
      final monday = startOfWeek(day);
      return List.generate(5, (index) => monday.add(Duration(days: index)));
    case CalendarView.week:
      final monday = startOfWeek(day);
      return List.generate(7, (index) => monday.add(Duration(days: index)));
    case CalendarView.month:
      final first = DateTime(day.year, day.month);
      final start = startOfWeek(first);
      return List.generate(42, (index) => start.add(Duration(days: index)));
  }
}

/// Moves `focused` one step forward or backward for the view.
DateTime shiftFocus(CalendarView view, DateTime focused, int direction) {
  switch (view) {
    case CalendarView.day:
      return focused.add(Duration(days: direction));
    case CalendarView.workWeek:
    case CalendarView.week:
      return focused.add(Duration(days: 7 * direction));
    case CalendarView.month:
      final target = DateTime(focused.year, focused.month + direction);
      final lastDay = DateTime(target.year, target.month + 1, 0).day;
      return DateTime(
        target.year,
        target.month,
        focused.day > lastDay ? lastDay : focused.day,
      );
  }
}

/// Heading for the visible range, e.g. `27. – 31. Juli 2026`.
String rangeLabel(CalendarView view, DateTime focused) {
  final days = visibleDays(view, focused);
  switch (view) {
    case CalendarView.day:
      return formatLongDate(days.first);
    case CalendarView.month:
      return formatMonthYear(focused);
    case CalendarView.workWeek:
    case CalendarView.week:
      final first = days.first;
      final last = days.last;
      if (first.month == last.month) {
        return '${first.day}. – ${last.day}. '
            '${germanMonths[first.month - 1]} ${first.year}';
      }
      if (first.year == last.year) {
        return '${first.day}. ${germanMonths[first.month - 1]} – '
            '${last.day}. ${germanMonths[last.month - 1]} ${first.year}';
      }
      return '${formatMediumDate(first)} – ${formatMediumDate(last)}';
  }
}

/// Mini month calendar for the navigation pane.
class MiniCalendar extends StatelessWidget {
  const MiniCalendar({
    super.key,
    required this.focused,
    required this.events,
    required this.onSelected,
    this.today,
  });

  final DateTime focused;
  final List<LocalCalendarItem> events;
  final ValueChanged<DateTime> onSelected;
  final DateTime? today;

  @override
  Widget build(BuildContext context) {
    final palette = MaicentaPalette.of(context);
    final now = dateOnly(today ?? DateTime.now());
    final days = visibleDays(CalendarView.month, focused);
    final busyDays = <DateTime>{
      for (final event in events)
        for (final day in days)
          if (event.occursOn(day)) day,
    };
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            Expanded(
              child: Text(
                formatMonthYear(focused),
                key: const Key('mini-calendar-month'),
                style: const TextStyle(fontWeight: FontWeight.w600),
              ),
            ),
            _NavIcon(
              key: const Key('mini-calendar-previous'),
              icon: Icons.chevron_left,
              tooltip: 'Vorheriger Monat',
              onTap: () =>
                  onSelected(shiftFocus(CalendarView.month, focused, -1)),
            ),
            _NavIcon(
              key: const Key('mini-calendar-next'),
              icon: Icons.chevron_right,
              tooltip: 'Nächster Monat',
              onTap: () =>
                  onSelected(shiftFocus(CalendarView.month, focused, 1)),
            ),
          ],
        ),
        const SizedBox(height: 8),
        Row(
          children: [
            for (final name in germanWeekdaysShort)
              Expanded(
                child: Center(
                  child: Text(
                    name,
                    style: TextStyle(fontSize: 9, color: palette.mutedText),
                  ),
                ),
              ),
          ],
        ),
        const SizedBox(height: 2),
        for (var row = 0; row < 6; row++)
          Row(
            children: [
              for (var column = 0; column < 7; column++)
                Expanded(
                  child: _MiniDay(
                    day: days[row * 7 + column],
                    inMonth: days[row * 7 + column].month == focused.month,
                    isToday: isSameDay(days[row * 7 + column], now),
                    isSelected: isSameDay(days[row * 7 + column], focused),
                    busy: busyDays.contains(days[row * 7 + column]),
                    onTap: () => onSelected(days[row * 7 + column]),
                  ),
                ),
            ],
          ),
      ],
    );
  }
}

class _MiniDay extends StatelessWidget {
  const _MiniDay({
    required this.day,
    required this.inMonth,
    required this.isToday,
    required this.isSelected,
    required this.busy,
    required this.onTap,
  });

  final DateTime day;
  final bool inMonth;
  final bool isToday;
  final bool isSelected;
  final bool busy;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final palette = MaicentaPalette.of(context);
    final primary = Theme.of(context).colorScheme.primary;
    return InkWell(
      key: Key('mini-calendar-day-${formatIsoDate(day)}'),
      onTap: onTap,
      borderRadius: BorderRadius.circular(12),
      child: Center(
        child: Container(
          width: 24,
          height: 24,
          alignment: Alignment.center,
          decoration: isToday
              ? BoxDecoration(color: primary, shape: BoxShape.circle)
              : isSelected
              ? BoxDecoration(
                  shape: BoxShape.circle,
                  border: Border.all(color: primary),
                )
              : null,
          child: Text(
            '${day.day}',
            style: TextStyle(
              fontSize: 10,
              fontWeight: busy ? FontWeight.w700 : FontWeight.normal,
              color: isToday
                  ? Colors.white
                  : inMonth
                  ? null
                  : palette.mutedText,
            ),
          ),
        ),
      ),
    );
  }
}

class _NavIcon extends StatelessWidget {
  const _NavIcon({
    super.key,
    required this.icon,
    required this.tooltip,
    required this.onTap,
  });

  final IconData icon;
  final String tooltip;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return IconButton(
      icon: Icon(icon, size: 18),
      tooltip: tooltip,
      padding: EdgeInsets.zero,
      constraints: const BoxConstraints.tightFor(width: 26, height: 26),
      onPressed: onTap,
    );
  }
}

/// The calendar module body: toolbar with navigation and view switch, and
/// the day, week, or month layout beneath it.
class CalendarWorkspace extends StatefulWidget {
  const CalendarWorkspace({
    super.key,
    required this.events,
    required this.enabled,
    required this.focused,
    required this.view,
    required this.onFocusedChanged,
    required this.onViewChanged,
    required this.onOpenEvent,
    required this.onCreateAt,
    this.today,
  });

  final List<LocalCalendarItem> events;

  /// Whether the personal calendar is switched on in the navigation pane.
  final bool enabled;
  final DateTime focused;
  final CalendarView view;
  final ValueChanged<DateTime> onFocusedChanged;
  final ValueChanged<CalendarView> onViewChanged;
  final ValueChanged<LocalCalendarItem> onOpenEvent;

  /// Called with the start of the double-clicked slot; `allDay` for a month
  /// cell or the all-day row.
  final void Function(DateTime start, bool allDay) onCreateAt;

  /// Overrides the current day, for tests.
  final DateTime? today;

  @override
  State<CalendarWorkspace> createState() => _CalendarWorkspaceState();
}

class _CalendarWorkspaceState extends State<CalendarWorkspace> {
  static const hourHeight = 48.0;
  final ScrollController timeScroll = ScrollController(
    initialScrollOffset: 7 * hourHeight,
  );

  @override
  void dispose() {
    timeScroll.dispose();
    super.dispose();
  }

  List<LocalCalendarItem> get visibleEvents =>
      widget.enabled ? widget.events : const [];

  @override
  Widget build(BuildContext context) {
    final palette = MaicentaPalette.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Container(
          height: 54,
          padding: const EdgeInsets.symmetric(horizontal: 14),
          decoration: BoxDecoration(
            color: palette.pane,
            border: Border(bottom: BorderSide(color: palette.border)),
          ),
          child: Row(
            children: [
              OutlinedButton(
                key: const Key('calendar-today'),
                onPressed: () =>
                    widget.onFocusedChanged(widget.today ?? DateTime.now()),
                child: const Text('Heute'),
              ),
              const SizedBox(width: 6),
              _NavIcon(
                key: const Key('calendar-previous'),
                icon: Icons.chevron_left,
                tooltip: 'Zurück',
                onTap: () => widget.onFocusedChanged(
                  shiftFocus(widget.view, widget.focused, -1),
                ),
              ),
              _NavIcon(
                key: const Key('calendar-next'),
                icon: Icons.chevron_right,
                tooltip: 'Weiter',
                onTap: () => widget.onFocusedChanged(
                  shiftFocus(widget.view, widget.focused, 1),
                ),
              ),
              const SizedBox(width: 10),
              Expanded(
                child: Text(
                  rangeLabel(widget.view, widget.focused),
                  key: const Key('calendar-range-label'),
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(
                    fontSize: 17,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
              for (final view in CalendarView.values)
                _ViewButton(
                  view: view,
                  selected: view == widget.view,
                  onTap: () => widget.onViewChanged(view),
                ),
            ],
          ),
        ),
        Expanded(
          child: widget.view == CalendarView.month
              ? _MonthGrid(
                  days: visibleDays(CalendarView.month, widget.focused),
                  focused: widget.focused,
                  today: widget.today ?? DateTime.now(),
                  events: visibleEvents,
                  onSelectDay: widget.onFocusedChanged,
                  onOpenEvent: widget.onOpenEvent,
                  onCreateAt: widget.onCreateAt,
                )
              : _TimeGrid(
                  days: visibleDays(widget.view, widget.focused),
                  today: widget.today ?? DateTime.now(),
                  events: visibleEvents,
                  hourHeight: hourHeight,
                  scrollController: timeScroll,
                  onOpenEvent: widget.onOpenEvent,
                  onCreateAt: widget.onCreateAt,
                  onSelectDay: widget.onFocusedChanged,
                ),
        ),
      ],
    );
  }
}

class _ViewButton extends StatelessWidget {
  const _ViewButton({
    required this.view,
    required this.selected,
    required this.onTap,
  });

  final CalendarView view;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final palette = MaicentaPalette.of(context);
    return InkWell(
      key: Key('calendar-view-${view.name}'),
      onTap: onTap,
      borderRadius: BorderRadius.circular(2),
      child: Container(
        margin: const EdgeInsets.only(left: 4),
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
        decoration: BoxDecoration(
          color: selected ? palette.selected : null,
          border: Border.all(
            color: selected ? maicentaPrimaryBlue : palette.border,
          ),
          borderRadius: BorderRadius.circular(2),
        ),
        child: Text(
          view.label,
          style: TextStyle(
            fontSize: 12,
            fontWeight: selected ? FontWeight.w600 : FontWeight.normal,
          ),
        ),
      ),
    );
  }
}

/// One timed event placed inside a day column.
class _PlacedEvent {
  const _PlacedEvent({
    required this.event,
    required this.start,
    required this.end,
    required this.column,
    required this.columns,
  });

  final LocalCalendarItem event;
  final DateTime start;
  final DateTime end;
  final int column;
  final int columns;
}

/// Lays out the timed events of one day side by side where they overlap.
List<_PlacedEvent> _placeTimedEvents(
  List<LocalCalendarItem> events,
  DateTime day,
) {
  final dayStart = dateOnly(day);
  final dayEnd = dayStart.add(const Duration(days: 1));
  final clipped =
      [
        for (final event in events)
          if (!event.allDay &&
              event.startsAt.isBefore(dayEnd) &&
              event.endsAt.isAfter(dayStart))
            (
              event: event,
              start: event.startsAt.isBefore(dayStart)
                  ? dayStart
                  : event.startsAt,
              end: event.endsAt.isAfter(dayEnd) ? dayEnd : event.endsAt,
            ),
      ]..sort((a, b) {
        final byStart = a.start.compareTo(b.start);
        return byStart != 0 ? byStart : b.end.compareTo(a.end);
      });

  final placed = <_PlacedEvent>[];
  // A cluster is a run of events that transitively overlap; every event in
  // it shares the cluster's column count.
  var clusterStart = 0;
  var columnEnds = <DateTime>[];
  var assignments = <int>[];

  void closeCluster(int end) {
    final columns = columnEnds.length;
    for (var index = clusterStart; index < end; index++) {
      final item = clipped[index];
      placed.add(
        _PlacedEvent(
          event: item.event,
          start: item.start,
          end: item.end,
          column: assignments[index - clusterStart],
          columns: columns,
        ),
      );
    }
    clusterStart = end;
    columnEnds = [];
    assignments = [];
  }

  for (var index = 0; index < clipped.length; index++) {
    final item = clipped[index];
    final overlapsCluster = columnEnds.any((end) => end.isAfter(item.start));
    if (!overlapsCluster && columnEnds.isNotEmpty) closeCluster(index);
    var column = columnEnds.indexWhere((end) => !end.isAfter(item.start));
    if (column < 0) {
      column = columnEnds.length;
      columnEnds.add(item.end);
    } else {
      columnEnds[column] = item.end;
    }
    assignments.add(column);
  }
  if (columnEnds.isNotEmpty) closeCluster(clipped.length);
  return placed;
}

class _TimeGrid extends StatelessWidget {
  const _TimeGrid({
    required this.days,
    required this.today,
    required this.events,
    required this.hourHeight,
    required this.scrollController,
    required this.onOpenEvent,
    required this.onCreateAt,
    required this.onSelectDay,
  });

  final List<DateTime> days;
  final DateTime today;
  final List<LocalCalendarItem> events;
  final double hourHeight;
  final ScrollController scrollController;
  final ValueChanged<LocalCalendarItem> onOpenEvent;
  final void Function(DateTime start, bool allDay) onCreateAt;
  final ValueChanged<DateTime> onSelectDay;

  static const gutterWidth = 52.0;

  @override
  Widget build(BuildContext context) {
    final palette = MaicentaPalette.of(context);
    final allDayByDay = {
      for (final day in days)
        day: events
            .where(
              (event) =>
                  (event.allDay || !isSameDay(event.startsAt, event.lastDay)) &&
                  event.occursOn(day),
            )
            .toList(growable: false),
    };
    final hasAllDay = allDayByDay.values.any((list) => list.isNotEmpty);
    return Column(
      children: [
        Row(
          children: [
            const SizedBox(width: gutterWidth),
            for (final day in days)
              Expanded(
                child: InkWell(
                  key: Key('calendar-day-header-${formatIsoDate(day)}'),
                  onTap: () => onSelectDay(day),
                  child: Container(
                    height: 44,
                    alignment: Alignment.center,
                    decoration: BoxDecoration(
                      color: isSameDay(day, today)
                          ? palette.selected
                          : palette.pane,
                      border: Border(
                        left: BorderSide(color: palette.border),
                        bottom: BorderSide(color: palette.border),
                      ),
                    ),
                    child: Text(
                      days.length == 1
                          ? formatLongDate(day)
                          : '${germanWeekdays[day.weekday - 1]}, ${day.day}.',
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        fontSize: 12,
                        fontWeight: FontWeight.w600,
                        color: isSameDay(day, today)
                            ? maicentaPrimaryBlue
                            : null,
                      ),
                    ),
                  ),
                ),
              ),
          ],
        ),
        if (hasAllDay)
          IntrinsicHeight(
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                SizedBox(
                  width: gutterWidth,
                  child: Center(
                    child: Text(
                      'ganztägig',
                      style: TextStyle(fontSize: 9, color: palette.mutedText),
                    ),
                  ),
                ),
                for (final day in days)
                  Expanded(
                    child: DoubleClickInkWell(
                      behavior: HitTestBehavior.opaque,
                      onTap: () => onSelectDay(day),
                      onDoubleTap: () => onCreateAt(day, true),
                      child: Container(
                        constraints: const BoxConstraints(minHeight: 28),
                        padding: const EdgeInsets.all(2),
                        decoration: BoxDecoration(
                          border: Border(
                            left: BorderSide(color: palette.border),
                            bottom: BorderSide(color: palette.border),
                          ),
                        ),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            for (final event in allDayByDay[day]!)
                              _EventChip(
                                event: event,
                                showTime: false,
                                onTap: () => onOpenEvent(event),
                              ),
                          ],
                        ),
                      ),
                    ),
                  ),
              ],
            ),
          ),
        Expanded(
          child: SingleChildScrollView(
            controller: scrollController,
            child: SizedBox(
              height: 24 * hourHeight,
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  SizedBox(
                    width: gutterWidth,
                    child: Column(
                      children: [
                        for (var hour = 0; hour < 24; hour++)
                          SizedBox(
                            height: hourHeight,
                            child: Align(
                              alignment: Alignment.topRight,
                              child: Padding(
                                padding: const EdgeInsets.only(
                                  right: 6,
                                  top: 2,
                                ),
                                child: Text(
                                  '${hour.toString().padLeft(2, '0')}:00',
                                  style: TextStyle(
                                    fontSize: 10,
                                    color: palette.mutedText,
                                  ),
                                ),
                              ),
                            ),
                          ),
                      ],
                    ),
                  ),
                  for (final day in days)
                    Expanded(
                      child: _DayColumn(
                        day: day,
                        isToday: isSameDay(day, today),
                        now: today,
                        placed: _placeTimedEvents(events, day),
                        hourHeight: hourHeight,
                        onOpenEvent: onOpenEvent,
                        onCreateAt: onCreateAt,
                      ),
                    ),
                ],
              ),
            ),
          ),
        ),
      ],
    );
  }
}

class _DayColumn extends StatelessWidget {
  const _DayColumn({
    required this.day,
    required this.isToday,
    required this.now,
    required this.placed,
    required this.hourHeight,
    required this.onOpenEvent,
    required this.onCreateAt,
  });

  final DateTime day;
  final bool isToday;
  final DateTime now;
  final List<_PlacedEvent> placed;
  final double hourHeight;
  final ValueChanged<LocalCalendarItem> onOpenEvent;
  final void Function(DateTime start, bool allDay) onCreateAt;

  double offsetFor(DateTime time) =>
      (time.hour + time.minute / 60) * hourHeight;

  @override
  Widget build(BuildContext context) {
    final palette = MaicentaPalette.of(context);
    return LayoutBuilder(
      builder: (context, constraints) {
        final width = constraints.maxWidth;
        return Stack(
          children: [
            Column(
              children: [
                for (var hour = 0; hour < 24; hour++)
                  GestureDetector(
                    key: Key(
                      'calendar-slot-${formatIsoDate(day)}-'
                      '${hour.toString().padLeft(2, '0')}',
                    ),
                    behavior: HitTestBehavior.opaque,
                    onDoubleTap: () => onCreateAt(
                      DateTime(day.year, day.month, day.day, hour),
                      false,
                    ),
                    child: Container(
                      height: hourHeight,
                      decoration: BoxDecoration(
                        color: isToday
                            ? palette.unread
                            : (hour < 8 || hour >= 18 ? palette.subtle : null),
                        border: Border(
                          left: BorderSide(color: palette.border),
                          bottom: BorderSide(color: palette.border),
                        ),
                      ),
                    ),
                  ),
              ],
            ),
            for (final item in placed)
              Positioned(
                top: offsetFor(item.start),
                left: 2 + width * item.column / item.columns,
                width: (width / item.columns - 4).clamp(20, double.infinity),
                height:
                    ((item.end.difference(item.start).inMinutes / 60) *
                                hourHeight -
                            2)
                        .clamp(18, double.infinity),
                child: _TimedEventBlock(
                  event: item.event,
                  onTap: () => onOpenEvent(item.event),
                ),
              ),
            if (isToday)
              Positioned(
                top: offsetFor(now) - 1,
                left: 0,
                right: 0,
                child: Container(
                  height: 2,
                  color: Theme.of(context).colorScheme.error,
                ),
              ),
          ],
        );
      },
    );
  }
}

class _TimedEventBlock extends StatelessWidget {
  const _TimedEventBlock({required this.event, required this.onTap});

  final LocalCalendarItem event;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final palette = MaicentaPalette.of(context);
    return InkWell(
      key: Key('calendar-event-${event.id}'),
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 3),
        decoration: BoxDecoration(
          color: palette.selectedStrong,
          border: const Border(
            left: BorderSide(color: maicentaPrimaryBlue, width: 4),
          ),
          borderRadius: BorderRadius.circular(2),
        ),
        clipBehavior: Clip.hardEdge,
        child: LayoutBuilder(
          builder: (context, constraints) {
            // Short blocks only have room for the title on one line.
            final showTime = constraints.maxHeight >= 34;
            final titleLines = constraints.maxHeight >= 50 ? 2 : 1;
            return Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(
                  event.title,
                  maxLines: titleLines,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(
                    fontSize: 11,
                    fontWeight: FontWeight.w600,
                  ),
                ),
                if (showTime)
                  Text(
                    event.location == null
                        ? event.time
                        : '${event.time} · ${event.location}',
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(fontSize: 10),
                  ),
              ],
            );
          },
        ),
      ),
    );
  }
}

class _EventChip extends StatelessWidget {
  const _EventChip({
    required this.event,
    required this.showTime,
    required this.onTap,
  });

  final LocalCalendarItem event;
  final bool showTime;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final palette = MaicentaPalette.of(context);
    return InkWell(
      key: Key('calendar-chip-${event.id}'),
      onTap: onTap,
      child: Container(
        margin: const EdgeInsets.only(bottom: 2),
        padding: const EdgeInsets.symmetric(horizontal: 5, vertical: 2),
        decoration: BoxDecoration(
          color: event.allDay ? maicentaPrimaryBlue : palette.selectedStrong,
          borderRadius: BorderRadius.circular(2),
        ),
        child: Text(
          showTime && !event.allDay
              ? '${formatClock(event.startsAt)} ${event.title}'
              : event.title,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: TextStyle(
            fontSize: 10.5,
            fontWeight: FontWeight.w600,
            color: event.allDay ? Colors.white : null,
          ),
        ),
      ),
    );
  }
}

class _MonthGrid extends StatelessWidget {
  const _MonthGrid({
    required this.days,
    required this.focused,
    required this.today,
    required this.events,
    required this.onSelectDay,
    required this.onOpenEvent,
    required this.onCreateAt,
  });

  final List<DateTime> days;
  final DateTime focused;
  final DateTime today;
  final List<LocalCalendarItem> events;
  final ValueChanged<DateTime> onSelectDay;
  final ValueChanged<LocalCalendarItem> onOpenEvent;
  final void Function(DateTime start, bool allDay) onCreateAt;

  @override
  Widget build(BuildContext context) {
    final palette = MaicentaPalette.of(context);
    return Column(
      children: [
        Row(
          children: [
            for (final name in germanWeekdays)
              Expanded(
                child: Container(
                  height: 28,
                  alignment: Alignment.center,
                  decoration: BoxDecoration(
                    color: palette.pane,
                    border: Border(
                      left: BorderSide(color: palette.border),
                      bottom: BorderSide(color: palette.border),
                    ),
                  ),
                  child: Text(
                    name,
                    style: const TextStyle(
                      fontSize: 11,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                ),
              ),
          ],
        ),
        for (var row = 0; row < 6; row++)
          Expanded(
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                for (var column = 0; column < 7; column++)
                  Expanded(
                    child: _MonthCell(
                      day: days[row * 7 + column],
                      inMonth: days[row * 7 + column].month == focused.month,
                      isToday: isSameDay(days[row * 7 + column], today),
                      isSelected: isSameDay(days[row * 7 + column], focused),
                      events: events
                          .where(
                            (event) => event.occursOn(days[row * 7 + column]),
                          )
                          .toList(growable: false),
                      onSelect: () => onSelectDay(days[row * 7 + column]),
                      onCreate: () => onCreateAt(days[row * 7 + column], true),
                      onOpenEvent: onOpenEvent,
                    ),
                  ),
              ],
            ),
          ),
      ],
    );
  }
}

class _MonthCell extends StatelessWidget {
  const _MonthCell({
    required this.day,
    required this.inMonth,
    required this.isToday,
    required this.isSelected,
    required this.events,
    required this.onSelect,
    required this.onCreate,
    required this.onOpenEvent,
  });

  final DateTime day;
  final bool inMonth;
  final bool isToday;
  final bool isSelected;
  final List<LocalCalendarItem> events;
  final VoidCallback onSelect;
  final VoidCallback onCreate;
  final ValueChanged<LocalCalendarItem> onOpenEvent;

  static const chipHeight = 20.0;
  static const headerHeight = 24.0;

  @override
  Widget build(BuildContext context) {
    final palette = MaicentaPalette.of(context);
    return DoubleClickInkWell(
      key: Key('calendar-month-cell-${formatIsoDate(day)}'),
      behavior: HitTestBehavior.opaque,
      onTap: onSelect,
      onDoubleTap: onCreate,
      child: Container(
        padding: const EdgeInsets.all(3),
        decoration: BoxDecoration(
          color: isSelected
              ? palette.selected
              : inMonth
              ? null
              : palette.subtle,
          border: Border(
            left: BorderSide(color: palette.border),
            bottom: BorderSide(color: palette.border),
          ),
        ),
        child: LayoutBuilder(
          builder: (context, constraints) {
            // Show as many chips as fit; the last slot names the remainder.
            final slots = ((constraints.maxHeight - headerHeight) / chipHeight)
                .floor()
                .clamp(0, 6);
            final hidden = events.length > slots
                ? events.length - (slots - 1).clamp(0, slots)
                : 0;
            final shown = hidden > 0 ? (slots - 1).clamp(0, slots) : slots;
            return Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              mainAxisSize: MainAxisSize.min,
              children: [
                SizedBox(
                  height: headerHeight,
                  child: Align(
                    alignment: Alignment.topRight,
                    child: Container(
                      width: 22,
                      height: 22,
                      alignment: Alignment.center,
                      decoration: isToday
                          ? const BoxDecoration(
                              color: maicentaPrimaryBlue,
                              shape: BoxShape.circle,
                            )
                          : null,
                      child: Text(
                        '${day.day}',
                        style: TextStyle(
                          fontSize: 11,
                          fontWeight: isToday
                              ? FontWeight.w700
                              : FontWeight.normal,
                          color: isToday
                              ? Colors.white
                              : inMonth
                              ? null
                              : palette.mutedText,
                        ),
                      ),
                    ),
                  ),
                ),
                for (final event in events.take(shown))
                  SizedBox(
                    height: chipHeight,
                    child: _EventChip(
                      event: event,
                      showTime: true,
                      onTap: () => onOpenEvent(event),
                    ),
                  ),
                if (hidden > 0)
                  SizedBox(
                    height: chipHeight,
                    child: Text(
                      '+$hidden weitere',
                      style: TextStyle(fontSize: 10, color: palette.mutedText),
                    ),
                  ),
              ],
            );
          },
        ),
      ),
    );
  }
}
