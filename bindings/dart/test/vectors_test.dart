import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:sortition/sortition.dart';
import 'package:test/test.dart';

/// The shared test vectors (docs/SPEC.md; test-vectors/vectors.json): every binding must give the same values.
final Map<String, dynamic> vectors = () {
  for (var dir = Directory.current.absolute; ; dir = dir.parent) {
    final file = File('${dir.path}/test-vectors/vectors.json');
    if (file.existsSync()) return jsonDecode(file.readAsStringSync()) as Map<String, dynamic>;
    if (dir.parent.path == dir.path) throw StateError('test-vectors/vectors.json was not found');
  }
}();

List<Map<String, dynamic>> list(String key) => (vectors[key] as List<dynamic>).cast<Map<String, dynamic>>();

/// A seed in the vectors is an unsigned decimal string; Dart holds the 64-bit pattern.
int seedOf(Object? s) => BigInt.parse(s! as String).toSigned(64).toInt();

List<int> ints(Object? v) => (v! as List<dynamic>).cast<int>();

Uint8List fromHex(String h) =>
    Uint8List.fromList([for (var i = 0; i < h.length; i += 2) int.parse(h.substring(i, i + 2), radix: 16)]);

List<Uint8List> itemsOf(Map<String, dynamic> set) => [
  for (final h in (set['items_hex'] as List<dynamic>).cast<String>()) fromHex(h),
];

List<Pick> picksOf(Object? v) => [
  for (final w in (v! as List<dynamic>).cast<Map<String, dynamic>>()) (index: w['index'] as int, id: w['id'] as String),
];

void drawAll(Session session, Map<String, dynamic> v) {
  final draws = ints(v['draws']);
  final expected = (v['expected'] as List<dynamic>).map(ints).toList();
  for (var i = 0; i < draws.length; i++) {
    expect(session.draw(draws[i]), expected[i], reason: 'draw $i');
  }
}

void main() {
  test('seed from text', () {
    for (final v in list('seed_from_text')) {
      expect(Lottery.seedFromText(v['text'] as String), seedOf(v['seed']));
      expect(Lottery.seedCommit(v['text'] as String), v['digest']);
    }
  });

  test('draw', () {
    for (final v in list('draw')) {
      expect(Lottery.draw(v['n'] as int, v['k'] as int, seed: seedOf(v['seed'])), ints(v['result']));
    }
  });

  test('item IDs and fingerprints', () {
    for (final v in list('item_sets')) {
      expect(Lottery.itemIds(itemsOf(v)), (v['ids'] as List<dynamic>).cast<String>());
      expect(Lottery.itemFingerprint(itemsOf(v)), v['fingerprint']);
    }
  });

  test('draw items', () {
    final sets = {for (final s in list('item_sets')) s['name'] as String: itemsOf(s)};
    for (final v in list('draw_items')) {
      expect(Lottery.drawItems(sets[v['set']]!, v['k'] as int, seed: seedOf(v['seed'])), picksOf(v['winners']));
    }
  });

  test('stages', () {
    final sets = {for (final s in list('item_sets')) s['name'] as String: itemsOf(s)};
    for (final v in list('stages')) {
      final expected = (v['stages'] as List<dynamic>).cast<Map<String, dynamic>>();
      final plan = [for (final s in expected) (name: s['name'] as String, count: s['count'] as int)];
      final got = Lottery.drawStages(sets[v['set']]!, plan, seed: Lottery.seedFromText(v['seed_text'] as String));
      expect(got.length, expected.length);
      for (var i = 0; i < got.length; i++) {
        expect(got[i].name, expected[i]['name']);
        expect(got[i].winners, picksOf(expected[i]['winners']));
      }
    }
  });

  test('a session restored from a ChaCha20 state', () {
    final v = vectors['chacha20_session'] as Map<String, dynamic>;
    final session = Session.load(jsonEncode(v['state']));
    addTearDown(session.close);
    drawAll(session, v);
  });

  test('a sealed token', () {
    final v = vectors['seal'] as Map<String, dynamic>;
    final token = v['token'] as String;
    expect(Lottery.sealKeyId(token), v['key_id']);
    final session = Session.unseal(token, fromHex(v['key'] as String));
    addTearDown(session.close);
    drawAll(session, v);
  });
}
