import 'dart:convert';
import 'dart:ffi';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

import 'native.dart' as native;
import 'support.dart';
import 'types.dart';

export 'types.dart';

/// Fair, repeatable and verifiable random lotteries (see the library documentation).
///
/// A candidate (an element of `items`) is a [String] or a list of bytes.
abstract final class Lottery {
  /// The version of the native library.
  static String get version => native.version().toDartString();

  static ({int seed, Uint8List digest}) _seedInfo(List<int> text) => using((arena) {
    final seed = arena<Uint64>();
    final digest = arena<Uint8>(32);
    check(native.seedFromText(bytesPtr(arena, text), text.length, seed, digest));
    return (seed: seed.value, digest: Uint8List.fromList(digest.asTypedList(32)));
  });

  /// The seed of a seed string: the first 8 bytes of its SHA-256, read as a little-endian number.
  static int seedFromText(String text) => _seedInfo(utf8.encode(text)).seed;

  /// The SHA-256 of a seed string (64 lowercase hex characters): the commitment to publish before the draw.
  static String seedCommit(String text) => hex(_seedInfo(utf8.encode(text)).digest);

  /// Draws [k] distinct numbers from 0..n-1, in the order drawn.
  static List<int> draw(int n, int k, {int? seed}) => using((arena) {
    u32(n, 'n');
    u32(k, 'k');
    final out = arena<Uint32>(k == 0 ? 1 : k);
    check(native.draw(n, k, seedPtr(arena, seed), out));
    return List<int>.of(out.asTypedList(k));
  });

  /// Draws [k] lines from [text] (one candidate per line; lines are trimmed and empty lines are not candidates).
  static List<String> drawText(String text, int k, {int? seed}) => using((arena) {
    u32(k, 'k');
    final data = utf8.encode(text);
    final capacity = data.length + 1;
    final out = arena<Uint8>(capacity);
    final n = native.drawText(bytesPtr(arena, data), data.length, k, seedPtr(arena, seed), out, capacity);
    if (n < 0) throw SortitionException(n);
    if (n == 0) return <String>[];
    return utf8.decode(out.asTypedList(n)).split('\n');
  });

  /// Draws [k] elements of an array inside JSON, YAML or TOML data and returns them as a JSON array string.
  /// [path] says where the array is (`"a.b.0"`); empty means the root.
  static String drawData(DataFormat format, String text, int k, {String path = '', int? seed}) => using((arena) {
    u32(k, 'k');
    final data = utf8.encode(text);
    final out = arena<Pointer<Utf8>>();
    check(
      native.drawData(
        format.value,
        bytesPtr(arena, data),
        data.length,
        cString(arena, path, 'path'),
        k,
        seedPtr(arena, seed),
        out,
      ),
      detailed: true,
    );
    return takeString(out);
  });

  /// Reads the key ID of a sealed token (to choose the key before opening it).
  static int sealKeyId(String token) => using((arena) {
    final data = utf8.encode(token);
    final out = arena<Uint8>();
    check(native.sealKeyId(bytesPtr(arena, data), data.length, out), detailed: true);
    return out.value;
  });

  /// The ID of each candidate (64 lowercase hex characters), in the order of [items].
  static List<String> itemIds(List<Object> items) => using((arena) {
    final packed = putItems(arena, items);
    final out = arena<Uint8>(32 * packed.count + 1);
    check(native.itemIds(packed.data, packed.lens, packed.count, out));
    final bytes = out.asTypedList(32 * packed.count);
    return [for (var i = 0; i < packed.count; i++) hex(Uint8List.fromList(bytes.sublist(32 * i, 32 * i + 32)))];
  });

  /// The fingerprint of the set of candidates (64 lowercase hex characters). It does not depend on the order
  /// of [items]: publish it with the commitment, before the draw.
  static String itemFingerprint(List<Object> items) => using((arena) {
    final packed = putItems(arena, items);
    final out = arena<Uint8>(32);
    check(native.itemFingerprint(packed.data, packed.lens, packed.count, out));
    return hex(Uint8List.fromList(out.asTypedList(32)));
  });

  /// Draws [k] candidates without replacement. The same set of candidates and the same seed give the same
  /// winners whatever the order of [items].
  static List<Pick> drawItems(List<Object> items, int k, {int? seed}) => using((arena) {
    u32(k, 'k');
    final packed = putItems(arena, items);
    final indexes = arena<Uint32>(k == 0 ? 1 : k);
    final ids = arena<Uint8>(32 * k + 1);
    check(native.itemsDraw(packed.data, packed.lens, packed.count, k, seedPtr(arena, seed), indexes, ids));
    return readPicks(indexes, ids, k);
  });

  /// Draws prize stages in order without giving anyone two prizes. It draws the total once and cuts it up in
  /// the order of [stages], which has the same probabilities as drawing stage by stage.
  static List<StageResult> drawStages(List<Object> items, List<Stage> stages, {int? seed}) {
    var total = 0;
    for (final s in stages) {
      if (s.count < 0) {
        throw SortitionException(SortitionException.invalid, 'stage count must not be negative');
      }
      total += s.count;
    }
    final all = drawItems(items, total, seed: seed);
    var offset = 0;
    return [
      for (final s in stages)
        (name: s.name, count: s.count, winners: List<Pick>.unmodifiable(all.sublist(offset, offset += s.count))),
    ];
  }
}
