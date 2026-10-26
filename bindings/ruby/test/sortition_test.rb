# frozen_string_literal: true

# cargo build --release && ruby -Ibindings/ruby/lib bindings/ruby/test/sortition_test.rb
require "minitest/autorun"
require "securerandom"
require "sortition"

# 全言語共通のテストベクトル（docs/SPEC.md、core のテストが作る）
VECTORS = JSON.parse(File.read(File.expand_path("../../../test-vectors/vectors.json", __dir__)))

class SortitionTest < Minitest::Test
  S = Sortition
  NAMES = %w[alice bob carol dave erin frank grace heidi].freeze

  def vector_items(v) = v["items_hex"].map { |h| [h].pack("H*") }

  def vector_sets = VECTORS["item_sets"].to_h { |v| [v["name"], vector_items(v)] }

  def winners(picks) = picks.map { |p| { "index" => p.index, "id" => p.id } }

  def test_shared_vectors_seed_and_draw
    VECTORS["seed_from_text"].each do |v|
      assert_equal v["seed"].to_i, S.seed_from_text(v["text"]), "seed of #{v['text'].inspect}"
      assert_equal v["digest"], S.seed_commit(v["text"])
    end
    VECTORS["draw"].each do |v|
      assert_equal v["result"], S.draw(v["n"], v["k"], seed: v["seed"].to_i), "draw #{v.values_at('n', 'k', 'seed')}"
    end
  end

  def test_shared_vectors_candidates
    sets = vector_sets
    VECTORS["item_sets"].each do |v|
      assert_equal v["ids"], S.item_ids(vector_items(v)), "ids of #{v['name']}"
      assert_equal v["fingerprint"], S.item_fingerprint(vector_items(v))
    end
    VECTORS["draw_items"].each do |v|
      got = winners(S.draw_items(sets[v["set"]], v["k"], seed: v["seed"].to_i))
      assert_equal v["winners"], got, "draw_items #{v.values_at('set', 'k', 'seed')}"
    end
    VECTORS["stages"].each do |v|
      plan = v["stages"].map { |st| [st["name"], st["count"]] }
      got = S.draw_stages(sets[v["set"]], plan, seed: S.seed_from_text(v["seed_text"])).map do |r|
        { "name" => r.name, "count" => r.count, "winners" => winners(r.winners) }
      end
      assert_equal v["stages"], got, "stages of #{v['set']}"
    end
  end

  def test_shared_vectors_sessions
    v = VECTORS["chacha20_session"]
    session = S::Session.load(JSON.generate(v["state"]))
    assert_equal v["expected"], v["draws"].map { |k| session.draw(k) }

    v = VECTORS["seal"]
    assert_equal v["key_id"], S.seal_key_id(v["token"])
    session = S::Session.unseal(v["token"], [v["key"]].pack("H*"))
    assert_equal v["expected"], v["draws"].map { |k| session.draw(k) }
  end

  def test_basics
    assert_equal [7, 2, 6], S.draw(10, 3, seed: 1)
    assert_equal (0...100).to_a, S.draw(100, 100).sort
    assert_raises(S::Error) { S.draw(3, 4) }
    assert_equal %w[a b c], S.draw_text("a\n b \n\nc", 3).sort
  end

  def test_session_resume_and_errors
    a = S::Session.new(200, seed: 9)
    got = a.draw(20)
    b = S::Session.load(a.save)
    got += b.draw(30)
    assert_equal S.draw(200, 50, seed: 9), got
    assert_equal [150, 50], [b.remaining, b.drawn_count]
    assert_equal got, JSON.parse(b.save)["drawn"]
    e = assert_raises(S::Error) { b.draw(151) }
    assert_equal(-1, e.code)
    assert_equal(-8, assert_raises(S::Error) { S::Session.load("{}") }.code)
    b.close
    b.close
    assert_raises(IOError) { b.draw(1) }
  end

  def test_unseeded_resume_is_deterministic
    a = S::Session.new(100)
    a.draw(10)
    saved = a.save
    assert_equal a.draw(5), S::Session.load(saved).draw(5)
  end

  def test_idempotency_key
    a = S::Session.new(50)
    first = a.draw(3, key: "req-1")
    state = a.save
    assert_equal first, a.draw(3, key: "req-1")
    assert_equal [state, 1], [a.save, a.revision]
    assert_equal(-9, assert_raises(S::Error) { a.draw(4, key: "req-1") }.code)
    assert_equal(-1, assert_raises(S::Error) { a.draw(1, key: "") }.code)
    assert_raises(TypeError) { a.draw(1, key: 5) }
    assert_equal [32, JSON.parse(state)["id"]], [a.id.size, a.id]
    all = S::Session.new(5)
    five = all.draw(5, key: "all")
    assert_equal five, all.draw(5, key: "all")
  end

  def test_stale_state_resend_gives_same_result
    a = S::Session.new(100)
    a.draw(4, key: "a")
    stale = a.save
    want = a.draw(6, key: "b")
    old = S::Session.load(stale)
    assert_equal want, old.draw(6, key: "b")
    assert_equal a.save, old.save
  end

  def test_seal
    key = SecureRandom.random_bytes(32)
    a = S::Session.new(50)
    a.draw(3, key: "r")
    token = a.seal(key, key_id: 7)
    assert token.start_with?("sortition1.")
    assert_equal 7, S.seal_key_id(token)
    refute_includes token, JSON.parse(a.save)["rng"]["key"]
    b = S::Session.unseal(token, key)
    assert_equal a.save, b.save
    assert_equal a.draw(2), b.draw(2)
    tampered = token[0..-2] + (token.end_with?("A") ? "B" : "A")
    [-> { S::Session.unseal(token, SecureRandom.random_bytes(32)) },
     -> { S::Session.unseal(tampered, key) },
     -> { S::Session.unseal("junk", key) },
     -> { S.seal_key_id("junk") }].each do |f|
      assert_equal(-8, assert_raises(S::Error, &f).code)
    end
    assert_raises(ArgumentError) { a.seal("short") }
    assert_raises(ArgumentError) { a.seal(nil) }
  end

  def test_seed_follows_the_core_rule
    # printf sortition | sha256sum の先頭 8 バイトをリトルエンディアンで読んだ値（全バインディング共通）
    assert_equal 131_367_258_856_992_384, S.seed_from_text("sortition")
    assert_equal "80ae496fd3b5d201bb96339356ff252e2b1cdc214ad567f6ebe8de4537e6b31c", S.seed_commit("sortition")
    assert_equal 0x141cfc9842c4b0e3, S.seed_from_text("")
    assert_equal S.seed_from_text("日本語"), S.seed_from_text("日本語".b)
    refute_equal S.seed_from_text("a"), S.seed_from_text("b")
    assert_equal 3, S.draw(10, 3, seed: S.seed_from_text("abc")).size
  end

  def test_stages_cut_one_draw_in_order
    plan = [["副賞", 3], ["大賞", 1]]
    seed = S.seed_from_text("stages")
    got = S.draw_stages(NAMES, plan, seed: seed)
    assert_equal [["副賞", 3, 3], ["大賞", 1, 1]], got.map { |r| [r.name, r.count, r.winners.size] }
    assert_equal S.draw_items(NAMES, 4, seed: seed), got.flat_map(&:winners)
    assert_equal 4, got.flat_map(&:winners).map(&:id).uniq.size
    # 並び順に依らない、Hash でも組でもよい
    shuffled = S.draw_stages(NAMES.reverse, [{ name: "副賞", count: 3 }, { name: "大賞", count: 1 }], seed: seed)
    assert_equal got.map { |r| r.winners.map(&:id) }, shuffled.map { |r| r.winners.map(&:id) }
    assert_equal [], S.draw_stages(NAMES, [])
    assert_equal [], S.draw_stages(NAMES, [["空", 0]]).first.winners
  end

  def test_stages_errors
    assert_raises(S::Error) { S.draw_stages(NAMES, [["多すぎる", 99]]) }
    assert_raises(ArgumentError) { S.draw_stages(NAMES, [["a", -1]]) }
    assert_raises(ArgumentError) { S.draw_stages(NAMES, [["a", 1.5]]) }
    assert_raises(TypeError) { S.draw_stages(NAMES, [[1, 1]]) }
  end

  def test_items
    assert_equal ["3b39139043226007aae3c88c03f4a1867e3cea04126ee893f8b395f7eb97899b"], S.item_ids(["abc"])
    assert_equal 5, S.item_ids(["x", "x", "", "", "\xff\x00".b]).uniq.size
    shuffled = (NAMES[3..] + NAMES[0, 3]).reverse
    assert_equal S.item_fingerprint(NAMES), S.item_fingerprint(shuffled)
    10.times do |seed|
      a = S.draw_items(NAMES, 5, seed: seed)
      b = S.draw_items(shuffled, 5, seed: seed)
      assert_equal a.map(&:id), b.map(&:id)
      assert_equal a.map { |p| NAMES[p.index] }, b.map { |p| shuffled[p.index] }
    end
    assert_raises(S::Error) { S.draw_items(NAMES, 9) }
    assert_raises(TypeError) { S.item_ids([1]) }
  end

  def test_item_session_resume_keyed_and_seal
    lot = S::ItemSession.new(NAMES, seed: 11)
    got = lot.draw(3, key: "k1")
    assert_equal got, lot.draw(3, key: "k1")
    assert_equal [1, 32], [lot.revision, lot.id.size]
    state = lot.save
    rest = S::ItemSession.load(state, NAMES.reverse).draw(5)
    assert_equal S.draw_items(NAMES, 8, seed: 11).map(&:id), (got + rest).map(&:id)
    assert_equal(-8, assert_raises(S::Error) { S::ItemSession.load(state, NAMES[1..]) }.code)

    key = SecureRandom.random_bytes(32)
    token = lot.seal(key, key_id: 1)
    again = S::ItemSession.unseal(token, key, NAMES.rotate(2))
    assert_equal lot.draw(5).map(&:id), again.draw(5).map(&:id)
    assert_raises(S::Error) { S::ItemSession.unseal(token, key, NAMES[1..]) }
  end
end
