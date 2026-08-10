import 'package:flutter_test/flutter_test.dart';
import 'package:maicenta/features/mail/mail_data.dart';

void main() {
  test('demo mail source remains deterministic', () {
    const source = DemoMailDataSource();

    expect(source.folders, hasLength(5));
    expect(source.folders.first.displayName, 'Posteingang');
    expect(source.messages, hasLength(5));
    expect(source.messages.where((message) => message.unread), hasLength(2));
    expect(source.messages.any((message) => message.hasAttachment), isTrue);
  });
}
