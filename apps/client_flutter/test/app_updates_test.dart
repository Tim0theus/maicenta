import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:maicenta/features/updates/app_updates.dart';
import 'package:maicenta/main.dart';

const _update = AvailableAppUpdate(
  version: '0.1.0-beta.2',
  currentVersion: '0.1.0-beta.1',
  releaseNotes: '### Behoben\n- Absturz beim Start',
  downloadSizeBytes: 48 * 1024 * 1024,
);

class FakeAppUpdater implements AppUpdater {
  FakeAppUpdater({
    this.supported = true,
    this.installsInApp = true,
    this.available = _update,
  });

  @override
  final bool supported;
  @override
  final bool installsInApp;
  AvailableAppUpdate? available;
  int checks = 0;
  int applies = 0;
  StreamController<int>? downloadController;

  @override
  Future<AvailableAppUpdate?> check() async {
    checks += 1;
    return available;
  }

  @override
  Stream<int> download() {
    downloadController = StreamController<int>();
    return downloadController!.stream;
  }

  @override
  Future<void> applyAndRestart() async {
    applies += 1;
  }
}

Future<void> _pumpApp(WidgetTester tester, AppUpdateService service) async {
  tester.view.physicalSize = const Size(1440, 900);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
  await tester.pumpWidget(MaicentaApp(appUpdates: service));
  await tester.pump();
  await tester.pump();
}

Future<void> _openOptions(WidgetTester tester) async {
  await tester.tap(find.byKey(const Key('ribbon-tab-Datei')));
  await tester.pump();
  await tester.tap(find.byKey(const Key('ribbon-action-Optionen')));
  await tester.pumpAndSettle();
}

void main() {
  group('AppUpdateService', () {
    test('checks automatically at most once a day', () async {
      var now = DateTime.utc(2026, 11, 4, 9);
      final service = AppUpdateService(
        updater: FakeAppUpdater(),
        store: MemoryUpdatePreferencesStore(),
        clock: () => now,
      );

      expect(service.automaticCheckDue, isTrue);
      await service.check(automatic: true);
      expect(service.automaticCheckDue, isFalse);
      now = now.add(const Duration(hours: 23));
      expect(service.automaticCheckDue, isFalse);
      now = now.add(const Duration(hours: 1));
      expect(service.automaticCheckDue, isTrue);
    });

    test('never checks automatically when disabled or unsupported', () async {
      final disabled = AppUpdateService(
        updater: FakeAppUpdater(),
        store: MemoryUpdatePreferencesStore(),
      );
      await disabled.setAutomaticChecks(false);
      expect(disabled.automaticCheckDue, isFalse);

      final unsupported = AppUpdateService(
        updater: FakeAppUpdater(supported: false),
        store: MemoryUpdatePreferencesStore(),
      );
      expect(unsupported.automaticCheckDue, isFalse);
    });

    test('a skipped version stays quiet only for automatic checks', () async {
      final service = AppUpdateService(
        updater: FakeAppUpdater(),
        store: MemoryUpdatePreferencesStore(),
      );
      await service.skipVersion(_update.version);

      expect(await service.check(automatic: true), isNull);
      expect((await service.check(automatic: false))?.version, '0.1.0-beta.2');
    });

    test('persists preferences as JSON and tolerates a broken file', () async {
      final directory = await Directory.systemTemp.createTemp('maicenta-upd');
      addTearDown(() => directory.delete(recursive: true));
      final file = File('${directory.path}/update_preferences.json');
      final store = FileUpdatePreferencesStore(file);

      expect((await store.load()).automaticChecks, isTrue);
      await store.save(
        UpdatePreferences(
          automaticChecks: false,
          skippedVersion: '0.2.0',
          lastCheckedAt: DateTime.utc(2026, 11, 4),
        ),
      );
      final loaded = await store.load();
      expect(loaded.automaticChecks, isFalse);
      expect(loaded.skippedVersion, '0.2.0');
      expect(loaded.lastCheckedAt, DateTime.utc(2026, 11, 4));

      await file.writeAsString('{not json');
      expect((await store.load()).automaticChecks, isTrue);
    });

    test('formats download sizes', () {
      expect(formatDownloadSize(0), '');
      expect(formatDownloadSize(5 * 1024 * 1024 + 512 * 1024), '5,5 MB');
      expect(formatDownloadSize(48 * 1024 * 1024), '48 MB');
    });
  });

  group('update dialog', () {
    testWidgets('offers a new version at startup and installs it', (
      tester,
    ) async {
      final updater = FakeAppUpdater();
      final service = AppUpdateService(
        updater: updater,
        store: MemoryUpdatePreferencesStore(),
      );
      await _pumpApp(tester, service);

      expect(updater.checks, 1);
      expect(find.byKey(const Key('app-update-dialog')), findsOneWidget);
      expect(find.textContaining('MAICENTA 0.1.0-beta.2'), findsOneWidget);
      expect(find.textContaining('Absturz beim Start'), findsOneWidget);
      expect(find.text('Download: 48 MB'), findsOneWidget);

      await tester.tap(find.byKey(const Key('app-update-install')));
      await tester.pump();
      updater.downloadController!.add(40);
      await tester.pump();
      expect(find.text('Wird heruntergeladen … 40 %'), findsOneWidget);

      await updater.downloadController!.close();
      await tester.pump();
      expect(find.text('Update bereit'), findsOneWidget);

      await tester.tap(find.byKey(const Key('app-update-restart')));
      await tester.pump();
      expect(updater.applies, 1);
    });

    testWidgets('shows a failed download and allows a retry', (tester) async {
      final updater = FakeAppUpdater();
      final service = AppUpdateService(
        updater: updater,
        store: MemoryUpdatePreferencesStore(),
      );
      await _pumpApp(tester, service);

      await tester.tap(find.byKey(const Key('app-update-install')));
      await tester.pump();
      updater.downloadController!.addError('checksum mismatch');
      await tester.pump();

      expect(find.text('Update fehlgeschlagen'), findsOneWidget);
      expect(find.text('checksum mismatch'), findsOneWidget);
      await tester.tap(find.byKey(const Key('app-update-retry')));
      await tester.pump();
      expect(find.byKey(const Key('app-update-progress')), findsOneWidget);
    });

    testWidgets('skipping a version stores it and closes the dialog', (
      tester,
    ) async {
      final store = MemoryUpdatePreferencesStore();
      final service = AppUpdateService(updater: FakeAppUpdater(), store: store);
      await _pumpApp(tester, service);

      await tester.tap(find.byKey(const Key('app-update-skip')));
      await tester.pumpAndSettle();

      expect(find.byKey(const Key('app-update-dialog')), findsNothing);
      expect(store.preferences.skippedVersion, '0.1.0-beta.2');
    });

    testWidgets('manual check reports an up-to-date installation', (
      tester,
    ) async {
      final updater = FakeAppUpdater(available: null);
      final service = AppUpdateService(
        updater: updater,
        store: MemoryUpdatePreferencesStore(),
      );
      await _pumpApp(tester, service);
      expect(updater.checks, 1);

      await _openOptions(tester);
      await tester.tap(find.byKey(const Key('options-check-updates')));
      await tester.pump();
      await tester.pump();

      expect(updater.checks, 2);
      expect(find.text('MAICENTA ist auf dem neuesten Stand.'), findsOneWidget);
    });

    testWidgets('options switch turns automatic checks off', (tester) async {
      final store = MemoryUpdatePreferencesStore(
        UpdatePreferences(lastCheckedAt: DateTime.now()),
      );
      final updater = FakeAppUpdater();
      final service = AppUpdateService(
        updater: updater,
        store: store,
        initial: store.preferences,
      );
      await _pumpApp(tester, service);
      expect(updater.checks, 0, reason: 'checked less than a day ago');

      await _openOptions(tester);
      await tester.tap(find.byKey(const Key('automatic-update-check-toggle')));
      await tester.pumpAndSettle();

      expect(store.preferences.automaticChecks, isFalse);
    });

    testWidgets('points to the download where the app cannot update itself', (
      tester,
    ) async {
      final service = AppUpdateService(
        updater: FakeAppUpdater(installsInApp: false),
        store: MemoryUpdatePreferencesStore(),
      );
      await _pumpApp(tester, service);

      expect(find.byKey(const Key('app-update-dialog')), findsOneWidget);
      expect(find.byKey(const Key('app-update-install')), findsNothing);
      expect(find.byKey(const Key('app-update-open-download')), findsOneWidget);
      expect(find.textContaining('in den Ordner „Programme“'), findsOneWidget);
    });

    testWidgets('unsupported installations never check', (tester) async {
      final updater = FakeAppUpdater(supported: false);
      final service = AppUpdateService(
        updater: updater,
        store: MemoryUpdatePreferencesStore(),
      );
      await _pumpApp(tester, service);
      await _openOptions(tester);

      expect(updater.checks, 0);
      expect(
        tester
            .widget<ListTile>(find.byKey(const Key('options-check-updates')))
            .enabled,
        isFalse,
      );
    });
  });
}
