import 'dart:ffi';
import 'dart:io';

import 'package:ffi/ffi.dart';

/// An opaque native session.
final class Handle extends Opaque {}

DynamicLibrary _open() {
  final file = Platform.isWindows
      ? 'sortition.dll'
      : Platform.isMacOS
      ? 'libsortition.dylib'
      : 'libsortition.so';
  final env = Platform.environment['LIBSORTITION_PATH'];
  if (env != null && env.isNotEmpty) return DynamicLibrary.open(env);
  // target/release in the working directory or any directory above it
  for (var dir = Directory.current.absolute; ; dir = dir.parent) {
    final candidate = File(
      '${dir.path}${Platform.pathSeparator}target${Platform.pathSeparator}release${Platform.pathSeparator}$file',
    );
    if (candidate.existsSync()) return DynamicLibrary.open(candidate.path);
    if (dir.parent.path == dir.path) break;
  }
  return DynamicLibrary.open(file);
}

final DynamicLibrary lib = _open();

typedef CharOut = Pointer<Pointer<Utf8>>;

final version = lib.lookupFunction<Pointer<Utf8> Function(), Pointer<Utf8> Function()>('sortition_version');
final lastError = lib.lookupFunction<Pointer<Utf8> Function(), Pointer<Utf8> Function()>('sortition_last_error');
final free = lib.lookupFunction<Void Function(Pointer<Utf8>), void Function(Pointer<Utf8>)>('sortition_free');

final seedFromText = lib
    .lookupFunction<
      Int32 Function(Pointer<Uint8>, Size, Pointer<Uint64>, Pointer<Uint8>),
      int Function(Pointer<Uint8>, int, Pointer<Uint64>, Pointer<Uint8>)
    >('sortition_seed_from_text');

final draw = lib
    .lookupFunction<
      Int32 Function(Uint32, Uint32, Pointer<Uint64>, Pointer<Uint32>),
      int Function(int, int, Pointer<Uint64>, Pointer<Uint32>)
    >('sortition_draw');

final drawText = lib
    .lookupFunction<
      Int64 Function(Pointer<Uint8>, Size, Uint32, Pointer<Uint64>, Pointer<Uint8>, Size),
      int Function(Pointer<Uint8>, int, int, Pointer<Uint64>, Pointer<Uint8>, int)
    >('sortition_draw_text');

final drawData = lib
    .lookupFunction<
      Int32 Function(Int32, Pointer<Uint8>, Size, Pointer<Utf8>, Uint32, Pointer<Uint64>, CharOut),
      int Function(int, Pointer<Uint8>, int, Pointer<Utf8>, int, Pointer<Uint64>, CharOut)
    >('sortition_draw_data');

final sealKeyId = lib
    .lookupFunction<
      Int32 Function(Pointer<Uint8>, Size, Pointer<Uint8>),
      int Function(Pointer<Uint8>, int, Pointer<Uint8>)
    >('sortition_seal_key_id');

final itemIds = lib
    .lookupFunction<
      Int32 Function(Pointer<Uint8>, Pointer<Size>, Size, Pointer<Uint8>),
      int Function(Pointer<Uint8>, Pointer<Size>, int, Pointer<Uint8>)
    >('sortition_item_ids');

final itemFingerprint = lib
    .lookupFunction<
      Int32 Function(Pointer<Uint8>, Pointer<Size>, Size, Pointer<Uint8>),
      int Function(Pointer<Uint8>, Pointer<Size>, int, Pointer<Uint8>)
    >('sortition_item_fingerprint');

final itemsDraw = lib
    .lookupFunction<
      Int32 Function(Pointer<Uint8>, Pointer<Size>, Size, Uint32, Pointer<Uint64>, Pointer<Uint32>, Pointer<Uint8>),
      int Function(Pointer<Uint8>, Pointer<Size>, int, int, Pointer<Uint64>, Pointer<Uint32>, Pointer<Uint8>)
    >('sortition_items_draw');

/// The functions of one kind of session. The two kinds have the same shape, except for how they are
/// created and what a draw returns.
final class SessionFns {
  SessionFns(String prefix)
    : remaining = lib.lookupFunction<Uint32 Function(Pointer<Handle>), int Function(Pointer<Handle>)>(
        '${prefix}remaining',
      ),
      drawnCount = lib.lookupFunction<Uint32 Function(Pointer<Handle>), int Function(Pointer<Handle>)>(
        '${prefix}drawn_count',
      ),
      revision = lib.lookupFunction<Uint64 Function(Pointer<Handle>), int Function(Pointer<Handle>)>(
        '${prefix}revision',
      ),
      id = lib
          .lookupFunction<
            Int32 Function(Pointer<Handle>, Pointer<Uint8>),
            int Function(Pointer<Handle>, Pointer<Uint8>)
          >('${prefix}id'),
      save = lib.lookupFunction<Int32 Function(Pointer<Handle>, CharOut), int Function(Pointer<Handle>, CharOut)>(
        '${prefix}save',
      ),
      seal = lib
          .lookupFunction<
            Int32 Function(Pointer<Handle>, Pointer<Uint8>, Uint8, CharOut),
            int Function(Pointer<Handle>, Pointer<Uint8>, int, CharOut)
          >('${prefix}seal'),
      free = lib.lookupFunction<Void Function(Pointer<Handle>), void Function(Pointer<Handle>)>('${prefix}free'),
      freeAddress = lib.lookup<NativeFunction<Void Function(Pointer<Handle>)>>('${prefix}free');

  final int Function(Pointer<Handle>) remaining;
  final int Function(Pointer<Handle>) drawnCount;
  final int Function(Pointer<Handle>) revision;
  final int Function(Pointer<Handle>, Pointer<Uint8>) id;
  final int Function(Pointer<Handle>, CharOut) save;
  final int Function(Pointer<Handle>, Pointer<Uint8>, int, CharOut) seal;
  final void Function(Pointer<Handle>) free;
  final Pointer<NativeFunction<Void Function(Pointer<Handle>)>> freeAddress;
}

final sessionFns = SessionFns('sortition_session_');
final itemSessionFns = SessionFns('sortition_item_session_');

final sessionNew = lib
    .lookupFunction<
      Int32 Function(Uint32, Pointer<Uint64>, Pointer<Pointer<Handle>>),
      int Function(int, Pointer<Uint64>, Pointer<Pointer<Handle>>)
    >('sortition_session_new');
final sessionLoad = lib
    .lookupFunction<
      Int32 Function(Pointer<Uint8>, Size, Pointer<Pointer<Handle>>),
      int Function(Pointer<Uint8>, int, Pointer<Pointer<Handle>>)
    >('sortition_session_load');
final sessionUnseal = lib
    .lookupFunction<
      Int32 Function(Pointer<Uint8>, Size, Pointer<Uint8>, Pointer<Pointer<Handle>>),
      int Function(Pointer<Uint8>, int, Pointer<Uint8>, Pointer<Pointer<Handle>>)
    >('sortition_session_unseal');
final sessionDraw = lib
    .lookupFunction<
      Int32 Function(Pointer<Handle>, Uint32, Pointer<Utf8>, Pointer<Uint32>, Pointer<Int32>),
      int Function(Pointer<Handle>, int, Pointer<Utf8>, Pointer<Uint32>, Pointer<Int32>)
    >('sortition_session_draw_keyed');

final itemSessionNew = lib
    .lookupFunction<
      Int32 Function(Pointer<Uint8>, Pointer<Size>, Size, Pointer<Uint64>, Pointer<Pointer<Handle>>),
      int Function(Pointer<Uint8>, Pointer<Size>, int, Pointer<Uint64>, Pointer<Pointer<Handle>>)
    >('sortition_item_session_new');
final itemSessionLoad = lib
    .lookupFunction<
      Int32 Function(Pointer<Uint8>, Size, Pointer<Uint8>, Pointer<Size>, Size, Pointer<Pointer<Handle>>),
      int Function(Pointer<Uint8>, int, Pointer<Uint8>, Pointer<Size>, int, Pointer<Pointer<Handle>>)
    >('sortition_item_session_load');
final itemSessionUnseal = lib
    .lookupFunction<
      Int32 Function(
        Pointer<Uint8>,
        Size,
        Pointer<Uint8>,
        Pointer<Uint8>,
        Pointer<Size>,
        Size,
        Pointer<Pointer<Handle>>,
      ),
      int Function(Pointer<Uint8>, int, Pointer<Uint8>, Pointer<Uint8>, Pointer<Size>, int, Pointer<Pointer<Handle>>)
    >('sortition_item_session_unseal');
final itemSessionDraw = lib
    .lookupFunction<
      Int32 Function(Pointer<Handle>, Uint32, Pointer<Utf8>, Pointer<Uint32>, Pointer<Uint8>, Pointer<Int32>),
      int Function(Pointer<Handle>, int, Pointer<Utf8>, Pointer<Uint32>, Pointer<Uint8>, Pointer<Int32>)
    >('sortition_item_session_draw_keyed');
