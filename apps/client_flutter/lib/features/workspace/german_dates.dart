/// German date and time formatting shared by the calendar, task, and
/// contact modules, plus text fields that accept typed dates and times and
/// offer the Material pickers as a shortcut.
library;

import 'package:flutter/material.dart';

const germanWeekdays = [
  'Montag',
  'Dienstag',
  'Mittwoch',
  'Donnerstag',
  'Freitag',
  'Samstag',
  'Sonntag',
];

const germanWeekdaysShort = ['Mo', 'Di', 'Mi', 'Do', 'Fr', 'Sa', 'So'];

const germanMonths = [
  'Januar',
  'Februar',
  'März',
  'April',
  'Mai',
  'Juni',
  'Juli',
  'August',
  'September',
  'Oktober',
  'November',
  'Dezember',
];

String _two(int value) => value.toString().padLeft(2, '0');

/// Midnight of the given day in local time.
DateTime dateOnly(DateTime value) =>
    DateTime(value.year, value.month, value.day);

/// Monday of the week that contains `value`, at midnight.
DateTime startOfWeek(DateTime value) =>
    dateOnly(value).subtract(Duration(days: value.weekday - 1));

bool isSameDay(DateTime a, DateTime b) =>
    a.year == b.year && a.month == b.month && a.day == b.day;

/// `27.07.2026`
String formatDate(DateTime value) =>
    '${_two(value.day)}.${_two(value.month)}.${value.year}';

/// `09:30`
String formatClock(DateTime value) =>
    '${_two(value.hour)}:${_two(value.minute)}';

/// `27.07.2026 09:30`
String formatDateTime(DateTime value) =>
    '${formatDate(value)} ${formatClock(value)}';

/// `Montag, 27. Juli 2026`
String formatLongDate(DateTime value) =>
    '${germanWeekdays[value.weekday - 1]}, ${value.day}. '
    '${germanMonths[value.month - 1]} ${value.year}';

/// `27. Juli 2026`
String formatMediumDate(DateTime value) =>
    '${value.day}. ${germanMonths[value.month - 1]} ${value.year}';

/// `Juli 2026`
String formatMonthYear(DateTime value) =>
    '${germanMonths[value.month - 1]} ${value.year}';

/// `Heute`, `Morgen`, `Gestern`, or a short date.
String relativeDayLabel(DateTime value, {DateTime? now}) {
  final today = dateOnly(now ?? DateTime.now());
  final difference = dateOnly(value).difference(today).inDays;
  if (difference == 0) return 'Heute';
  if (difference == 1) return 'Morgen';
  if (difference == -1) return 'Gestern';
  return formatDate(value);
}

/// Parses `d.M.yyyy` or `dd.MM.yyyy`; returns null for anything else.
DateTime? parseGermanDate(String input) {
  final match = RegExp(
    r'^\s*(\d{1,2})\.(\d{1,2})\.(\d{4})\s*$',
  ).firstMatch(input);
  if (match == null) return null;
  final day = int.parse(match.group(1)!);
  final month = int.parse(match.group(2)!);
  final year = int.parse(match.group(3)!);
  if (month < 1 || month > 12 || day < 1 || day > 31) return null;
  final parsed = DateTime(year, month, day);
  if (parsed.month != month || parsed.day != day) return null;
  return parsed;
}

/// Parses `H:mm` or `HH:mm`; returns null for anything else.
TimeOfDay? parseClock(String input) {
  final match = RegExp(r'^\s*(\d{1,2}):(\d{2})\s*$').firstMatch(input);
  if (match == null) return null;
  final hour = int.parse(match.group(1)!);
  final minute = int.parse(match.group(2)!);
  if (hour > 23 || minute > 59) return null;
  return TimeOfDay(hour: hour, minute: minute);
}

/// Parses an ISO calendar date (`YYYY-MM-DD`) as a local midnight.
DateTime? parseIsoDate(String? input) {
  if (input == null) return null;
  final match = RegExp(r'^(\d{4})-(\d{2})-(\d{2})$').firstMatch(input);
  if (match == null) return null;
  return DateTime(
    int.parse(match.group(1)!),
    int.parse(match.group(2)!),
    int.parse(match.group(3)!),
  );
}

/// Formats a calendar date as `YYYY-MM-DD`.
String formatIsoDate(DateTime value) =>
    '${value.year.toString().padLeft(4, '0')}-${_two(value.month)}-${_two(value.day)}';

DateTime combine(DateTime day, TimeOfDay time) =>
    DateTime(day.year, day.month, day.day, time.hour, time.minute);

/// Text field for a date in `dd.MM.yyyy` form with a calendar picker.
class DateField extends StatelessWidget {
  const DateField({
    super.key,
    required this.controller,
    required this.label,
    this.enabled = true,
    this.required = false,
    this.onChanged,
    this.fieldKey,
  });

  final TextEditingController controller;
  final String label;
  final bool enabled;
  final bool required;
  final VoidCallback? onChanged;
  final Key? fieldKey;

  @override
  Widget build(BuildContext context) {
    return TextFormField(
      key: fieldKey,
      controller: controller,
      enabled: enabled,
      decoration: InputDecoration(
        labelText: label,
        hintText: 'TT.MM.JJJJ',
        suffixIcon: IconButton(
          tooltip: 'Datum wählen',
          icon: const Icon(Icons.calendar_today_outlined, size: 18),
          onPressed: enabled
              ? () async {
                  final initial =
                      parseGermanDate(controller.text) ?? DateTime.now();
                  final picked = await showDatePicker(
                    context: context,
                    initialDate: initial,
                    firstDate: DateTime(1900),
                    lastDate: DateTime(2100),
                  );
                  if (picked == null) return;
                  controller.text = formatDate(picked);
                  onChanged?.call();
                }
              : null,
        ),
      ),
      onChanged: (_) => onChanged?.call(),
      validator: (value) {
        final text = value?.trim() ?? '';
        if (text.isEmpty) {
          return required ? 'Bitte ein Datum eingeben.' : null;
        }
        return parseGermanDate(text) == null ? 'Format: TT.MM.JJJJ' : null;
      },
    );
  }
}

/// Text field for a time in `HH:mm` form with a clock picker.
class TimeField extends StatelessWidget {
  const TimeField({
    super.key,
    required this.controller,
    required this.label,
    this.enabled = true,
    this.required = false,
    this.onChanged,
    this.fieldKey,
  });

  final TextEditingController controller;
  final String label;
  final bool enabled;
  final bool required;
  final VoidCallback? onChanged;
  final Key? fieldKey;

  @override
  Widget build(BuildContext context) {
    return TextFormField(
      key: fieldKey,
      controller: controller,
      enabled: enabled,
      decoration: InputDecoration(
        labelText: label,
        hintText: 'HH:MM',
        suffixIcon: IconButton(
          tooltip: 'Uhrzeit wählen',
          icon: const Icon(Icons.schedule_outlined, size: 18),
          onPressed: enabled
              ? () async {
                  final initial =
                      parseClock(controller.text) ??
                      const TimeOfDay(hour: 9, minute: 0);
                  final picked = await showTimePicker(
                    context: context,
                    initialTime: initial,
                  );
                  if (picked == null) return;
                  controller.text =
                      '${_two(picked.hour)}:${_two(picked.minute)}';
                  onChanged?.call();
                }
              : null,
        ),
      ),
      onChanged: (_) => onChanged?.call(),
      validator: (value) {
        final text = value?.trim() ?? '';
        if (text.isEmpty) {
          return required ? 'Bitte eine Uhrzeit eingeben.' : null;
        }
        return parseClock(text) == null ? 'Format: HH:MM' : null;
      },
    );
  }
}

/// Standard frame for the item editors: a wide, scrollable dialog with a
/// title row, optional leading actions (such as delete) on the left, and
/// the confirming actions on the right.
class EditorDialog extends StatelessWidget {
  const EditorDialog({
    super.key,
    required this.title,
    required this.icon,
    required this.children,
    required this.actions,
    this.leading = const [],
    this.width = 620,
  });

  final String title;
  final IconData icon;
  final List<Widget> children;
  final List<Widget> actions;
  final List<Widget> leading;
  final double width;

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      titlePadding: const EdgeInsets.fromLTRB(24, 20, 24, 0),
      contentPadding: const EdgeInsets.fromLTRB(24, 16, 24, 12),
      title: Row(
        children: [
          Icon(icon, size: 22, color: Theme.of(context).colorScheme.primary),
          const SizedBox(width: 10),
          Expanded(child: Text(title)),
        ],
      ),
      content: SizedBox(
        width: width,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Flexible(
              child: SingleChildScrollView(
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: children,
                ),
              ),
            ),
            const SizedBox(height: 16),
            Row(
              children: [
                ...leading,
                const Spacer(),
                for (final action in actions)
                  Padding(
                    padding: const EdgeInsets.only(left: 8),
                    child: action,
                  ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}
