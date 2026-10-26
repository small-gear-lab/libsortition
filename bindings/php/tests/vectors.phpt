--TEST--
全言語共通のテストベクトル（test-vectors/vectors.json）と同じ値になること
--EXTENSIONS--
sortition
--FILE--
<?php
$v = json_decode(file_get_contents(__DIR__ . '/../../../test-vectors/vectors.json'), true);

/* 10 進文字列の u64 を、PHP の符号付き int のビット列として読む（2^63 以上は負の int になる） */
function u64(string $s): int {
    $hi = 0; $lo = 0;
    foreach (str_split($s) as $d) {
        $lo2 = $lo * 10 + (int) $d;
        $hi = ($hi * 10 + ($lo2 >> 32)) & 0xFFFFFFFF;
        $lo = $lo2 & 0xFFFFFFFF;
    }
    return ($hi << 32) | $lo;
}

$problems = [];
$check = function (string $label, $got, $want) use (&$problems) {
    if ($got !== $want) { $problems[] = "$label: got " . json_encode($got) . ", expected " . json_encode($want); }
};
$items = fn(array $set) => array_map('hex2bin', $set['items_hex']);
$sets = [];
foreach ($v['item_sets'] as $set) { $sets[$set['name']] = $items($set); }

foreach ($v['seed_from_text'] as $e) {
    $check("seed of {$e['text']}", sortition_seed_from_text($e['text']), u64($e['seed']));
    $check("digest of {$e['text']}", sortition_seed_commit($e['text']), $e['digest']);
}
foreach ($v['draw'] as $e) {
    $check("draw {$e['n']} {$e['k']} {$e['seed']}", sortition_draw($e['n'], $e['k'], u64($e['seed'])), $e['result']);
}
foreach ($v['item_sets'] as $set) {
    $check("ids of {$set['name']}", sortition_item_ids($items($set)), $set['ids']);
    $check("fingerprint of {$set['name']}", sortition_item_fingerprint($items($set)), $set['fingerprint']);
}
foreach ($v['draw_items'] as $e) {
    $check("draw_items {$e['set']} {$e['k']} {$e['seed']}", sortition_draw_items($sets[$e['set']], $e['k'], u64($e['seed'])), $e['winners']);
}
foreach ($v['stages'] as $e) {
    $plan = array_map(fn($st) => ['name' => $st['name'], 'count' => $st['count']], $e['stages']);
    $check("stages of {$e['set']}", sortition_draw_stages($sets[$e['set']], $plan, sortition_seed_from_text($e['seed_text'])), $e['stages']);
}
$c = $v['chacha20_session'];
$session = SortitionSession::load(json_encode($c['state']));
$check('chacha20 session', array_map(fn($k) => $session->draw($k), $c['draws']), $c['expected']);
$s = $v['seal'];
$check('seal key id', sortition_seal_key_id($s['token']), $s['key_id']);
$sealed = SortitionSession::unseal($s['token'], hex2bin($s['key']));
$check('sealed session', array_map(fn($k) => $sealed->draw($k), $s['draws']), $s['expected']);

echo $problems ? implode("\n", $problems) : 'all vectors match', "\n";
?>
--EXPECT--
all vectors match
