import 'package:flutter/material.dart';

import '../workspace/german_dates.dart';
import '../workspace/workspace_items.dart';
import 'recurrence.dart';

/// What the user decided in an item editor.
enum EditorAction { save, delete }

class EventEditorResult {
  const EventEditorResult(this.action, this.event);

  final EditorAction action;
  final LocalCalendarItem event;
}

/// Opens the appointment window for a new or existing event.
///
/// `initialStart` seeds a new event; it is rounded up to the next half hour
/// unless `initialAllDay` is set. `occurrenceDate` names the occurrence of
/// a recurring event the user opened, so a single occurrence can be
/// removed. `calendars` lists the synchronized calendars the event may be
/// stored in besides the local one.
Future<EventEditorResult?> showEventEditor(
  BuildContext context, {
  LocalCalendarItem? event,
  DateTime? initialStart,
  bool initialAllDay = false,
  DateTime? occurrenceDate,
  List<WorkspaceCollection> calendars = const [],
  String? initialCollectionId,
}) {
  return showDialog<EventEditorResult>(
    context: context,
    barrierDismissible: false,
    builder: (dialogContext) => _EventEditor(
      event: event,
      initialStart: initialStart,
      initialAllDay: initialAllDay,
      occurrenceDate: occurrenceDate,
      calendars: calendars,
      initialCollectionId: initialCollectionId,
    ),
  );
}

DateTime _nextHalfHour(DateTime value) {
  final minute = value.minute < 30 ? 30 : 60;
  return DateTime(value.year, value.month, value.day, value.hour, minute);
}

/// Repetition choices offered in the window.
enum _RepeatChoice {
  none('Keine Wiederholung'),
  daily('Täglich'),
  weekly('Wöchentlich'),
  monthly('Monatlich'),
  yearly('Jährlich');

  const _RepeatChoice(this.label);

  final String label;

  RecurrenceFrequency? get frequency => switch (this) {
    _RepeatChoice.none => null,
    _RepeatChoice.daily => RecurrenceFrequency.daily,
    _RepeatChoice.weekly => RecurrenceFrequency.weekly,
    _RepeatChoice.monthly => RecurrenceFrequency.monthly,
    _RepeatChoice.yearly => RecurrenceFrequency.yearly,
  };

  static _RepeatChoice of(RecurrenceFrequency? frequency) =>
      switch (frequency) {
        null => _RepeatChoice.none,
        RecurrenceFrequency.daily => _RepeatChoice.daily,
        RecurrenceFrequency.weekly => _RepeatChoice.weekly,
        RecurrenceFrequency.monthly => _RepeatChoice.monthly,
        RecurrenceFrequency.yearly => _RepeatChoice.yearly,
      };
}

enum _EndChoice {
  never('Nie'),
  onDate('Am Datum'),
  afterCount('Nach Anzahl');

  const _EndChoice(this.label);

  final String label;
}

class _EventEditor extends StatefulWidget {
  const _EventEditor({
    required this.event,
    required this.initialStart,
    required this.initialAllDay,
    required this.occurrenceDate,
    required this.calendars,
    required this.initialCollectionId,
  });

  final LocalCalendarItem? event;
  final DateTime? initialStart;
  final bool initialAllDay;
  final DateTime? occurrenceDate;
  final List<WorkspaceCollection> calendars;
  final String? initialCollectionId;

  @override
  State<_EventEditor> createState() => _EventEditorState();
}

class _EventEditorState extends State<_EventEditor> {
  final formKey = GlobalKey<FormState>();
  late final TextEditingController title;
  late final TextEditingController location;
  late final TextEditingController description;
  late final TextEditingController startDate;
  late final TextEditingController startTime;
  late final TextEditingController endDate;
  late final TextEditingController endTime;
  late final TextEditingController interval;
  late final TextEditingController untilDate;
  late final TextEditingController count;
  late bool allDay;
  int? reminderMinutes;
  String? rangeError;
  late _RepeatChoice repeat;
  late _EndChoice endChoice;
  late Set<int> weekdays;
  String? collectionId;

  @override
  void initState() {
    super.initState();
    final event = widget.event;
    final DateTime start;
    final DateTime end;
    if (event != null) {
      allDay = event.allDay;
      start = event.startsAt;
      end = allDay
          ? event.endsAt.subtract(const Duration(days: 1))
          : event.endsAt;
      reminderMinutes = event.reminderMinutesBefore;
      collectionId = event.collectionId;
    } else {
      allDay = widget.initialAllDay;
      final seed = widget.initialStart ?? DateTime.now();
      start = allDay ? dateOnly(seed) : _nextHalfHour(seed);
      end = allDay ? start : start.add(const Duration(minutes: 30));
      reminderMinutes = 15;
      collectionId = widget.initialCollectionId;
    }
    if (collectionId != null &&
        !widget.calendars.any((calendar) => calendar.id == collectionId)) {
      collectionId = null;
    }
    final rule = event?.recurrence;
    repeat = _RepeatChoice.of(rule?.frequency);
    endChoice = rule?.until != null
        ? _EndChoice.onDate
        : rule?.count != null
        ? _EndChoice.afterCount
        : _EndChoice.never;
    weekdays = {...?rule?.weekdays};
    if (weekdays.isEmpty) weekdays = {start.weekday};
    title = TextEditingController(text: event?.title ?? '');
    location = TextEditingController(text: event?.location ?? '');
    description = TextEditingController(text: event?.description ?? '');
    startDate = TextEditingController(text: formatDate(start));
    startTime = TextEditingController(text: formatClock(start));
    endDate = TextEditingController(text: formatDate(end));
    endTime = TextEditingController(text: formatClock(end));
    interval = TextEditingController(text: '${rule?.interval ?? 1}');
    untilDate = TextEditingController(
      text: rule?.until == null ? '' : formatDate(rule!.until!),
    );
    count = TextEditingController(text: '${rule?.count ?? 10}');
  }

  @override
  void dispose() {
    for (final controller in [
      title,
      location,
      description,
      startDate,
      startTime,
      endDate,
      endTime,
      interval,
      untilDate,
      count,
    ]) {
      controller.dispose();
    }
    super.dispose();
  }

  /// Resolves the typed range; null when a field is still invalid.
  (DateTime, DateTime)? resolvedRange() {
    final startDay = parseGermanDate(startDate.text);
    final endDay = parseGermanDate(endDate.text);
    if (startDay == null || endDay == null) return null;
    if (allDay) {
      return (startDay, endDay.add(const Duration(days: 1)));
    }
    final startClock = parseClock(startTime.text);
    final endClock = parseClock(endTime.text);
    if (startClock == null || endClock == null) return null;
    return (combine(startDay, startClock), combine(endDay, endClock));
  }

  Recurrence? resolvedRecurrence() {
    final frequency = repeat.frequency;
    if (frequency == null) return null;
    final every = int.tryParse(interval.text.trim()) ?? 1;
    return Recurrence(
      frequency: frequency,
      interval: every.clamp(1, 999),
      weekdays: frequency == RecurrenceFrequency.weekly
          ? (weekdays.toList()..sort())
          : const [],
      until: endChoice == _EndChoice.onDate
          ? parseGermanDate(
              untilDate.text,
            )?.add(const Duration(hours: 23, minutes: 59))
          : null,
      count: endChoice == _EndChoice.afterCount
          ? int.tryParse(count.text.trim())?.clamp(1, 9999)
          : null,
    );
  }

  void save() {
    if (!(formKey.currentState?.validate() ?? false)) return;
    final range = resolvedRange();
    if (range == null) return;
    final (start, end) = range;
    if (!end.isAfter(start)) {
      setState(() => rangeError = 'Das Ende muss nach dem Beginn liegen.');
      return;
    }
    final recurrence = resolvedRecurrence();
    if (recurrence != null &&
        recurrence.frequency == RecurrenceFrequency.weekly &&
        recurrence.weekdays.isEmpty) {
      setState(() => rangeError = 'Bitte mindestens einen Wochentag wählen.');
      return;
    }
    if (endChoice == _EndChoice.onDate &&
        recurrence != null &&
        recurrence.until == null) {
      setState(() => rangeError = 'Bitte ein Enddatum der Serie eingeben.');
      return;
    }
    final original = widget.event;
    final timingChanged =
        original == null ||
        original.startsAt != start ||
        original.reminderMinutesBefore != reminderMinutes;
    final event = LocalCalendarItem(
      id:
          original?.id ??
          'local.event.${DateTime.now().microsecondsSinceEpoch}',
      title: title.text.trim(),
      startsAt: start,
      endsAt: end,
      allDay: allDay,
      location: location.text.trim().isEmpty ? null : location.text.trim(),
      description: description.text.trim().isEmpty
          ? null
          : description.text.trim(),
      reminderMinutesBefore: reminderMinutes,
      reminderDismissedThrough: timingChanged
          ? null
          : original.reminderDismissedThrough,
      recurrence: recurrence,
      exceptionDates: recurrence == null
          ? const []
          : (original?.exceptionDates ?? const []),
      collectionId: collectionId,
    );
    Navigator.pop(context, EventEditorResult(EditorAction.save, event));
  }

  Future<bool> confirm(String title, String message, Key key) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (dialogContext) => AlertDialog(
        title: Text(title),
        content: Text(message),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(dialogContext, false),
            child: const Text('Abbrechen'),
          ),
          FilledButton(
            key: key,
            onPressed: () => Navigator.pop(dialogContext, true),
            child: const Text('Löschen'),
          ),
        ],
      ),
    );
    return confirmed == true;
  }

  Future<void> deleteSeries() async {
    final event = widget.event;
    if (event == null) return;
    final confirmed = await confirm(
      event.recurring ? 'Gesamte Serie löschen?' : 'Termin löschen?',
      event.recurring
          ? '„${event.title}“ wird mit allen Wiederholungen entfernt.'
          : '„${event.title}“ wird endgültig entfernt.',
      const Key('event-delete-confirm'),
    );
    if (!confirmed || !mounted) return;
    Navigator.pop(context, EventEditorResult(EditorAction.delete, event));
  }

  Future<void> deleteOccurrence() async {
    final event = widget.event;
    final date = widget.occurrenceDate;
    if (event == null || date == null) return;
    final confirmed = await confirm(
      'Nur diesen Termin löschen?',
      'Der Termin am ${formatDate(date)} wird aus der Serie entfernt; '
          'alle anderen Wiederholungen bleiben.',
      const Key('event-delete-occurrence-confirm'),
    );
    if (!confirmed || !mounted) return;
    final updated = event.copyWith(
      exceptionDates: {...event.exceptionDates, formatIsoDate(date)}.toList()
        ..sort(),
    );
    Navigator.pop(context, EventEditorResult(EditorAction.save, updated));
  }

  @override
  Widget build(BuildContext context) {
    final editing = widget.event != null;
    final recurringOccurrence =
        editing && widget.event!.recurring && widget.occurrenceDate != null;
    return EditorDialog(
      title: editing ? 'Termin bearbeiten' : 'Neuer Termin',
      icon: Icons.event_outlined,
      leading: [
        if (recurringOccurrence)
          TextButton.icon(
            key: const Key('event-delete-occurrence'),
            onPressed: deleteOccurrence,
            icon: const Icon(Icons.event_busy_outlined, size: 18),
            label: const Text('Nur diesen Termin'),
          ),
        if (editing)
          TextButton.icon(
            key: const Key('event-delete'),
            onPressed: deleteSeries,
            icon: const Icon(Icons.delete_outline, size: 18),
            label: Text(recurringOccurrence ? 'Ganze Serie' : 'Löschen'),
          ),
      ],
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('Abbrechen'),
        ),
        FilledButton(
          key: const Key('event-save'),
          onPressed: save,
          child: const Text('Speichern'),
        ),
      ],
      children: [
        Form(
          key: formKey,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              TextFormField(
                key: const Key('event-title'),
                controller: title,
                autofocus: !editing,
                decoration: const InputDecoration(labelText: 'Betreff'),
                textInputAction: TextInputAction.next,
                validator: (value) => (value?.trim().isEmpty ?? true)
                    ? 'Bitte einen Betreff eingeben.'
                    : null,
              ),
              const SizedBox(height: 12),
              Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Expanded(
                    flex: 3,
                    child: TextFormField(
                      key: const Key('event-location'),
                      controller: location,
                      decoration: const InputDecoration(
                        labelText: 'Ort',
                        prefixIcon: Icon(Icons.place_outlined, size: 18),
                      ),
                    ),
                  ),
                  if (widget.calendars.isNotEmpty) ...[
                    const SizedBox(width: 8),
                    Expanded(
                      flex: 2,
                      child: DropdownButtonFormField<String?>(
                        key: const Key('event-calendar'),
                        isExpanded: true,
                        initialValue: collectionId,
                        decoration: const InputDecoration(
                          labelText: 'Kalender',
                        ),
                        items: [
                          const DropdownMenuItem<String?>(
                            value: null,
                            child: Text('Persönlicher Kalender'),
                          ),
                          for (final calendar in widget.calendars)
                            DropdownMenuItem<String?>(
                              value: calendar.id,
                              child: Text(
                                calendar.name,
                                overflow: TextOverflow.ellipsis,
                              ),
                            ),
                        ],
                        onChanged: (value) =>
                            setState(() => collectionId = value),
                      ),
                    ),
                  ],
                ],
              ),
              const SizedBox(height: 12),
              CheckboxListTile(
                key: const Key('event-all-day'),
                value: allDay,
                dense: true,
                contentPadding: EdgeInsets.zero,
                controlAffinity: ListTileControlAffinity.leading,
                title: const Text('Ganztägiges Ereignis'),
                onChanged: (value) => setState(() {
                  allDay = value ?? false;
                  rangeError = null;
                }),
              ),
              const SizedBox(height: 4),
              Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Expanded(
                    flex: 3,
                    child: DateField(
                      fieldKey: const Key('event-start-date'),
                      controller: startDate,
                      label: 'Beginn',
                      required: true,
                      onChanged: () => setState(() => rangeError = null),
                    ),
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    flex: 2,
                    child: TimeField(
                      fieldKey: const Key('event-start-time'),
                      controller: startTime,
                      label: 'Uhrzeit',
                      enabled: !allDay,
                      required: !allDay,
                      onChanged: () => setState(() => rangeError = null),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 12),
              Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Expanded(
                    flex: 3,
                    child: DateField(
                      fieldKey: const Key('event-end-date'),
                      controller: endDate,
                      label: 'Ende',
                      required: true,
                      onChanged: () => setState(() => rangeError = null),
                    ),
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    flex: 2,
                    child: TimeField(
                      fieldKey: const Key('event-end-time'),
                      controller: endTime,
                      label: 'Uhrzeit',
                      enabled: !allDay,
                      required: !allDay,
                      onChanged: () => setState(() => rangeError = null),
                    ),
                  ),
                ],
              ),
              if (rangeError case final error?)
                Padding(
                  padding: const EdgeInsets.only(top: 8),
                  child: Text(
                    error,
                    key: const Key('event-range-error'),
                    style: TextStyle(
                      color: Theme.of(context).colorScheme.error,
                      fontSize: 12,
                    ),
                  ),
                ),
              const SizedBox(height: 12),
              _recurrenceSection(context),
              const SizedBox(height: 12),
              DropdownButtonFormField<int?>(
                key: const Key('event-reminder'),
                isExpanded: true,
                initialValue: reminderMinutes,
                decoration: const InputDecoration(
                  labelText: 'Erinnerung',
                  prefixIcon: Icon(Icons.notifications_outlined, size: 18),
                ),
                items: [
                  const DropdownMenuItem<int?>(
                    value: null,
                    child: Text('Keine'),
                  ),
                  for (final minutes in eventReminderChoices)
                    DropdownMenuItem<int?>(
                      value: minutes,
                      child: Text(reminderLeadLabel(minutes)),
                    ),
                ],
                onChanged: (value) => setState(() => reminderMinutes = value),
              ),
              const SizedBox(height: 12),
              TextFormField(
                key: const Key('event-description'),
                controller: description,
                decoration: const InputDecoration(
                  labelText: 'Beschreibung',
                  alignLabelWithHint: true,
                ),
                minLines: 4,
                maxLines: 10,
              ),
            ],
          ),
        ),
      ],
    );
  }

  Widget _recurrenceSection(BuildContext context) {
    final palette = Theme.of(context).colorScheme;
    return Container(
      padding: const EdgeInsets.all(10),
      decoration: BoxDecoration(
        border: Border.all(color: Theme.of(context).dividerColor),
        borderRadius: BorderRadius.circular(3),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Expanded(
                flex: 3,
                child: DropdownButtonFormField<_RepeatChoice>(
                  key: const Key('event-repeat'),
                  isExpanded: true,
                  initialValue: repeat,
                  decoration: const InputDecoration(
                    labelText: 'Wiederholung',
                    prefixIcon: Icon(Icons.repeat, size: 18),
                  ),
                  items: [
                    for (final choice in _RepeatChoice.values)
                      DropdownMenuItem(
                        value: choice,
                        child: Text(choice.label),
                      ),
                  ],
                  onChanged: (value) => setState(() {
                    repeat = value ?? _RepeatChoice.none;
                    rangeError = null;
                  }),
                ),
              ),
              if (repeat != _RepeatChoice.none) ...[
                const SizedBox(width: 8),
                Expanded(
                  flex: 2,
                  child: TextFormField(
                    key: const Key('event-repeat-interval'),
                    controller: interval,
                    decoration: InputDecoration(
                      labelText: 'Alle',
                      suffixText: switch (repeat) {
                        _RepeatChoice.daily => 'Tage',
                        _RepeatChoice.weekly => 'Wochen',
                        _RepeatChoice.monthly => 'Monate',
                        _RepeatChoice.yearly => 'Jahre',
                        _RepeatChoice.none => '',
                      },
                    ),
                    keyboardType: TextInputType.number,
                    onChanged: (_) => setState(() {}),
                    validator: (value) {
                      final parsed = int.tryParse(value?.trim() ?? '');
                      return parsed == null || parsed < 1 ? 'Zahl ab 1' : null;
                    },
                  ),
                ),
              ],
            ],
          ),
          if (repeat == _RepeatChoice.weekly) ...[
            const SizedBox(height: 10),
            Wrap(
              spacing: 6,
              children: [
                for (var day = 1; day <= 7; day++)
                  FilterChip(
                    key: Key('event-weekday-$day'),
                    label: Text(germanWeekdaysShort[day - 1]),
                    selected: weekdays.contains(day),
                    onSelected: (selected) => setState(() {
                      if (selected) {
                        weekdays.add(day);
                      } else {
                        weekdays.remove(day);
                      }
                      rangeError = null;
                    }),
                  ),
              ],
            ),
          ],
          if (repeat != _RepeatChoice.none) ...[
            const SizedBox(height: 10),
            Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Expanded(
                  flex: 3,
                  child: DropdownButtonFormField<_EndChoice>(
                    key: const Key('event-repeat-end'),
                    isExpanded: true,
                    initialValue: endChoice,
                    decoration: const InputDecoration(labelText: 'Ende'),
                    items: [
                      for (final choice in _EndChoice.values)
                        DropdownMenuItem(
                          value: choice,
                          child: Text(choice.label),
                        ),
                    ],
                    onChanged: (value) => setState(() {
                      endChoice = value ?? _EndChoice.never;
                      rangeError = null;
                    }),
                  ),
                ),
                const SizedBox(width: 8),
                Expanded(
                  flex: 2,
                  child: switch (endChoice) {
                    _EndChoice.onDate => DateField(
                      fieldKey: const Key('event-repeat-until'),
                      controller: untilDate,
                      label: 'Bis',
                      required: true,
                      onChanged: () => setState(() => rangeError = null),
                    ),
                    _EndChoice.afterCount => TextFormField(
                      key: const Key('event-repeat-count'),
                      controller: count,
                      decoration: const InputDecoration(labelText: 'Termine'),
                      keyboardType: TextInputType.number,
                      onChanged: (_) => setState(() {}),
                      validator: (value) {
                        final parsed = int.tryParse(value?.trim() ?? '');
                        return parsed == null || parsed < 1
                            ? 'Zahl ab 1'
                            : null;
                      },
                    ),
                    _EndChoice.never => const SizedBox.shrink(),
                  },
                ),
              ],
            ),
            const SizedBox(height: 6),
            Text(
              describeRecurrence(resolvedRecurrence()),
              key: const Key('event-repeat-summary'),
              style: TextStyle(fontSize: 12, color: palette.primary),
            ),
          ],
        ],
      ),
    );
  }
}
