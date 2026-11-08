/// View models for the local calendar, task, and contact records.
///
/// Timestamps are kept as local `DateTime`s; the data source converts them
/// to and from the Unix milliseconds stored by the Rust core.
library;

import 'german_dates.dart';

/// Importance of a task, mirroring the three classic levels.
enum TaskPriority {
  low('low', 'Niedrig'),
  normal('normal', 'Normal'),
  high('high', 'Hoch');

  const TaskPriority(this.key, this.label);

  /// Stable key exchanged with the Rust core.
  final String key;
  final String label;

  static TaskPriority fromKey(String key) => values.firstWhere(
    (value) => value.key == key,
    orElse: () => TaskPriority.normal,
  );
}

/// Progress state of a task.
enum TaskStatus {
  notStarted('not_started', 'Nicht begonnen'),
  inProgress('in_progress', 'In Bearbeitung'),
  waiting('waiting', 'Wartet auf jemand anderen'),
  deferred('deferred', 'Zurückgestellt'),
  completed('completed', 'Erledigt');

  const TaskStatus(this.key, this.label);

  final String key;
  final String label;

  static TaskStatus fromKey(String key) => values.firstWhere(
    (value) => value.key == key,
    orElse: () => TaskStatus.notStarted,
  );
}

/// Kind of a contact detail such as an email address or phone number.
enum ContactLabel {
  work('work', 'Geschäftlich'),
  home('home', 'Privat'),
  mobile('mobile', 'Mobil'),
  fax('fax', 'Fax'),
  other('other', 'Sonstige');

  const ContactLabel(this.key, this.label);

  final String key;
  final String label;

  static ContactLabel fromKey(String key) => values.firstWhere(
    (value) => value.key == key,
    orElse: () => ContactLabel.other,
  );

  /// Labels offered for email addresses.
  static const emailLabels = [work, home, other];

  /// Labels offered for phone numbers.
  static const phoneLabels = [mobile, work, home, fax, other];

  /// Labels offered for postal addresses.
  static const addressLabels = [work, home, other];
}

/// How often a recurring event repeats.
enum RecurrenceFrequency {
  daily('daily', 'Täglich'),
  weekly('weekly', 'Wöchentlich'),
  monthly('monthly', 'Monatlich'),
  yearly('yearly', 'Jährlich');

  const RecurrenceFrequency(this.key, this.label);

  final String key;
  final String label;

  static RecurrenceFrequency fromKey(String key) => values.firstWhere(
    (value) => value.key == key,
    orElse: () => RecurrenceFrequency.weekly,
  );
}

/// Repetition pattern of an event.
class Recurrence {
  const Recurrence({
    required this.frequency,
    this.interval = 1,
    this.weekdays = const [],
    this.until,
    this.count,
  });

  final RecurrenceFrequency frequency;

  /// Every `interval` days, weeks, months, or years.
  final int interval;

  /// ISO weekdays (1 = Monday … 7 = Sunday) for weekly rules; empty means
  /// the weekday of the first occurrence.
  final List<int> weekdays;

  /// The last occurrence may start at or before this moment.
  final DateTime? until;

  /// Total number of occurrences including the first.
  final int? count;
}

/// What a synchronized collection holds.
enum CollectionKind {
  calendar('calendar', 'Kalender'),
  taskList('task_list', 'Aufgabenliste'),
  addressBook('address_book', 'Adressbuch');

  const CollectionKind(this.key, this.label);

  final String key;
  final String label;

  static CollectionKind fromKey(String key) => values.firstWhere(
    (value) => value.key == key,
    orElse: () => CollectionKind.calendar,
  );
}

/// One calendar, task list, or address book of a CalDAV/CardDAV account.
class WorkspaceCollection {
  const WorkspaceCollection({
    required this.id,
    required this.accountId,
    required this.kind,
    required this.name,
    this.color,
    this.enabled = true,
  });

  final String id;
  final String accountId;
  final CollectionKind kind;
  final String name;

  /// CSS-style color such as `#0F6CBD`, when the server supplies one.
  final String? color;
  final bool enabled;

  /// Whether events can be stored here.
  bool get holdsEvents => kind == CollectionKind.calendar;

  /// Whether tasks can be stored here (calendars accept VTODO too).
  bool get holdsTasks =>
      kind == CollectionKind.calendar || kind == CollectionKind.taskList;

  bool get holdsContacts => kind == CollectionKind.addressBook;

  WorkspaceCollection copyWith({bool? enabled}) => WorkspaceCollection(
    id: id,
    accountId: accountId,
    kind: kind,
    name: name,
    color: color,
    enabled: enabled ?? this.enabled,
  );
}

/// Configuration of one CalDAV/CardDAV account; the password stays in the
/// encrypted profile.
class DavAccountConfig {
  const DavAccountConfig({
    required this.id,
    required this.displayName,
    required this.serverUrl,
    required this.username,
    this.lastSyncAt,
  });

  final String id;
  final String displayName;
  final String serverUrl;
  final String username;
  final DateTime? lastSyncAt;
}

/// One collection a connection test found on a server.
class DiscoveredCollection {
  const DiscoveredCollection({
    required this.kind,
    required this.href,
    required this.name,
  });

  final CollectionKind kind;
  final String href;
  final String name;
}

/// Reminder lead times offered for calendar events, in minutes.
const eventReminderChoices = <int>[0, 5, 10, 15, 30, 60, 120, 1440, 2880];

/// Human-readable label for a reminder lead time in minutes.
String reminderLeadLabel(int? minutes) {
  if (minutes == null) return 'Keine';
  if (minutes == 0) return 'Zum Beginn';
  if (minutes < 60) return '$minutes Minuten vorher';
  if (minutes % 1440 == 0) {
    final days = minutes ~/ 1440;
    return days == 1 ? '1 Tag vorher' : '$days Tage vorher';
  }
  if (minutes % 60 == 0) {
    final hours = minutes ~/ 60;
    return hours == 1 ? '1 Stunde vorher' : '$hours Stunden vorher';
  }
  return '$minutes Minuten vorher';
}

class LocalCalendarItem {
  const LocalCalendarItem({
    required this.id,
    required this.title,
    required this.startsAt,
    required this.endsAt,
    this.allDay = false,
    this.location,
    this.description,
    this.reminderMinutesBefore,
    this.reminderDismissedThrough,
    this.recurrence,
    this.exceptionDates = const [],
    this.collectionId,
  });

  final String id;
  final String title;

  /// Start of the event. For all-day events this is local midnight of the
  /// first day.
  final DateTime startsAt;

  /// End of the event. For all-day events this is local midnight after the
  /// last day, so `endsAt.difference(startsAt)` counts whole days.
  final DateTime endsAt;
  final bool allDay;
  final String? location;
  final String? description;

  /// Minutes before the start at which the reminder is due; null disables it.
  final int? reminderMinutesBefore;

  /// Occurrences starting at or before this moment had their reminder
  /// closed by the user.
  final DateTime? reminderDismissedThrough;

  /// Repetition pattern; null for a single event.
  final Recurrence? recurrence;

  /// ISO dates (`YYYY-MM-DD`) of occurrences removed from a recurring event.
  final List<String> exceptionDates;

  /// Calendar the event is synchronized with; null for the local calendar.
  final String? collectionId;

  bool get recurring => recurrence != null;

  /// Whether the reminder of the first occurrence was closed.
  bool get reminderDismissed {
    final through = reminderDismissedThrough;
    return through != null && !startsAt.isAfter(through);
  }

  int get day => startsAt.day;

  /// Moment at which the reminder becomes due, if one is set.
  DateTime? get reminderAt {
    final minutes = reminderMinutesBefore;
    if (minutes == null) return null;
    return startsAt.subtract(Duration(minutes: minutes));
  }

  /// Last calendar day the event touches.
  DateTime get lastDay {
    final inclusiveEnd = allDay
        ? endsAt.subtract(const Duration(minutes: 1))
        : endsAt;
    // A timed event ending exactly at midnight belongs to the previous day.
    final adjusted =
        !allDay &&
            inclusiveEnd.hour == 0 &&
            inclusiveEnd.minute == 0 &&
            inclusiveEnd.isAfter(startsAt)
        ? inclusiveEnd.subtract(const Duration(minutes: 1))
        : inclusiveEnd;
    return dateOnly(adjusted);
  }

  /// Whether the event touches the calendar day `day`.
  bool occursOn(DateTime day) {
    final target = dateOnly(day);
    return !target.isBefore(dateOnly(startsAt)) && !target.isAfter(lastDay);
  }

  /// Compact time range such as `09:30–10:00` or `Ganztägig`.
  String get time {
    if (allDay) return 'Ganztägig';
    return '${formatClock(startsAt)}–${formatClock(endsAt)}';
  }

  LocalCalendarItem copyWith({
    String? title,
    DateTime? startsAt,
    DateTime? endsAt,
    bool? allDay,
    String? location,
    String? description,
    int? reminderMinutesBefore,
    bool clearReminder = false,
    DateTime? reminderDismissedThrough,
    bool clearReminderDismissed = false,
    Recurrence? recurrence,
    bool clearRecurrence = false,
    List<String>? exceptionDates,
    String? collectionId,
    bool clearCollection = false,
  }) {
    return LocalCalendarItem(
      id: id,
      title: title ?? this.title,
      startsAt: startsAt ?? this.startsAt,
      endsAt: endsAt ?? this.endsAt,
      allDay: allDay ?? this.allDay,
      location: location ?? this.location,
      description: description ?? this.description,
      reminderMinutesBefore: clearReminder
          ? null
          : reminderMinutesBefore ?? this.reminderMinutesBefore,
      reminderDismissedThrough: clearReminderDismissed
          ? null
          : reminderDismissedThrough ?? this.reminderDismissedThrough,
      recurrence: clearRecurrence ? null : recurrence ?? this.recurrence,
      exceptionDates: exceptionDates ?? this.exceptionDates,
      collectionId: clearCollection ? null : collectionId ?? this.collectionId,
    );
  }
}

class LocalTaskItem {
  const LocalTaskItem({
    required this.id,
    required this.title,
    this.description,
    this.startsAt,
    this.dueAt,
    this.status = TaskStatus.notStarted,
    this.priority = TaskPriority.normal,
    this.reminderAt,
    this.reminderDismissed = false,
    this.completedAt,
    this.collectionId,
  });

  final String id;
  final String title;
  final String? description;
  final DateTime? startsAt;
  final DateTime? dueAt;
  final TaskStatus status;
  final TaskPriority priority;
  final DateTime? reminderAt;
  final bool reminderDismissed;
  final DateTime? completedAt;

  /// Task list the task is synchronized with; null for the local list.
  final String? collectionId;

  bool get done => status == TaskStatus.completed;

  /// Whether the due date lies in the past and the task is still open.
  bool get overdue {
    final due = dueAt;
    if (due == null || done) return false;
    return dateOnly(due).isBefore(dateOnly(DateTime.now()));
  }

  /// Short due label: `Heute`, `Morgen`, a date, or the state.
  String get due {
    final value = dueAt;
    if (value == null) return done ? 'Erledigt' : 'Ohne Datum';
    return relativeDayLabel(value);
  }

  LocalTaskItem copyWith({
    String? title,
    String? description,
    bool clearDescription = false,
    DateTime? startsAt,
    bool clearStartsAt = false,
    DateTime? dueAt,
    bool clearDueAt = false,
    TaskStatus? status,
    TaskPriority? priority,
    DateTime? reminderAt,
    bool clearReminder = false,
    bool? reminderDismissed,
    DateTime? completedAt,
    bool clearCompletedAt = false,
    String? collectionId,
    bool clearCollection = false,
  }) {
    return LocalTaskItem(
      id: id,
      title: title ?? this.title,
      description: clearDescription ? null : description ?? this.description,
      startsAt: clearStartsAt ? null : startsAt ?? this.startsAt,
      dueAt: clearDueAt ? null : dueAt ?? this.dueAt,
      status: status ?? this.status,
      priority: priority ?? this.priority,
      reminderAt: clearReminder ? null : reminderAt ?? this.reminderAt,
      reminderDismissed: reminderDismissed ?? this.reminderDismissed,
      completedAt: clearCompletedAt ? null : completedAt ?? this.completedAt,
      collectionId: clearCollection ? null : collectionId ?? this.collectionId,
    );
  }

  /// Marks the task completed or reopens it, keeping the completion time in
  /// sync.
  LocalTaskItem withDone(bool done, {DateTime? now}) {
    if (done) {
      return copyWith(
        status: TaskStatus.completed,
        completedAt: now ?? DateTime.now(),
      );
    }
    return copyWith(status: TaskStatus.notStarted, clearCompletedAt: true);
  }
}

class ContactEmail {
  const ContactEmail({required this.label, required this.address});

  final ContactLabel label;
  final String address;
}

class ContactPhone {
  const ContactPhone({required this.label, required this.number});

  final ContactLabel label;
  final String number;
}

class ContactAddress {
  const ContactAddress({
    required this.label,
    this.street = '',
    this.postalCode = '',
    this.city = '',
    this.country = '',
  });

  final ContactLabel label;
  final String street;
  final String postalCode;
  final String city;
  final String country;

  bool get isEmpty =>
      street.isEmpty && postalCode.isEmpty && city.isEmpty && country.isEmpty;

  /// Address lines in the usual German postal order.
  List<String> get lines => [
    if (street.isNotEmpty) street,
    if (postalCode.isNotEmpty || city.isNotEmpty)
      [postalCode, city].where((part) => part.isNotEmpty).join(' '),
    if (country.isNotEmpty) country,
  ];
}

class LocalContactItem {
  const LocalContactItem({
    required this.id,
    required this.name,
    this.company,
    this.jobTitle,
    this.emails = const [],
    this.phones = const [],
    this.addresses = const [],
    this.birthday,
    this.website,
    this.notes,
    this.collectionId,
  });

  final String id;
  final String name;
  final String? company;
  final String? jobTitle;
  final List<ContactEmail> emails;
  final List<ContactPhone> phones;
  final List<ContactAddress> addresses;

  /// Calendar date without a time; only year, month, and day are meaningful.
  final DateTime? birthday;
  final String? website;
  final String? notes;

  /// Address book the contact is synchronized with; null for the local one.
  final String? collectionId;

  /// The first email address, or an empty string.
  String get email => emails.isEmpty ? '' : emails.first.address;

  /// Recipient in `Name <address>` form for the composer, if any address is
  /// known.
  String? get primaryRecipient {
    if (emails.isEmpty) return null;
    return '$name <${emails.first.address}>';
  }

  /// Second line shown under the name in lists.
  String get subtitle {
    final parts = [
      if (jobTitle case final title? when title.isNotEmpty) title,
      if (company case final company? when company.isNotEmpty) company,
    ];
    if (parts.isNotEmpty) return parts.join(' · ');
    if (emails.isNotEmpty) return emails.first.address;
    if (phones.isNotEmpty) return phones.first.number;
    return '';
  }

  /// Whether the contact matches a free-text search.
  bool matches(String query) {
    final needle = query.trim().toLowerCase();
    if (needle.isEmpty) return true;
    final haystack = [
      name,
      company ?? '',
      jobTitle ?? '',
      ...emails.map((email) => email.address),
      ...phones.map((phone) => phone.number),
      ...addresses.expand((address) => address.lines),
      notes ?? '',
    ].join('\n').toLowerCase();
    return haystack.contains(needle);
  }
}
