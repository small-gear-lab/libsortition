import 'dart:convert';
import 'dart:ffi';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

import 'native.dart' as native;
import 'types.dart';

String? lastErrorMessage() {
  final p = native.lastError();
  return p == nullptr ? null : p.toDartString();
}

void check(int rc, {bool detailed = false}) {
  if (rc != 0) throw SortitionException(rc, detailed ? lastErrorMessage() : null);
}

/// A value for a C `uint32_t`.
int u32(int value, String name) {
  if (value < 0 || value > 0xFFFFFFFF) {
    throw SortitionException(SortitionException.invalid, '$name must be in 0..4294967295');
  }
  return value;
}

/// A pointer to a `uint64_t` seed, or NULL for no seed.
Pointer<Uint64> seedPtr(Arena arena, int? seed) {
  if (seed == null) return nullptr;
  final p = arena<Uint64>();
  p.value = seed;
  return p;
}

/// A copy of [bytes] in native memory (never zero-sized, so the pointer is never NULL).
Pointer<Uint8> bytesPtr(Arena arena, List<int> bytes) {
  final p = arena<Uint8>(bytes.isEmpty ? 1 : bytes.length);
  p.asTypedList(bytes.length).setAll(0, bytes);
  return p;
}

/// A NUL-terminated UTF-8 copy ([text] null means NULL). A NUL inside the text is refused.
Pointer<Utf8> cString(Arena arena, String? text, String name) {
  if (text == null) return nullptr;
  if (text.contains('\u0000')) {
    throw SortitionException(SortitionException.invalid, '$name must not contain NUL');
  }
  return text.toNativeUtf8(allocator: arena);
}

typedef Packed = ({Pointer<Uint8> data, Pointer<Size> lens, int count});

/// The candidates as one buffer and an array of lengths. A candidate is a [String] (its UTF-8 bytes)
/// or a list of bytes.
Packed putItems(Arena arena, List<Object> items) {
  final chunks = <List<int>>[
    for (final item in items)
      switch (item) {
        String s => utf8.encode(s),
        List<int> b => b,
        _ => throw ArgumentError.value(item, 'items', 'a candidate must be a String or a List<int>'),
      },
  ];
  final total = chunks.fold<int>(0, (a, c) => a + c.length);
  final data = arena<Uint8>(total == 0 ? 1 : total);
  final lens = arena<IntPtr>(chunks.isEmpty ? 1 : chunks.length);
  var offset = 0;
  for (var i = 0; i < chunks.length; i++) {
    data.asTypedList(total).setAll(offset, chunks[i]);
    lens[i] = chunks[i].length;
    offset += chunks[i].length;
  }
  return (data: data, lens: lens.cast<Size>(), count: chunks.length);
}

/// Reads a string that the core wrote through [out] (a `char **`), and frees it.
String takeString(Pointer<Pointer<Utf8>> out) {
  final p = out.value;
  try {
    return p.toDartString();
  } finally {
    native.free(p);
  }
}

String hex(Uint8List bytes) {
  final out = StringBuffer();
  for (final b in bytes) {
    out.write(b.toRadixString(16).padLeft(2, '0'));
  }
  return out.toString();
}

List<Pick> readPicks(Pointer<Uint32> indexes, Pointer<Uint8> ids, int k) => [
  for (var i = 0; i < k; i++)
    (index: indexes[i], id: hex(Uint8List.fromList(ids.asTypedList(32 * k).sublist(32 * i, 32 * i + 32)))),
];

/// A 32-byte secret key, checked.
void checkKey(List<int> key) {
  if (key.length != 32) {
    throw SortitionException(SortitionException.invalid, 'key must be exactly 32 bytes');
  }
}
