/// Fair, repeatable and verifiable random lotteries.
///
/// Use the candidate functions ([Lottery.drawItems], [Lottery.drawStages], [ItemSession]) when the
/// winners must not depend on the order of your list: each candidate gets an ID from its bytes, and
/// the draw works on the IDs in sorted order. [Lottery.draw], [Lottery.drawText] and
/// [Lottery.drawData] return positions in the list as given.
///
/// A seed is a 64-bit pattern held in a Dart `int` (it may be negative; use `seed.toUnsigned(64)` to
/// print it as the unsigned number every other binding shows). Without a seed the draw uses the
/// operating system's randomness. Everything throws [SortitionException] for invalid input.
library;

export 'src/lottery.dart';
export 'src/session.dart';
