import 'package:flutter_test/flutter_test.dart';
import 'package:maicenta/features/compose/compose_window.dart';

void main() {
  test('converts supported Quill formatting to conservative email HTML', () {
    final html = quillDeltaToEmailHtml([
      {'insert': 'Hallo '},
      {
        'insert': 'Welt',
        'attributes': {
          'bold': true,
          'color': '#0f5fae',
          'font': 'Arial',
          'size': 'large',
        },
      },
      {
        'insert': '\n',
        'attributes': {'align': 'center'},
      },
      {'insert': 'Erster Punkt'},
      {
        'insert': '\n',
        'attributes': {'list': 'ordered'},
      },
    ]);

    expect(html, contains('<strong>'));
    expect(html, contains('color:#0f5fae'));
    expect(html, contains('font-family:Arial'));
    expect(html, contains('font-size:18px'));
    expect(html, contains('text-align:center'));
    expect(html, contains('<ol'));
    expect(html, contains('<li'));
  });

  test('escapes content and rejects unsafe composer links', () {
    final html = quillDeltaToEmailHtml([
      {
        'insert': '<script>alert(1)</script>',
        'attributes': {'link': 'javascript:alert(1)'},
      },
      {'insert': '\n'},
    ]);

    expect(html, contains('&lt;script&gt;'));
    expect(html, isNot(contains('<script>')));
    expect(html, isNot(contains('href="javascript:')));
  });
}
