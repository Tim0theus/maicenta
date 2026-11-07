import 'dart:async';
import 'dart:convert';
import 'dart:io';

import '../../src/rust/api/updates.dart' as rust;

/// A newer MAICENTA release offered to the user.
class AvailableAppUpdate {
  const AvailableAppUpdate({
    required this.version,
    required this.currentVersion,
    this.releaseNotes = '',
    this.downloadSizeBytes = 0,
    this.downloadUrl,
    this.releasePageUrl,
  });

  final String version;
  final String currentVersion;

  /// Markdown from the CHANGELOG section of the release; may be empty.
  final String releaseNotes;
  final int downloadSizeBytes;

  /// Set for releases that are installed by hand (see [AppUpdater]).
  final String? downloadUrl;
  final String? releasePageUrl;
}

/// Platform side of the in-app update: finding, downloading and installing a
/// release. Implemented by Velopack in the Rust core; tests use fakes.
abstract class AppUpdater {
  /// Whether this installation can update itself. False for development
  /// builds and for copies that were not installed by the MAICENTA installer.
  bool get supported;

  /// Whether [download] and [applyAndRestart] work. When false the user is
  /// pointed to the download instead.
  bool get installsInApp;

  Future<AvailableAppUpdate?> check();

  /// Downloads the update found by the last [check] and emits progress from
  /// 0 to 100. The stream completes when the package is verified.
  Stream<int> download();

  /// Quits MAICENTA, installs the downloaded update and starts the new
  /// version. Does not return on success.
  Future<void> applyAndRestart();
}

class RustAppUpdater implements AppUpdater {
  RustAppUpdater();

  @override
  late final bool supported = rust.appUpdatesSupported();

  @override
  bool get installsInApp => true;

  @override
  Future<AvailableAppUpdate?> check() async {
    final update = await rust.checkForAppUpdate();
    if (update == null) return null;
    return AvailableAppUpdate(
      version: update.version,
      currentVersion: update.currentVersion,
      releaseNotes: update.releaseNotesMarkdown.trim(),
      downloadSizeBytes: update.downloadSizeBytes.toInt(),
    );
  }

  @override
  Stream<int> download() => rust.downloadAppUpdate();

  @override
  Future<void> applyAndRestart() => rust.applyAppUpdateAndRestart();
}

/// Reports new releases without installing them, for the sandboxed macOS
/// app that Velopack cannot replace. The user downloads the DMG.
class ReleaseNoticeUpdater implements AppUpdater {
  ReleaseNoticeUpdater({
    required this.currentVersion,
    required this.assetSuffix,
    required this.supported,
  });

  final String currentVersion;

  /// Ending of the release asset to offer, e.g. `-macos.dmg`.
  final String assetSuffix;

  @override
  final bool supported;

  @override
  bool get installsInApp => false;

  @override
  Future<AvailableAppUpdate?> check() async {
    final notice = await rust.checkForReleaseNotice(
      currentVersion: currentVersion,
      assetSuffix: assetSuffix,
    );
    if (notice == null) return null;
    return AvailableAppUpdate(
      version: notice.version,
      currentVersion: notice.currentVersion,
      releaseNotes: notice.releaseNotesMarkdown.trim(),
      downloadSizeBytes: notice.downloadSizeBytes.toInt(),
      downloadUrl: notice.downloadUrl,
      releasePageUrl: notice.releasePageUrl,
    );
  }

  @override
  Stream<int> download() =>
      Stream.error(UnsupportedError('install this release by hand'));

  @override
  Future<void> applyAndRestart() =>
      Future.error(UnsupportedError('install this release by hand'));
}

/// Update settings of this installation. They are deliberately kept outside
/// the profile: they describe the installed app, so a profile export or
/// import must not carry them to another machine.
class UpdatePreferences {
  const UpdatePreferences({
    this.automaticChecks = true,
    this.skippedVersion,
    this.lastCheckedAt,
  });

  factory UpdatePreferences.fromJson(Map<String, Object?> json) {
    final lastChecked = json['lastCheckedAt'];
    final skipped = json['skippedVersion'];
    return UpdatePreferences(
      automaticChecks: json['automaticChecks'] != false,
      skippedVersion: skipped is String && skipped.isNotEmpty ? skipped : null,
      lastCheckedAt: lastChecked is String
          ? DateTime.tryParse(lastChecked)
          : null,
    );
  }

  final bool automaticChecks;

  /// Version the user chose to skip; automatic checks stay quiet about it.
  final String? skippedVersion;
  final DateTime? lastCheckedAt;

  UpdatePreferences copyWith({
    bool? automaticChecks,
    String? skippedVersion,
    DateTime? lastCheckedAt,
  }) => UpdatePreferences(
    automaticChecks: automaticChecks ?? this.automaticChecks,
    skippedVersion: skippedVersion ?? this.skippedVersion,
    lastCheckedAt: lastCheckedAt ?? this.lastCheckedAt,
  );

  Map<String, Object?> toJson() => {
    'automaticChecks': automaticChecks,
    if (skippedVersion != null) 'skippedVersion': skippedVersion,
    if (lastCheckedAt != null)
      'lastCheckedAt': lastCheckedAt!.toUtc().toIso8601String(),
  };
}

abstract class UpdatePreferencesStore {
  Future<UpdatePreferences> load();
  Future<void> save(UpdatePreferences preferences);
}

/// Stores [UpdatePreferences] as JSON next to the profile database.
class FileUpdatePreferencesStore implements UpdatePreferencesStore {
  FileUpdatePreferencesStore(this.file);

  final File file;

  @override
  Future<UpdatePreferences> load() async {
    try {
      final decoded = jsonDecode(await file.readAsString());
      if (decoded is Map<String, Object?>) {
        return UpdatePreferences.fromJson(decoded);
      }
    } on Object {
      // Missing or unreadable settings fall back to the defaults.
    }
    return const UpdatePreferences();
  }

  @override
  Future<void> save(UpdatePreferences preferences) async {
    final temporary = File('${file.path}.tmp');
    await temporary.writeAsString(
      jsonEncode(preferences.toJson()),
      flush: true,
    );
    await temporary.rename(file.path);
  }
}

class MemoryUpdatePreferencesStore implements UpdatePreferencesStore {
  MemoryUpdatePreferencesStore([this.preferences = const UpdatePreferences()]);

  UpdatePreferences preferences;

  @override
  Future<UpdatePreferences> load() async => preferences;

  @override
  Future<void> save(UpdatePreferences preferences) async {
    this.preferences = preferences;
  }
}

/// Combines the updater with the user's update settings and decides when an
/// automatic check is due.
class AppUpdateService {
  AppUpdateService({
    required this.updater,
    required this.store,
    UpdatePreferences initial = const UpdatePreferences(),
    DateTime Function()? clock,
  }) : _preferences = initial,
       _clock = clock ?? DateTime.now;

  static Future<AppUpdateService> open({
    required AppUpdater updater,
    required UpdatePreferencesStore store,
  }) async => AppUpdateService(
    updater: updater,
    store: store,
    initial: await store.load(),
  );

  /// Automatic checks run at most this often; the GitHub API allows 60
  /// unauthenticated requests per hour and address.
  static const checkInterval = Duration(hours: 24);

  final AppUpdater updater;
  final UpdatePreferencesStore store;
  final DateTime Function() _clock;
  UpdatePreferences _preferences;

  UpdatePreferences get preferences => _preferences;
  bool get supported => updater.supported;

  bool get automaticCheckDue {
    if (!supported || !_preferences.automaticChecks) return false;
    final last = _preferences.lastCheckedAt;
    return last == null || _clock().difference(last) >= checkInterval;
  }

  Future<void> setAutomaticChecks(bool enabled) =>
      _update(_preferences.copyWith(automaticChecks: enabled));

  Future<void> skipVersion(String version) =>
      _update(_preferences.copyWith(skippedVersion: version));

  /// Looks for an update. Automatic checks ignore a skipped version; a check
  /// the user started always reports what is available.
  Future<AvailableAppUpdate?> check({required bool automatic}) async {
    final update = await updater.check();
    await _update(_preferences.copyWith(lastCheckedAt: _clock()));
    if (update == null) return null;
    if (automatic && update.version == _preferences.skippedVersion) {
      return null;
    }
    return update;
  }

  Future<void> _update(UpdatePreferences next) async {
    _preferences = next;
    try {
      await store.save(next);
    } on Object {
      // Losing the settings only means another prompt later; never block.
    }
  }
}

/// `42,5 MB`; empty when the size is unknown.
String formatDownloadSize(int bytes) {
  if (bytes <= 0) return '';
  final megabytes = bytes / (1024 * 1024);
  final text = megabytes >= 10
      ? megabytes.toStringAsFixed(0)
      : megabytes.toStringAsFixed(1).replaceAll('.', ',');
  return '$text MB';
}
