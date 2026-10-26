--TEST--
羃等キー・revision・id・seal / unseal（SortitionSession と SortitionItemSession）
--EXTENSIONS--
sortition
--FILE--
<?php
$names = ['alice', 'bob', 'carol', 'dave', 'erin', 'frank', 'grace', 'heidi'];
$ids = fn(array $picks) => array_column($picks, 'id');

// 羃等キー: 再送は前回と同じ結果、別の k は衝突
$a = new SortitionSession(50);
$first = $a->draw(3, 'req-1');
$state = $a->save();
var_dump($a->draw(3, 'req-1') === $first, $a->save() === $state, $a->revision());
try { $a->draw(4, 'req-1'); } catch (ValueError $e) { echo $e->getCode(), ": ", $e->getMessage(), "\n"; }
try { $a->draw(1, ''); } catch (ValueError $e) { echo $e->getCode(), "\n"; }
try { $a->draw(1, "a\0b"); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
var_dump(strlen($a->id()), json_decode($state, true)['id'] === $a->id());
// 全部引いたあとでも、同じキーの再送は前回の結果を返す
$b = new SortitionSession(5);
$all = $b->draw(5, 'all');
var_dump($b->draw(5, 'all') === $all, $b->remaining());
// キーなしでは残りを超える要求はこれまで通りエラー
try { $b->draw(1); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
// 古い状態への再送は、同じ結果・同じ状態になる
$c = new SortitionSession(100);
$c->draw(4, 'a');
$stale = $c->save();
$want = $c->draw(6, 'b');
$old = SortitionSession::load($stale);
var_dump($old->draw(6, 'b') === $want, $old->save() === $c->save());

// seal / unseal
$key = str_repeat("\x07", 32);
$token = $a->seal($key, 9);
var_dump(str_starts_with($token, 'sortition1.'), sortition_seal_key_id($token));
var_dump(!str_contains($token, json_decode($a->save(), true)['rng']['key']));
$r = SortitionSession::unseal($token, $key);
var_dump($r->save() === $a->save(), $r->draw(2) === $a->draw(2));
$tampered = substr($token, 0, -1) . (substr($token, -1) === 'A' ? 'B' : 'A');
foreach ([
    fn() => SortitionSession::unseal($token, str_repeat("\x08", 32)),
    fn() => SortitionSession::unseal($tampered, $key),
    fn() => SortitionSession::unseal('junk', $key),
    fn() => sortition_seal_key_id('junk'),
] as $f) {
    try { $f(); } catch (ValueError $e) { echo $e->getCode(), "\n"; }
}
try { $a->seal('short'); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
try { $a->seal($key, 256); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }

// バイト列の候補のセッション
$lot = new SortitionItemSession($names, 11);
$got = $lot->draw(3, 'k1');
var_dump($lot->draw(3, 'k1') === $got, $lot->revision(), strlen($lot->id()));
$t = $lot->seal($key, 1);
$again = SortitionItemSession::unseal($t, $key, array_merge(array_slice($names, 2), array_slice($names, 0, 2)));
var_dump($ids($again->draw(5)) === $ids($lot->draw(5)));
try { SortitionItemSession::unseal($t, $key, array_slice($names, 1)); } catch (ValueError $e) { echo $e->getCode(), "\n"; }
try { SortitionSession::unseal($t, $key); } catch (ValueError $e) { echo $e->getCode(), "\n"; }
?>
--EXPECT--
bool(true)
bool(true)
int(1)
-9: SortitionSession::draw(): idempotency key was already used with a different request
-1
SortitionSession::draw(): key must not contain NUL bytes
int(32)
bool(true)
bool(true)
int(0)
SortitionSession::draw(): k exceeds the remaining count
bool(true)
bool(true)
bool(true)
int(9)
bool(true)
bool(true)
bool(true)
-8
-8
-8
-8
SortitionSession::seal(): key must be exactly 32 bytes
SortitionSession::seal(): keyId must be in 0..255
bool(true)
int(1)
int(32)
bool(true)
-8
-8
