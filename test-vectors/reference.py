#!/usr/bin/env python3
"""libsortition の参照実装。docs/SPEC.md の文面だけから書いた、Python 標準ライブラリだけの第二の実装。

  python3 test-vectors/reference.py [vectors.json]

vectors.json（Rust のコードが作る）の値を、この実装で再現できるか全部確かめる。食い違いがあれば表示して、
終了コード 1 で終わる。仕様書に抜けや曖昧さがあれば、ここで食い違いとして現れる。
速さや使いやすさは目的ではないので、仕様書の擬似コードにそのまま対応する書き方にしてある。
"""
import base64
import hashlib
import json
import sys
from pathlib import Path

M64 = (1 << 64) - 1
ALGORITHM = "libsortition-draw/1"


def le64(x):
    return x.to_bytes(8, "little")


def rotl(x, k):
    return ((x << k) | (x >> (64 - k))) & M64


# ---- 3. seed from a string ----

def seed_digest(text_bytes):
    return hashlib.sha256(text_bytes).digest()


def seed_from_text(text):
    return int.from_bytes(seed_digest(text.encode("utf-8"))[:8], "little")


# ---- 4.1 seeded stream: xoshiro256** ----

class Xoshiro:
    def __init__(self, seed):
        z = seed
        self.s = []
        for _ in range(4):
            z = (z + 0x9E3779B97F4A7C15) & M64
            x = z
            x = ((x ^ (x >> 30)) * 0xBF58476D1CE4E5B9) & M64
            x = ((x ^ (x >> 27)) * 0x94D049BB133111EB) & M64
            self.s.append(x ^ (x >> 31))

    def next(self):
        s = self.s
        result = (rotl((s[1] * 5) & M64, 7) * 9) & M64
        t = (s[1] << 17) & M64
        s[2] ^= s[0]
        s[3] ^= s[1]
        s[1] ^= s[2]
        s[0] ^= s[3]
        s[2] ^= t
        s[3] = rotl(s[3], 45)
        return result


# ---- ChaCha20 (RFC 8439) ----

def _qr(st, a, b, c, d):
    st[a] = (st[a] + st[b]) & 0xFFFFFFFF
    st[d] ^= st[a]
    st[d] = ((st[d] << 16) | (st[d] >> 16)) & 0xFFFFFFFF
    st[c] = (st[c] + st[d]) & 0xFFFFFFFF
    st[b] ^= st[c]
    st[b] = ((st[b] << 12) | (st[b] >> 20)) & 0xFFFFFFFF
    st[a] = (st[a] + st[b]) & 0xFFFFFFFF
    st[d] ^= st[a]
    st[d] = ((st[d] << 8) | (st[d] >> 24)) & 0xFFFFFFFF
    st[c] = (st[c] + st[d]) & 0xFFFFFFFF
    st[b] ^= st[c]
    st[b] = ((st[b] << 7) | (st[b] >> 25)) & 0xFFFFFFFF


_CONSTANTS = [0x61707865, 0x3320646E, 0x79622D32, 0x6B206574]  # "expand 32-byte k"


def _rounds(st):
    for _ in range(10):
        _qr(st, 0, 4, 8, 12)
        _qr(st, 1, 5, 9, 13)
        _qr(st, 2, 6, 10, 14)
        _qr(st, 3, 7, 11, 15)
        _qr(st, 0, 5, 10, 15)
        _qr(st, 1, 6, 11, 12)
        _qr(st, 2, 7, 8, 13)
        _qr(st, 3, 4, 9, 14)


def _words(b):
    return [int.from_bytes(b[i:i + 4], "little") for i in range(0, len(b), 4)]


def chacha20_block(key, counter, nonce12):
    init = _CONSTANTS + _words(key) + [counter] + _words(nonce12)
    st = list(init)
    _rounds(st)
    return b"".join(((a + b) & 0xFFFFFFFF).to_bytes(4, "little") for a, b in zip(st, init))


def chacha20_keystream(key, nonce12, start, length):
    """key stream のバイト start .. start + length（ブロックのカウンタは 0 から）"""
    out = b""
    block = start // 64
    while len(out) < (start % 64) + length:
        out += chacha20_block(key, block, nonce12)
        block += 1
    return out[start % 64:start % 64 + length]


# ---- 4.2 stream of a session without a seed: ChaCha20 ----

class ChaChaStream:
    def __init__(self, key, pos):
        self.key, self.pos = key, pos

    def next(self):
        value = int.from_bytes(chacha20_keystream(self.key, bytes(12), self.pos, 8), "little")
        self.pos += 8
        return value


# ---- 5. a uniform number below a bound ----

def below(stream, bound):
    zone = M64 - (((M64 % bound) + 1) % bound)
    while True:
        v = stream.next()
        if v <= zone:
            return v % bound


# ---- 6. drawing k of n (it can continue: swapped and the start position are given) ----

def draw_from(stream, n, k, swapped=None, start=0):
    swapped = {} if swapped is None else swapped
    result = []
    for i in range(start, start + k):
        j = i + below(stream, n - i)
        vj = swapped.get(j, j)
        vi = swapped.get(i, i)
        swapped[j] = vi
        result.append(vj)
    return result


def draw(n, k, seed):
    assert k <= n
    return draw_from(Xoshiro(seed), n, k)


# ---- 7. candidates: IDs, order and fingerprint ----

def candidate_ids(items):
    seen = {}
    ids = []
    for item in items:
        occurrence = seen.get(item, 0)
        seen[item] = occurrence + 1
        ids.append(hashlib.sha256(b"libsortition/candidate/v1\x00" + item + le64(occurrence)).digest())
    return ids


def fingerprint(ids):
    return hashlib.sha256(b"libsortition/set/v1\x00" + le64(len(ids)) + b"".join(sorted(ids))).digest()


# ---- 8. drawing candidates and prize stages ----

def draw_items(items, k, seed):
    ids = candidate_ids(items)
    order = sorted(range(len(items)), key=lambda i: ids[i])  # canonical order: positions -> input indexes
    return [(order[p], ids[order[p]]) for p in draw(len(items), k, seed)]


def draw_stages(items, stages, seed):
    total = sum(count for _, count in stages)
    picks = draw_items(items, total, seed)
    out, at = [], 0
    for name, count in stages:
        out.append((name, count, picks[at:at + count]))
        at += count
    return out


# ---- 9. sessions (only what the vectors need: continue from a saved ChaCha20 state) ----

class Session:
    def __init__(self, state):
        rng = state["rng"]
        assert rng["kind"] == "chacha20"
        self.stream = ChaChaStream(bytes.fromhex(rng["key"]), int(rng["pos"]))
        self.n = state["n"]
        self.drawn = list(state["drawn"])
        self.swapped = {int(p): v for p, v in state["swapped"]}

    def draw(self, k):
        assert k <= self.n - len(self.drawn)
        picks = draw_from(self.stream, self.n, k, self.swapped, start=len(self.drawn))
        self.drawn += picks
        return picks


# ---- 11. sealed tokens: XChaCha20-Poly1305 ----

def hchacha20(key, nonce16):
    st = _CONSTANTS + _words(key) + _words(nonce16)
    _rounds(st)
    return b"".join(w.to_bytes(4, "little") for w in st[0:4] + st[12:16])


def poly1305(key32, message):
    r = int.from_bytes(key32[:16], "little") & 0x0FFFFFFC0FFFFFFC0FFFFFFC0FFFFFFF
    s = int.from_bytes(key32[16:], "little")
    p = (1 << 130) - 5
    acc = 0
    for i in range(0, len(message), 16):
        block = message[i:i + 16]
        acc = ((acc + int.from_bytes(block + b"\x01", "little")) * r) % p
    return ((acc + s) & ((1 << 128) - 1)).to_bytes(16, "little")


def _pad16(b):
    return b"\x00" * (-len(b) % 16)


def _xor_stream(key, nonce12, data):
    out = bytearray()
    for i in range(0, len(data), 64):
        ks = chacha20_block(key, 1 + i // 64, nonce12)
        out += bytes(a ^ b for a, b in zip(data[i:i + 64], ks))
    return bytes(out)


def _tag(key, nonce12, aad, ciphertext):
    otk = chacha20_block(key, 0, nonce12)[:32]
    mac_data = aad + _pad16(aad) + ciphertext + _pad16(ciphertext) + le64(len(aad)) + le64(len(ciphertext))
    return poly1305(otk, mac_data)


def xchacha20poly1305_seal(key, nonce24, aad, plaintext):
    subkey = hchacha20(key, nonce24[:16])
    nonce12 = bytes(4) + nonce24[16:]
    ciphertext = _xor_stream(subkey, nonce12, plaintext)
    return ciphertext + _tag(subkey, nonce12, aad, ciphertext)


def xchacha20poly1305_open(key, nonce24, aad, data):
    subkey = hchacha20(key, nonce24[:16])
    nonce12 = bytes(4) + nonce24[16:]
    ciphertext, tag = data[:-16], data[-16:]
    if _tag(subkey, nonce12, aad, ciphertext) != tag:
        return None
    return _xor_stream(subkey, nonce12, ciphertext)


def b64url_encode(b):
    return base64.urlsafe_b64encode(b).decode().rstrip("=")


def b64url_decode(s):
    return base64.urlsafe_b64decode(s + "=" * (-len(s) % 4))


def seal(key, key_id, nonce24, plaintext):
    aad = b"libsortition/seal/v1" + bytes([key_id])
    body = bytes([key_id]) + nonce24 + xchacha20poly1305_seal(key, nonce24, aad, plaintext)
    return "sortition1." + b64url_encode(body)


def unseal(key, token):
    assert token.startswith("sortition1.")
    raw = b64url_decode(token[len("sortition1."):])
    key_id, nonce24, rest = raw[0], raw[1:25], raw[25:]
    return xchacha20poly1305_open(key, nonce24, b"libsortition/seal/v1" + bytes([key_id]), rest)


# ---- checking the vectors ----

def check(vectors):
    problems = []
    count = 0

    def eq(label, got, want):
        nonlocal count
        count += 1
        if got != want:
            problems.append(f"{label}: got {got!r}, expected {want!r}")

    eq("format", vectors["format"], "libsortition-test-vectors/1")
    eq("algorithm", vectors["algorithm"], ALGORITHM)

    for v in vectors["seed_from_text"]:
        eq(f"seed digest of {v['text']!r}", seed_digest(v["text"].encode()).hex(), v["digest"])
        eq(f"seed of {v['text']!r}", str(seed_from_text(v["text"])), v["seed"])

    for v in vectors["xoshiro256"]:
        x = Xoshiro(int(v["seed"]))
        eq(f"xoshiro state for seed {v['seed']}", [str(s) for s in x.s], v["state"])
        eq(f"xoshiro outputs for seed {v['seed']}", [str(x.next()) for _ in v["outputs"]], v["outputs"])

    for v in vectors["below"]:
        x = Xoshiro(int(v["seed"]))
        eq(f"below({v['bound']})", [str(below(x, int(v["bound"]))) for _ in v["values"]], v["values"])

    for v in vectors["draw"]:
        eq(f"draw({v['n']}, {v['k']}, seed {v['seed']})", draw(v["n"], v["k"], int(v["seed"])), v["result"])

    sets = {}
    for v in vectors["item_sets"]:
        items = [bytes.fromhex(h) for h in v["items_hex"]]
        sets[v["name"]] = items
        ids = candidate_ids(items)
        eq(f"ids of {v['name']}", [i.hex() for i in ids], v["ids"])
        eq(f"fingerprint of {v['name']}", fingerprint(ids).hex(), v["fingerprint"])

    for v in vectors["draw_items"]:
        got = [{"index": i, "id": d.hex()} for i, d in draw_items(sets[v["set"]], v["k"], int(v["seed"]))]
        eq(f"draw_items({v['set']}, {v['k']}, seed {v['seed']})", got, v["winners"])

    for v in vectors["stages"]:
        stages = [(s["name"], s["count"]) for s in v["stages"]]
        got = [
            {"name": n, "count": c, "winners": [{"index": i, "id": d.hex()} for i, d in w]}
            for n, c, w in draw_stages(sets[v["set"]], stages, seed_from_text(v["seed_text"]))
        ]
        eq(f"stages of {v['set']} with {v['seed_text']!r}", got, v["stages"])

    for v in vectors["chacha20_stream"]:
        s = ChaChaStream(bytes.fromhex(v["key"]), int(v["pos"]))
        eq(f"chacha20 stream key {v['key'][:8]}.. pos {v['pos']}", [str(s.next()) for _ in v["outputs"]], v["outputs"])

    v = vectors["chacha20_session"]
    session = Session(v["state"])
    eq("chacha20 session draws", [session.draw(k) for k in v["draws"]], v["expected"])

    v = vectors["seal"]
    key, nonce = bytes.fromhex(v["key"]), bytes.fromhex(v["nonce"])
    eq("sealed token", seal(key, v["key_id"], nonce, v["plaintext"].encode()), v["token"])
    eq("unsealed state", (unseal(key, v["token"]) or b"").decode(), v["plaintext"])
    tampered = v["token"][:-1] + ("A" if v["token"][-1] != "A" else "B")
    eq("a changed token cannot be opened", unseal(key, tampered), None)
    eq("a wrong key cannot open the token", unseal(bytes(32), v["token"]), None)
    session = Session(json.loads(v["plaintext"]))
    eq("draws from the unsealed state", [session.draw(k) for k in v["draws"]], v["expected"])

    return count, problems


def main():
    path = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).with_name("vectors.json")
    vectors = json.loads(path.read_text(encoding="utf-8"))
    count, problems = check(vectors)
    if problems:
        for p in problems:
            print("MISMATCH:", p)
        print(f"{len(problems)} of {count} checks failed")
        sys.exit(1)
    print(f"OK: the reference implementation reproduces all {count} checks in {path.name}")


if __name__ == "__main__":
    main()
