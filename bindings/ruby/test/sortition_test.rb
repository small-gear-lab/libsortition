# frozen_string_literal: true

# cargo build --release && ruby -Ibindings/ruby/lib bindings/ruby/test/sortition_test.rb
require "minitest/autorun"
require "securerandom"
require "sortition"

class SortitionTest < Minitest::Test
  S = Sortition
  NAMES = %w[alice bob carol dave erin frank grace heidi].freeze

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
