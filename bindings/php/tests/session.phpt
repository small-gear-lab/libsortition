--TEST--
SortitionSession: 分けて引いても一括と同じ並び、保存・復元、エラー
--EXTENSIONS--
sortition
--FILE--
<?php
$a = new SortitionSession(200, 9);
$got = $a->draw(20);
$state = $a->save();
$a->close();
$a->close(); // 二重 close は無害

$b = SortitionSession::load($state);
$got = array_merge($got, $b->draw(30));
var_dump($got === sortition_draw(200, 50, 9));
var_dump($b->remaining(), $b->drawnCount());
var_dump(json_decode($b->save(), true)['drawn'] === $got);
var_dump($b->draw(0));

// 乱数源でも使い切ると重複なしで全件が出る
$c = new SortitionSession(30);
$all = array_merge($c->draw(10), $c->draw(20));
sort($all);
var_dump($all === range(0, 29));

// 超過しても状態は変わらない
$before = $b->save();
try { $b->draw(151); } catch (ValueError $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump($b->save() === $before);

// 不正な状態
try { SortitionSession::load('{}'); } catch (ValueError $e) { echo $e->getCode(), ": ", $e->getMessage(), "\n"; }
try { new SortitionSession(-1); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }

// 閉じた後・クローン
$b->close();
try { $b->draw(1); } catch (Exception $e) { echo $e->getMessage(), "\n"; }
try { clone $c; } catch (Error $e) { echo $e->getMessage(), "\n"; }
?>
--EXPECT--
bool(true)
int(150)
int(50)
bool(true)
array(0) {
}
bool(true)
ValueError: SortitionSession::draw(): k exceeds the remaining count
bool(true)
-8: SortitionSession::load(): unsupported state version
SortitionSession::__construct(): require 0 <= n <= 4294967295
SortitionSession::draw(): session is closed
Trying to clone an uncloneable object of class SortitionSession
