import 'dart:convert';

import 'package:sortition/sortition.dart';
import 'package:test/test.dart';

const names = ['alice', 'bob', 'carol', 'dave', 'erin', 'frank', 'grace', 'heidi'];

Matcher failsWith(int code) => throwsA(isA<SortitionException>().having((e) => e.code, 'code', code));

List<String> ids(List<Pick> picks) => [for (final p in picks) p.id];

void main() {
  test('the quickstart prints the same winners as every other binding', () {
    const people = ['alice', 'bob', 'carol', 'dave'];
    final picks = Lottery.drawItems(people, 2, seed: Lottery.seedFromText('my seed'));
    expect([for (final p in picks) people[p.index]], ['dave', 'bob']);
    expect(Lottery.seedCommit('my seed'), '7efa9b064a86e6a99f9cff6c838e61c005ad9aac30e74efc50a0b9ec8270e7ad');
  });

  test('winners do not depend on the order of the list', () {
    final seed = Lottery.seedFromText('order');
    final reversed = names.reversed.toList();
    final a = Lottery.drawItems(names, 5, seed: seed);
    final b = Lottery.drawItems(reversed, 5, seed: seed);
    expect(ids(a), ids(b));
    expect([for (final p in a) names[p.index]], [for (final p in b) reversed[p.index]]);
    expect(Lottery.itemFingerprint(names), Lottery.itemFingerprint(reversed));
  });

  test('bytes and strings are the same candidates', () {
    expect(
      Lottery.itemIds([
        [0x61, 0x62],
        [0x63],
        <int>[],
      ]),
      Lottery.itemIds(['ab', 'c', '']),
    );
    expect(Lottery.itemIds(['x', 'x', 'x']).toSet().length, 3);
    expect(Lottery.itemIds(<String>[]), isEmpty);
    expect(() => Lottery.itemIds([42]), throwsArgumentError);
  });

  test('errors carry the code', () {
    expect(() => Lottery.draw(3, 4), failsWith(SortitionException.invalid));
    expect(() => Lottery.draw(3, -1), failsWith(SortitionException.invalid));
    expect(() => Lottery.drawItems(['a'], 2), failsWith(SortitionException.invalid));
    expect(() => Session.load('{}'), failsWith(SortitionException.state));
    expect(() => Lottery.sealKeyId('junk'), failsWith(SortitionException.state));
    expect(SortitionException(-1).toString(), 'SortitionException: error code -1');
  });

  test('text and data', () {
    expect(Lottery.drawText('  a \n\nb\nc\n', 3, seed: 1)..sort(), ['a', 'b', 'c']);
    expect(Lottery.drawText('', 0), isEmpty);
    final json = Lottery.drawData(DataFormat.json, '{"members": ["a", "b", "c"]}', 2, path: 'members', seed: 7);
    expect((jsonDecode(json) as List<dynamic>).length, 2);
    expect(
      () => Lottery.drawData(DataFormat.json, '{}', 1, path: 'nope'),
      throwsA(
        isA<SortitionException>().having((e) => e.code, 'code', -5).having((e) => e.detail, 'detail', isNotEmpty),
      ),
    );
  });

  test('split draws equal one draw and survive save and load', () {
    final a = Session(200, seed: 9);
    final got = a.draw(20);
    final b = Session.load(a.save());
    got.addAll(b.draw(30));
    expect(got, Lottery.draw(200, 50, seed: 9));
    expect([b.remaining, b.drawnCount], [150, 50]);
    expect(b.id, matches(RegExp(r'^[0-9a-f]{32}$')));
    expect(b.revision, 2); // one draw before the save, one after the restore
    expect(() => b.draw(151), failsWith(SortitionException.invalid));
    a.close();
    b.close();
  });

  test('idempotency keys replay and conflict', () {
    final s = Session(50);
    final first = s.draw(3, key: 'req-1');
    final state = s.save();
    expect(s.draw(3, key: 'req-1'), first);
    expect(s.save(), state);
    expect(() => s.draw(4, key: 'req-1'), failsWith(SortitionException.conflict));
    expect(() => s.draw(1, key: 'a\u0000b'), failsWith(SortitionException.invalid));
    expect(() => s.draw(1, key: ''), failsWith(SortitionException.invalid));
    s.close();
  });

  test('an old state sent again gives the same result', () {
    final a = Session(100)..draw(4, key: 'a');
    final stale = a.save();
    final want = a.draw(6, key: 'b');
    final old = Session.load(stale);
    expect(old.draw(6, key: 'b'), want);
    expect(a.save(), old.save());
    a.close();
    old.close();
  });

  test('sealing hides the state and detects changes', () {
    final key = List<int>.filled(32, 7);
    final a = Session(50)..draw(3, key: 'r');
    final token = a.seal(key, keyId: 9);
    expect(token, startsWith('sortition1.'));
    expect(Lottery.sealKeyId(token), 9);
    final b = Session.unseal(token, key);
    expect(b.save(), a.save());
    final tampered = token.substring(0, token.length - 1) + (token.endsWith('A') ? 'B' : 'A');
    expect(() => Session.unseal(tampered, key), failsWith(SortitionException.state));
    expect(() => Session.unseal(token, List<int>.filled(32, 0)), failsWith(SortitionException.state));
    expect(() => a.seal(List<int>.filled(31, 0)), failsWith(SortitionException.invalid));
    expect(() => a.seal(key, keyId: 256), failsWith(SortitionException.invalid));
    a.close();
    b.close();
  });

  test('item sessions resume with the same set in any order', () {
    final lot = ItemSession(names, seed: 11);
    final got = lot.draw(3, key: 'k1');
    expect(lot.draw(3, key: 'k1'), got);
    expect(lot.revision, 1);
    final state = lot.save();
    final reversed = names.reversed.toList();
    final again = ItemSession.load(state, reversed);
    final rest = again.draw(5);
    expect([...ids(got), ...ids(rest)], ids(Lottery.drawItems(names, 8, seed: 11)));
    final reversedIds = Lottery.itemIds(reversed);
    expect(rest.every((p) => reversedIds[p.index] == p.id), isTrue);
    expect(() => ItemSession.load(state, names.sublist(1)), failsWith(SortitionException.state));

    final key = List<int>.filled(32, 0);
    final token = lot.seal(key, keyId: 1);
    final unsealed = ItemSession.unseal(token, key, [...names.sublist(2), ...names.sublist(0, 2)]);
    expect(ids(lot.draw(5)), ids(unsealed.draw(5)));
    for (final s in [lot, again, unsealed]) {
      s.close();
    }
  });

  test('stages cut one draw in order and check their input', () {
    final seed = Lottery.seedFromText('stages');
    final got = Lottery.drawStages(names, [(name: '副賞', count: 3), (name: '大賞', count: 1)], seed: seed);
    expect([for (final r in got) r.name], ['副賞', '大賞']);
    expect([for (final r in got) ...r.winners], Lottery.drawItems(names, 4, seed: seed));
    expect([for (final p in got[0].winners) names[p.index]], ['carol', 'heidi', 'bob']);
    expect(Lottery.drawStages(names, []), isEmpty);
    expect(Lottery.drawStages(names, [(name: 'none', count: 0)])[0].winners, isEmpty);
    expect(() => Lottery.drawStages(names, [(name: 'too many', count: 99)]), failsWith(SortitionException.invalid));
  });

  test('a closed session cannot be used', () {
    final s = Session(5)..close();
    s.close();
    expect(() => s.draw(1), throwsStateError);
    expect(() => s.remaining, throwsStateError);
  });

  test('without a seed the results differ', () {
    expect(Lottery.draw(1000000, 8), isNot(Lottery.draw(1000000, 8)));
    expect(Session(10).id, isNot(Session(10).id));
  });

  test('the native version is reported', () {
    expect(Lottery.version, '0.1.1');
  });
}
