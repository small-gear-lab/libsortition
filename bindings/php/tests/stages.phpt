--TEST--
seed の文字列と、賞ごとの抽選（sortition_seed_from_text / sortition_seed_commit / sortition_draw_stages）
--EXTENSIONS--
sortition
--FILE--
<?php
// 全バインディング共通の規則: SHA-256 の先頭 8 バイトをリトルエンディアンで読んだ値
var_dump(sortition_seed_from_text('sortition'));
var_dump(sortition_seed_commit('sortition'));
var_dump(sortition_seed_from_text('') === 0x141cfc9842c4b0e3);
var_dump(sortition_seed_from_text('a') !== sortition_seed_from_text('b'));
// 2^63 以上の seed は負の int になるが、そのまま seed に渡せる
$big = null;
foreach (range(0, 40) as $i) {
    if (sortition_seed_from_text("probe-$i") < 0) { $big = sortition_seed_from_text("probe-$i"); break; }
}
var_dump($big < 0, count(sortition_draw_items(['a', 'b', 'c'], 2, $big)));

$names = ['alice', 'bob', 'carol', 'dave', 'erin', 'frank', 'grace', 'heidi'];
$plan = [['name' => '副賞', 'count' => 3], ['name' => '大賞', 'count' => 1]];
$seed = sortition_seed_from_text('stages');
$got = sortition_draw_stages($names, $plan, $seed);
$whole = sortition_draw_items($names, 4, $seed);
var_dump(array_map(fn($r) => [$r['name'], $r['count'], count($r['winners'])], $got));
var_dump(array_merge(...array_column($got, 'winners')) === $whole);
var_dump(count(array_unique(array_column(array_merge(...array_column($got, 'winners')), 'id'))));

// 並び順に依らない。組（[名前, 人数]）でもよい
$ids = fn(array $r) => array_map(fn($s) => array_column($s['winners'], 'id'), $r);
var_dump($ids(sortition_draw_stages(array_reverse($names), $plan, $seed)) === $ids($got));
var_dump(sortition_draw_stages($names, [['副賞', 3], ['大賞', 1]], $seed) === $got);
var_dump(sortition_draw_stages($names, []));
var_dump(sortition_draw_stages($names, [['name' => '空', 'count' => 0]])[0]['winners']);

// エラー
foreach ([
    fn() => sortition_draw_stages($names, [['name' => '多すぎる', 'count' => 99]]),
    fn() => sortition_draw_stages($names, [['name' => 'a', 'count' => -1]]),
    fn() => sortition_draw_stages($names, [['name' => 'a', 'count' => 1.5]]),
    fn() => sortition_draw_stages($names, [['name' => 1, 'count' => 1]]),
    fn() => sortition_draw_stages($names, ['not an array']),
    fn() => sortition_draw_stages([1], [['name' => 'a', 'count' => 1]]),
] as $f) {
    try { $f(); echo "no error\n"; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
?>
--EXPECT--
int(131367258856992384)
string(64) "80ae496fd3b5d201bb96339356ff252e2b1cdc214ad567f6ebe8de4537e6b31c"
bool(true)
bool(true)
bool(true)
int(2)
array(2) {
  [0]=>
  array(3) {
    [0]=>
    string(6) "副賞"
    [1]=>
    int(3)
    [2]=>
    int(3)
  }
  [1]=>
  array(3) {
    [0]=>
    string(6) "大賞"
    [1]=>
    int(1)
    [2]=>
    int(1)
  }
}
bool(true)
int(4)
bool(true)
bool(true)
array(0) {
}
array(0) {
}
ValueError: sortition_draw_stages(): the stages need 99 winners but there are only 8 candidates
ValueError: sortition_draw_stages(): stages[0] count must not be negative
TypeError: sortition_draw_stages(): stages[0] count must be an int
TypeError: sortition_draw_stages(): stages[0] name must be a string
TypeError: sortition_draw_stages(): stages[0] must be an array
TypeError: sortition_draw_stages(): candidates must be strings
