--TEST--
バイト列の候補: 並び順に依らない当選、ID・指紋、SortitionItemSession の保存・復元とエラー
--EXTENSIONS--
sortition
--FILE--
<?php
$names = ['alice', 'bob', 'carol', 'dave', 'erin', 'frank', 'grace', 'heidi'];
$shuffled = array_reverse(array_merge(array_slice($names, 3), array_slice($names, 0, 3)));
$ids = fn(array $picks) => array_column($picks, 'id');

var_dump(sortition_item_ids(['abc']) === ['3b39139043226007aae3c88c03f4a1867e3cea04126ee893f8b395f7eb97899b']);
$dup = sortition_item_ids(['x', 'x', '', '', "\xff\x00\xfe", 'x']);
var_dump(count(array_unique($dup)));
var_dump(sortition_item_fingerprint($names) === sortition_item_fingerprint($shuffled));

// 並び順に依らず、同じ集合とシードなら同じ候補が当選する
$same = true;
for ($seed = 0; $seed < 10; $seed++) {
    $a = sortition_draw_items($names, 5, $seed);
    $b = sortition_draw_items($shuffled, 5, $seed);
    $same = $same && $ids($a) === $ids($b)
        && array_map(fn($p) => $names[$p['index']], $a) === array_map(fn($p) => $shuffled[$p['index']], $b);
}
var_dump($same);
var_dump(sortition_draw_items([], 0));

// セッション: 分けて引いても一括と同じ。並びを変えて復元し、続きから引ける
$s = new SortitionItemSession($names, 11);
$got = $s->draw(3);
$state = $s->save();
$s->close();
$s->close();
$r = SortitionItemSession::load($state, array_merge(array_slice($names, 3), array_slice($names, 0, 3)));
$rest = $r->draw(5);
$got = array_merge($got, $rest);
var_dump($ids($got) === $ids(sortition_draw_items($names, 8, 11)));
$saved = json_decode($r->save(), true);
var_dump($saved['drawn'] === $ids($got), $saved['fingerprint'] === sortition_item_fingerprint($names));
var_dump($r->remaining(), $r->drawnCount());

// エラー
try { SortitionItemSession::load($state, array_slice($names, 1)); } catch (ValueError $e) { echo $e->getCode(), ": ", $e->getMessage(), "\n"; }
try { SortitionItemSession::load($state, array_merge(array_slice($names, 1), ['mallory'])); } catch (ValueError $e) { echo $e->getCode(), "\n"; }
try { SortitionSession::load($state); } catch (ValueError $e) { echo $e->getCode(), "\n"; }
try { sortition_draw_items($names, 9); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
try { sortition_item_ids([1]); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { $r->draw(1); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
$r->close();
try { $r->draw(0); } catch (Exception $e) { echo $e->getMessage(), "\n"; }
try { clone $r; } catch (Error $e) { echo $e->getMessage(), "\n"; }
?>
--EXPECT--
bool(true)
int(6)
bool(true)
bool(true)
array(0) {
}
bool(true)
bool(true)
bool(true)
int(0)
int(8)
-8: SortitionItemSession::load(): candidates do not match the saved state
-8
-8
sortition_draw_items(): k exceeds the number of candidates
sortition_item_ids(): candidates must be strings
SortitionItemSession::draw(): k exceeds the remaining count
SortitionItemSession::draw(): session is closed
Trying to clone an uncloneable object of class SortitionItemSession
