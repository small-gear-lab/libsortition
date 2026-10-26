"""libsortition の Python バインディングのテスト（標準ライブラリの unittest）。

  cargo build --release && python3 -m unittest discover -s bindings/python/tests
"""
import json
import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
import sortition as s  # noqa: E402

NAMES = [b"alice", "bob", b"carol", b"dave", b"erin", b"frank", b"grace", b"heidi"]


class Basics(unittest.TestCase):
    def test_draw_is_reproducible(self):
        self.assertEqual(s.draw(10, 3, seed=1), [7, 2, 6])
        self.assertEqual(sorted(s.draw(100, 100)), list(range(100)))
        with self.assertRaises(s.SortitionError):
            s.draw(3, 4)

    def test_text_and_data(self):
        self.assertEqual(sorted(s.draw_text("a\n b \n\nc", 3)), ["a", "b", "c"])
        self.assertEqual(len(s.draw_data('{"m": [1, 2, 3]}', 2, "json", path="m")), 2)


class Session(unittest.TestCase):
    def test_split_draws_equal_one_draw_and_resume(self):
        a = s.Session(200, seed=9)
        got = a.draw(20)
        b = s.Session.load(a.save())
        got += b.draw(30)
        self.assertEqual(got, s.draw(200, 50, seed=9))
        self.assertEqual((b.remaining, b.drawn_count), (150, 50))
        self.assertEqual(json.loads(b.save())["drawn"], got)

    def test_unseeded_resume_is_deterministic(self):
        a = s.Session(100)
        a.draw(10)
        saved = a.save()
        self.assertEqual(s.Session.load(saved).draw(5), a.draw(5))

    def test_errors_and_close(self):
        a = s.Session(5, seed=1)
        a.draw(3)
        before = a.save()
        with self.assertRaises(s.SortitionError) as e:
            a.draw(3)
        self.assertEqual(e.exception.code, -1)
        self.assertEqual(a.save(), before)
        with self.assertRaises(s.SortitionError) as e:
            s.Session.load("{}")
        self.assertEqual(e.exception.code, -8)
        a.close()
        a.close()
        with self.assertRaises(ValueError):
            a.draw(1)

    def test_idempotency_key(self):
        a = s.Session(50)
        first = a.draw(3, key="req-1")
        state = a.save()
        self.assertEqual(a.draw(3, key="req-1"), first)  # 再送は前回と同じ
        self.assertEqual((a.save(), a.revision), (state, 1))
        with self.assertRaises(s.SortitionError) as e:
            a.draw(4, key="req-1")
        self.assertEqual(e.exception.code, -9)
        with self.assertRaises(s.SortitionError):
            a.draw(1, key="")
        with self.assertRaises(TypeError):
            a.draw(1, key=5)
        self.assertEqual((len(a.id), json.loads(state)["id"]), (32, a.id))

    def test_replay_after_everything_was_drawn(self):
        b = s.Session(5)
        all5 = b.draw(5, key="all")
        self.assertEqual((b.draw(5, key="all"), b.remaining), (all5, 0))

    def test_stale_state_resend_gives_same_result(self):
        a = s.Session(100)
        a.draw(4, key="a")
        stale = a.save()
        want = a.draw(6, key="b")
        after = a.save()
        old = s.Session.load(stale)
        self.assertEqual(old.draw(6, key="b"), want)
        self.assertEqual(old.save(), after)

    def test_seal(self):
        key = os.urandom(32)
        a = s.Session(50)
        a.draw(3, key="r")
        token = a.seal(key, key_id=7)
        self.assertTrue(token.startswith("sortition1."))
        self.assertEqual(s.seal_key_id(token), 7)
        self.assertNotIn(json.loads(a.save())["rng"]["key"], token)
        b = s.Session.unseal(token, key)
        self.assertEqual(b.save(), a.save())
        self.assertEqual(b.draw(2), a.draw(2))
        for bad in (lambda: s.Session.unseal(token, os.urandom(32)),
                    lambda: s.Session.unseal(token[:-1] + ("A" if token[-1] != "A" else "B"), key),
                    lambda: s.Session.unseal("junk", key),
                    lambda: s.seal_key_id("junk")):
            with self.assertRaises(s.SortitionError) as e:
                bad()
            self.assertEqual(e.exception.code, -8)
        for bad_key in (b"short", "x" * 32, None):
            with self.assertRaises(ValueError):
                a.seal(bad_key)


class Seed(unittest.TestCase):
    def test_seed_from_text_follows_the_core_rule(self):
        # printf sortition | sha256sum の先頭 8 バイトをリトルエンディアンで読んだ値（全バインディング共通）
        self.assertEqual(s.seed_from_text("sortition"), 131367258856992384)
        self.assertEqual(s.seed_from_text(b"sortition"), 131367258856992384)
        self.assertEqual(s.seed_commit("sortition"), "80ae496fd3b5d201bb96339356ff252e2b1cdc214ad567f6ebe8de4537e6b31c")
        self.assertEqual(s.seed_from_text(""), 0x141cfc9842c4b0e3)
        self.assertEqual(s.seed_from_text("日本語"), s.seed_from_text("日本語".encode("utf-8")))
        self.assertNotEqual(s.seed_from_text("a"), s.seed_from_text("b"))
        # 抽選の seed にそのまま渡せる
        self.assertEqual(len(s.draw(10, 3, seed=s.seed_from_text("abc"))), 3)


class Stages(unittest.TestCase):
    PLAN = [("副賞", 3), ("大賞", 1)]

    def test_stages_cut_one_draw_in_order(self):
        seed = s.seed_from_text("stages")
        got = s.draw_stages(NAMES, self.PLAN, seed=seed)
        whole = s.draw_items(NAMES, 4, seed=seed)
        self.assertEqual([(r.name, r.count, len(r.winners)) for r in got], [("副賞", 3, 3), ("大賞", 1, 1)])
        self.assertEqual([p for r in got for p in r.winners], whole)
        self.assertEqual(len({p.id for r in got for p in r.winners}), 4)  # 重複当選なし

    def test_independent_of_the_order_of_items(self):
        seed = s.seed_from_text("stages")
        a = s.draw_stages(NAMES, self.PLAN, seed=seed)
        b = s.draw_stages(list(reversed(NAMES)), self.PLAN, seed=seed)
        self.assertEqual([[p.id for p in r.winners] for r in a], [[p.id for p in r.winners] for r in b])

    def test_accepts_stage_pairs_and_dicts(self):
        seed = 5
        want = s.draw_stages(NAMES, self.PLAN, seed=seed)
        self.assertEqual(s.draw_stages(NAMES, [s.Stage("副賞", 3), s.Stage("大賞", 1)], seed=seed), want)
        self.assertEqual(s.draw_stages(NAMES, [{"name": "副賞", "count": 3}, {"name": "大賞", "count": 1}], seed=seed), want)
        self.assertEqual(s.draw_stages(NAMES, []), [])
        self.assertEqual(s.draw_stages(NAMES, [("空", 0)])[0].winners, [])

    def test_errors(self):
        with self.assertRaises(s.SortitionError):
            s.draw_stages(NAMES, [("多すぎる", 99)])
        for bad in ([("a", -1)], [("a", 1.5)], [("a", True)]):
            with self.assertRaises(ValueError):
                s.draw_stages(NAMES, bad)
        for bad in ([(1, 1)], [("a",)], ["ab"], [{"count": 1}]):
            with self.assertRaises((TypeError, ValueError)):
                s.draw_stages(NAMES, bad)


class Items(unittest.TestCase):
    def test_ids_and_fingerprint(self):
        self.assertEqual(s.item_ids([b"abc"]),
                         ["3b39139043226007aae3c88c03f4a1867e3cea04126ee893f8b395f7eb97899b"])
        self.assertEqual(len(set(s.item_ids(["x", "x", "", "", b"\xff\x00\xfe"]))), 5)
        self.assertEqual(s.item_fingerprint(NAMES), s.item_fingerprint(NAMES[3:] + NAMES[:3]))
        with self.assertRaises(TypeError):
            s.item_ids([1])

    def test_order_independence(self):
        shuffled = list(reversed(NAMES[3:] + NAMES[:3]))
        for seed in range(10):
            a = s.draw_items(NAMES, 5, seed=seed)
            b = s.draw_items(shuffled, 5, seed=seed)
            self.assertEqual([p.id for p in a], [p.id for p in b])
            self.assertEqual([NAMES[p.index] for p in a], [shuffled[p.index] for p in b])
        with self.assertRaises(s.SortitionError):
            s.draw_items(NAMES, 9)

    def test_session_resume_keyed_and_seal(self):
        lot = s.ItemSession(NAMES, seed=11)
        got = lot.draw(3, key="k1")
        self.assertEqual(lot.draw(3, key="k1"), got)
        self.assertEqual((lot.revision, len(lot.id)), (1, 32))
        state = lot.save()
        rest = s.ItemSession.load(state, reversed(NAMES)).draw(5)
        self.assertEqual([p.id for p in got + rest], [p.id for p in s.draw_items(NAMES, 8, seed=11)])
        with self.assertRaises(s.SortitionError) as e:
            s.ItemSession.load(state, NAMES[1:])
        self.assertEqual(e.exception.code, -8)

        key = os.urandom(32)
        token = lot.seal(key, key_id=1)
        again = s.ItemSession.unseal(token, key, NAMES[2:] + NAMES[:2])
        self.assertEqual([p.id for p in again.draw(5)], [p.id for p in lot.draw(5)])
        with self.assertRaises(s.SortitionError):
            s.ItemSession.unseal(token, key, NAMES[1:])


if __name__ == "__main__":
    unittest.main()
