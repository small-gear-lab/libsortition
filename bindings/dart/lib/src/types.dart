/// A failure reported by the core.
class SortitionException implements Exception {
  /// An argument is out of range (k larger than the number of candidates, an empty key, ...).
  static const invalid = -1;

  /// An invalid saved state or sealed token (or a different set of candidates).
  static const state = -8;

  /// An idempotency key was used with a different request.
  static const conflict = -9;

  SortitionException(this.code, [this.detail]);

  /// The negative error code of the core (see docs/SPEC.md).
  final int code;

  /// Details, when the core has them.
  final String? detail;

  @override
  String toString() => detail != null && detail!.isNotEmpty
      ? 'SortitionException: $detail (code $code)'
      : 'SortitionException: error code $code';
}

/// The format of structured data for `Lottery.drawData`.
enum DataFormat {
  json(0),

  /// YAML (aliases, custom tags and multiple documents are rejected).
  yaml(1),

  /// TOML (it needs a path, because its root is a table).
  toml(2);

  const DataFormat(this.value);
  final int value;
}

/// A winner: its position in the list that was passed, and its ID (SHA-256, 64 lowercase hex characters).
typedef Pick = ({int index, String id});

/// One prize stage of a plan.
typedef Stage = ({String name, int count});

/// The winners of one prize stage.
typedef StageResult = ({String name, int count, List<Pick> winners});
