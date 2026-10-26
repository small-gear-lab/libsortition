import 'dart:convert';
import 'dart:ffi';

import 'package:ffi/ffi.dart';

import 'native.dart' as native;
import 'support.dart';
import 'types.dart';

final _sessionFinalizer = NativeFinalizer(native.sessionFns.freeAddress.cast());
final _itemSessionFinalizer = NativeFinalizer(native.itemSessionFns.freeAddress.cast());

/// What both kinds of session share. Call [close] when done; a forgotten session is freed when it is
/// garbage collected. A session is not thread-safe, and not shared between isolates.
abstract class SessionBase implements Finalizable {
  SessionBase._(this._fns, this._finalizer, this._handle) {
    _finalizer.attach(this, _handle.cast<Void>(), detach: this);
  }

  final native.SessionFns _fns;
  final NativeFinalizer _finalizer;
  Pointer<native.Handle> _handle;
  bool _closed = false;

  Pointer<native.Handle> get _live {
    if (_closed) throw StateError('session is closed');
    return _handle;
  }

  /// Frees the session. It is safe to call it more than once.
  void close() {
    if (_closed) return;
    _closed = true;
    _finalizer.detach(this);
    _fns.free(_handle);
    _handle = nullptr;
  }

  /// How many are not drawn yet.
  int get remaining => _fns.remaining(_live);

  /// How many were drawn.
  int get drawnCount => _fns.drawnCount(_live);

  /// The number of draws that changed the state. A store can update a record only when the revision is the
  /// one it read, so that two updates made from the same state do not overwrite each other.
  int get revision => _fns.revision(_live);

  /// The session's identifier (32 lowercase hex characters), taken from the operating system when it was created.
  String get id => using((arena) {
    final out = arena<Uint8>(33);
    check(_fns.id(_live, out));
    return utf8.decode(out.asTypedList(32));
  });

  /// The state as JSON. It contains the internal state of the random generator: whoever has it can predict
  /// the next winners, so do not give it to a party you do not trust. [seal] it instead.
  String save() => using((arena) {
    final out = arena<Pointer<Utf8>>();
    check(_fns.save(_live, out));
    return takeString(out);
  });

  /// The state as a token that only the holder of [key] can open, and that cannot be changed without notice
  /// (XChaCha20-Poly1305). [key] is a secret of exactly 32 bytes (make it with a cryptographic random
  /// generator); [keyId] (0 to 255) is a label for rotating keys, see `Lottery.sealKeyId`.
  String seal(List<int> key, {int keyId = 0}) => using((arena) {
    checkKey(key);
    if (keyId < 0 || keyId > 255) {
      throw SortitionException(SortitionException.invalid, 'keyId must be in 0..255');
    }
    final out = arena<Pointer<Utf8>>();
    check(_fns.seal(_live, bytesPtr(arena, key), keyId, out));
    return takeString(out);
  });
}

Pointer<native.Handle> _handleOf(Pointer<Pointer<native.Handle>> out) => out.value;

/// A session draws without replacement from 0..n-1 and continues where it stopped. Drawing in several
/// steps, with a save and a restore in between, gives the same order as one draw.
final class Session extends SessionBase {
  Session._(Pointer<native.Handle> handle) : super._(native.sessionFns, _sessionFinalizer, handle);

  /// Creates a session over 0..n-1. Without a [seed] it runs a cryptographic stream seeded from the
  /// operating system.
  factory Session(int n, {int? seed}) => using((arena) {
    u32(n, 'n');
    final out = arena<Pointer<native.Handle>>();
    check(native.sessionNew(n, seedPtr(arena, seed), out));
    return Session._(_handleOf(out));
  });

  /// Restores a session from [save].
  factory Session.load(String state) => using((arena) {
    final data = utf8.encode(state);
    final out = arena<Pointer<native.Handle>>();
    check(native.sessionLoad(bytesPtr(arena, data), data.length, out), detailed: true);
    return Session._(_handleOf(out));
  });

  /// Restores a session from a token made by [seal]. A malformed token, a wrong key and a changed token all
  /// give [SortitionException.state].
  factory Session.unseal(String token, List<int> key) => using((arena) {
    checkKey(key);
    final data = utf8.encode(token);
    final out = arena<Pointer<native.Handle>>();
    check(native.sessionUnseal(bytesPtr(arena, data), data.length, bytesPtr(arena, key), out), detailed: true);
    return Session._(_handleOf(out));
  });

  /// Draws the next [k] numbers. More than [remaining] throws [SortitionException.invalid] and leaves the
  /// state unchanged. With an idempotency [key] (1 to 128 bytes), the same key and the same [k] again does not
  /// change the state and returns the earlier result; the same key with another [k] throws
  /// [SortitionException.conflict].
  List<int> draw(int k, {String? key}) => using((arena) {
    u32(k, 'k');
    final out = arena<Uint32>(k == 0 ? 1 : k);
    check(native.sessionDraw(_live, k, cString(arena, key, 'key'), out, nullptr));
    return List<int>.of(out.asTypedList(k));
  });
}

/// A session over a fixed set of candidates. Winners are [Pick]s; after a restore, `index` is a position in
/// the list that was passed again.
final class ItemSession extends SessionBase {
  ItemSession._(Pointer<native.Handle> handle) : super._(native.itemSessionFns, _itemSessionFinalizer, handle);

  /// Creates a session over a fixed set of candidates.
  factory ItemSession(List<Object> items, {int? seed}) => using((arena) {
    final packed = putItems(arena, items);
    final out = arena<Pointer<native.Handle>>();
    check(native.itemSessionNew(packed.data, packed.lens, packed.count, seedPtr(arena, seed), out));
    return ItemSession._(_handleOf(out));
  });

  /// Restores a session from [save] and the same set of candidates (the order may differ; another set throws
  /// [SortitionException.state]).
  factory ItemSession.load(String state, List<Object> items) => using((arena) {
    final data = utf8.encode(state);
    final packed = putItems(arena, items);
    final out = arena<Pointer<native.Handle>>();
    check(
      native.itemSessionLoad(bytesPtr(arena, data), data.length, packed.data, packed.lens, packed.count, out),
      detailed: true,
    );
    return ItemSession._(_handleOf(out));
  });

  /// Restores a session from a token made by [seal] and the same set of candidates.
  factory ItemSession.unseal(String token, List<int> key, List<Object> items) => using((arena) {
    checkKey(key);
    final data = utf8.encode(token);
    final packed = putItems(arena, items);
    final out = arena<Pointer<native.Handle>>();
    check(
      native.itemSessionUnseal(
        bytesPtr(arena, data),
        data.length,
        bytesPtr(arena, key),
        packed.data,
        packed.lens,
        packed.count,
        out,
      ),
      detailed: true,
    );
    return ItemSession._(_handleOf(out));
  });

  /// Draws the next [k] candidates, with an optional idempotency [key] as in [Session.draw].
  List<Pick> draw(int k, {String? key}) => using((arena) {
    u32(k, 'k');
    final indexes = arena<Uint32>(k == 0 ? 1 : k);
    final ids = arena<Uint8>(32 * k + 1);
    check(native.itemSessionDraw(_live, k, cString(arena, key, 'key'), indexes, ids, nullptr));
    return readPicks(indexes, ids, k);
  });
}
