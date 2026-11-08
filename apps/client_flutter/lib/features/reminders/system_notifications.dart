/// Operating-system notifications for reminders, so a reminder reaches the
/// user while MAICENTA sits in the background, and, on macOS and Windows,
/// even after the app was closed: upcoming reminders are handed to the
/// system ahead of time and re-planned whenever events or tasks change.
library;

import 'dart:io' show Platform;

import 'package:flutter/foundation.dart';
import 'package:flutter_local_notifications/flutter_local_notifications.dart';
import 'package:timezone/data/latest.dart' as tz_data;
import 'package:timezone/timezone.dart' as tz;

/// One reminder the system should announce.
class PlannedNotification {
  const PlannedNotification({
    required this.id,
    required this.title,
    required this.body,
    required this.at,
  });

  /// Stable key: the event or task identifier, with the occurrence for
  /// recurring events.
  final String id;
  final String title;
  final String body;
  final DateTime at;
}

/// Boundary the workspace talks to; tests use a recording fake.
abstract interface class SystemNotifier {
  /// Whether the platform can show notifications at all.
  bool get supported;

  /// Shows a notification right now.
  Future<void> show(PlannedNotification notification);

  /// Replaces every planned notification with the given set.
  Future<void> schedule(List<PlannedNotification> notifications);
}

/// A notifier that does nothing, for platforms or builds without support.
class NoSystemNotifier implements SystemNotifier {
  const NoSystemNotifier();

  @override
  bool get supported => false;

  @override
  Future<void> show(PlannedNotification notification) async {}

  @override
  Future<void> schedule(List<PlannedNotification> notifications) async {}
}

/// Placeholder that forwards to a real notifier once it is ready, so the
/// user interface never waits for plugin initialization or a permission
/// prompt. Calls made before that are kept and replayed.
class DeferredSystemNotifier implements SystemNotifier {
  SystemNotifier _inner = const NoSystemNotifier();
  bool _ready = false;
  final List<PlannedNotification> _pendingShows = [];
  List<PlannedNotification>? _pendingSchedule;

  /// Adopts the notifier produced by [factory] when it completes.
  Future<void> attach(Future<SystemNotifier> factory) async {
    try {
      _inner = await factory;
    } on Object {
      _inner = const NoSystemNotifier();
    }
    _ready = true;
    for (final notification in _pendingShows) {
      await _inner.show(notification);
    }
    _pendingShows.clear();
    if (_pendingSchedule case final planned?) {
      _pendingSchedule = null;
      await _inner.schedule(planned);
    }
  }

  @override
  bool get supported => _inner.supported;

  @override
  Future<void> show(PlannedNotification notification) async {
    if (!_ready) {
      _pendingShows.add(notification);
      return;
    }
    await _inner.show(notification);
  }

  @override
  Future<void> schedule(List<PlannedNotification> notifications) async {
    if (!_ready) {
      _pendingSchedule = notifications;
      return;
    }
    await _inner.schedule(notifications);
  }
}

/// Notifier backed by `flutter_local_notifications`.
///
/// Scheduling ahead works on macOS and Windows; Linux shows notifications
/// only while the app runs, so there `schedule` is a no-op and the in-app
/// timer calls `show` when a reminder becomes due.
class LocalSystemNotifier implements SystemNotifier {
  LocalSystemNotifier._(this._plugin, this._canSchedule);

  final FlutterLocalNotificationsPlugin _plugin;
  final bool _canSchedule;

  /// Identifiers of the notifications planned by the last `schedule` call,
  /// so they can be cancelled one by one where `cancelAll` is unavailable.
  final List<int> _planned = [];

  /// Highest number of reminders handed to the system at once; macOS keeps
  /// at most 64 pending notifications per app.
  static const maxPlanned = 60;

  /// Initializes the platform plugin. Returns a [NoSystemNotifier] when the
  /// platform is unsupported or initialization fails, so reminders keep
  /// working inside the app.
  static Future<SystemNotifier> open({
    required void Function(String payload) onSelected,
  }) async {
    if (kIsWeb ||
        !(Platform.isMacOS || Platform.isLinux || Platform.isWindows)) {
      return const NoSystemNotifier();
    }
    try {
      tz_data.initializeTimeZones();
      final plugin = FlutterLocalNotificationsPlugin();
      final initialized = await plugin.initialize(
        settings: const InitializationSettings(
          macOS: DarwinInitializationSettings(
            requestAlertPermission: true,
            requestSoundPermission: true,
            requestBadgePermission: false,
          ),
          linux: LinuxInitializationSettings(defaultActionName: 'Öffnen'),
          windows: WindowsInitializationSettings(
            appName: 'MAICENTA',
            appUserModelId: 'eu.be-rich.maicenta',
            guid: '7f1c4c2e-5b1a-4b7a-9c7e-3d9a8f6e2b10',
          ),
        ),
        onDidReceiveNotificationResponse: (response) {
          final payload = response.payload;
          if (payload != null && payload.isNotEmpty) onSelected(payload);
        },
      );
      if (initialized == false) return const NoSystemNotifier();
      return LocalSystemNotifier._(
        plugin,
        Platform.isMacOS || Platform.isWindows,
      );
    } on Object {
      return const NoSystemNotifier();
    }
  }

  static int _numericId(String key) {
    // Stable 31-bit hash so the same reminder replaces its own notification.
    var hash = 0x811c9dc5;
    for (final unit in key.codeUnits) {
      hash = ((hash ^ unit) * 0x01000193) & 0x7fffffff;
    }
    return hash;
  }

  static const _details = NotificationDetails(
    macOS: DarwinNotificationDetails(presentAlert: true, presentSound: true),
    linux: LinuxNotificationDetails(),
    windows: WindowsNotificationDetails(),
  );

  @override
  bool get supported => true;

  @override
  Future<void> show(PlannedNotification notification) async {
    try {
      await _plugin.show(
        id: _numericId('now:${notification.id}'),
        title: notification.title,
        body: notification.body,
        notificationDetails: _details,
        payload: notification.id,
      );
    } on Object {
      // A failed banner must never break the reminder window.
    }
  }

  @override
  Future<void> schedule(List<PlannedNotification> notifications) async {
    if (!_canSchedule) return;
    try {
      for (final id in _planned) {
        await _plugin.cancel(id: id);
      }
      _planned.clear();
      final now = DateTime.now();
      final upcoming =
          notifications.where((item) => item.at.isAfter(now)).toList()
            ..sort((a, b) => a.at.compareTo(b.at));
      for (final notification in upcoming.take(maxPlanned)) {
        final id = _numericId('planned:${notification.id}');
        await _plugin.zonedSchedule(
          id: id,
          title: notification.title,
          body: notification.body,
          scheduledDate: tz.TZDateTime.from(notification.at.toUtc(), tz.UTC),
          notificationDetails: _details,
          androidScheduleMode: AndroidScheduleMode.inexactAllowWhileIdle,
          payload: notification.id,
        );
        _planned.add(id);
      }
    } on Object {
      // Scheduling is best effort; the in-app reminder window remains.
    }
  }
}
