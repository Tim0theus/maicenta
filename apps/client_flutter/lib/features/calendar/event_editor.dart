import 'package:flutter/material.dart';

import '../workspace/german_dates.dart';
import '../workspace/workspace_items.dart';

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
/// unless `initialAllDay` is set.
Future<EventEditorResult?> showEventEditor(
  BuildContext context, {
  LocalCalendarItem? event,
  DateTime? initialStart,
  bool initialAllDay = false,
}) {
  return showDialog<EventEditorResult>(
    context: context,
    barrierDismissible: false,
    builder: (dialogContext) => _EventEditor(
      event: event,
      initialStart: initialStart,
      initialAllDay: initialAllDay,
    ),
  );
}

DateTime _nextHalfHour(DateTime value) {
  final minute = value.minute < 30 ? 30 : 60;
  return DateTime(value.year, value.month, value.day, value.hour, minute);
}

class _EventEditor extends StatefulWidget {
  const _EventEditor({
    required this.event,
    required this.initialStart,
    required this.initialAllDay,
  });

  final LocalCalendarItem? event;
  final DateTime? initialStart;
  final bool initialAllDay;

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
  late bool allDay;
  int? reminderMinutes;
  String? rangeError;

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
    } else {
      allDay = widget.initialAllDay;
      final seed = widget.initialStart ?? DateTime.now();
      start = allDay ? dateOnly(seed) : _nextHalfHour(seed);
      end = allDay ? start : start.add(const Duration(minutes: 30));
      reminderMinutes = 15;
    }
    title = TextEditingController(text: event?.title ?? '');
    location = TextEditingController(text: event?.location ?? '');
    description = TextEditingController(text: event?.description ?? '');
    startDate = TextEditingController(text: formatDate(start));
    startTime = TextEditingController(text: formatClock(start));
    endDate = TextEditingController(text: formatDate(end));
    endTime = TextEditingController(text: formatClock(end));
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

  void save() {
    if (!(formKey.currentState?.validate() ?? false)) return;
    final range = resolvedRange();
    if (range == null) return;
    final (start, end) = range;
    if (!end.isAfter(start)) {
      setState(() => rangeError = 'Das Ende muss nach dem Beginn liegen.');
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
      reminderDismissed: timingChanged ? false : original.reminderDismissed,
    );
    Navigator.pop(context, EventEditorResult(EditorAction.save, event));
  }

  Future<void> delete() async {
    final event = widget.event;
    if (event == null) return;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (dialogContext) => AlertDialog(
        title: const Text('Termin löschen?'),
        content: Text('„${event.title}“ wird endgültig entfernt.'),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(dialogContext, false),
            child: const Text('Abbrechen'),
          ),
          FilledButton(
            key: const Key('event-delete-confirm'),
            onPressed: () => Navigator.pop(dialogContext, true),
            child: const Text('Löschen'),
          ),
        ],
      ),
    );
    if (confirmed != true || !mounted) return;
    Navigator.pop(context, EventEditorResult(EditorAction.delete, event));
  }

  @override
  Widget build(BuildContext context) {
    final editing = widget.event != null;
    return EditorDialog(
      title: editing ? 'Termin bearbeiten' : 'Neuer Termin',
      icon: Icons.event_outlined,
      leading: [
        if (editing)
          TextButton.icon(
            key: const Key('event-delete'),
            onPressed: delete,
            icon: const Icon(Icons.delete_outline, size: 18),
            label: const Text('Löschen'),
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
              TextFormField(
                key: const Key('event-location'),
                controller: location,
                decoration: const InputDecoration(
                  labelText: 'Ort',
                  prefixIcon: Icon(Icons.place_outlined, size: 18),
                ),
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
              DropdownButtonFormField<int?>(
                isExpanded: true,
                key: const Key('event-reminder'),
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
}
