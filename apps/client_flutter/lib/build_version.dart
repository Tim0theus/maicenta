/// Human-readable version of this build.
///
/// Kept as a Dart constant because the platform bundles cannot carry a
/// pre-release suffix: macOS reduces `0.1.0-beta.1` to `0.1.0.1` in
/// `CFBundleShortVersionString`. The value must equal the `version:` in
/// `pubspec.yaml` without the build number; a unit test and the release
/// workflow enforce that.
const String maicentaVersion = '0.1.0-beta.1';
