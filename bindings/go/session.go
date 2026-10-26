package sortition

import (
	"runtime"
	"strings"

	"github.com/tetratelabs/wazero/api"
)

// base is what both kinds of session share. prefix picks the C functions
// ("sortition_session_" or "sortition_item_session_").
type base struct {
	handle uint32
	prefix string
}

func newBase(handle uint32, prefix string) *base {
	b := &base{handle: handle, prefix: prefix}
	// Closing is the rule, but a forgotten session must not leak the core's memory.
	runtime.SetFinalizer(b, func(b *base) { _ = b.Close() })
	return b
}

// live returns the handle; the caller holds the lock (via a frame).
func (b *base) live() (uint32, error) {
	if b.handle == 0 {
		return 0, errClosed
	}
	return b.handle, nil
}

func (b *base) call(f *frame, name string, args ...uint64) uint64 {
	return f.h.call(b.prefix+name, args...)
}

// Close frees the session. It is safe to call it more than once.
func (b *base) Close() error {
	f := begin()
	defer f.end()
	if b.handle != 0 {
		b.call(f, "free", uint64(b.handle))
		b.handle = 0
	}
	runtime.SetFinalizer(b, nil)
	return nil
}

func (b *base) counter(name string) (uint32, error) {
	f := begin()
	defer f.end()
	h, err := b.live()
	if err != nil {
		return 0, err
	}
	return api.DecodeU32(b.call(f, name, uint64(h))), nil
}

// Remaining is how many are not drawn yet.
func (b *base) Remaining() (uint32, error) { return b.counter("remaining") }

// DrawnCount is how many were drawn.
func (b *base) DrawnCount() (uint32, error) { return b.counter("drawn_count") }

// Revision is the number of draws that changed the state. A store can update a record only when the
// revision is the one it read, so that two updates made from the same state do not overwrite each other.
func (b *base) Revision() (uint64, error) {
	f := begin()
	defer f.end()
	h, err := b.live()
	if err != nil {
		return 0, err
	}
	return b.call(f, "revision", uint64(h)), nil
}

// ID is the session's identifier (32 lowercase hex characters), taken from the operating system when it was created.
func (b *base) ID() (string, error) {
	f := begin()
	defer f.end()
	h, err := b.live()
	if err != nil {
		return "", err
	}
	out := f.alloc(33)
	if rc := int32(api.DecodeU32(b.call(f, "id", uint64(h), uint64(out)))); rc != 0 {
		return "", fail(f, rc, false)
	}
	return f.cString(out), nil
}

// Save returns the state as JSON. It contains the internal state of the random generator: whoever has
// it can predict the next winners, so do not give it to a party you do not trust. Seal it instead.
func (b *base) Save() (string, error) {
	f := begin()
	defer f.end()
	h, err := b.live()
	if err != nil {
		return "", err
	}
	out := f.alloc(4)
	if rc := int32(api.DecodeU32(b.call(f, "save", uint64(h), uint64(out)))); rc != 0 {
		return "", fail(f, rc, false)
	}
	return f.takeString(f.u32(out)), nil
}

// Seal returns the state as a token that only the holder of key (32 bytes) can open, and that cannot
// be changed without notice. keyID (0 to 255) is a label for rotating keys; see SealKeyID.
func (b *base) Seal(key []byte, keyID uint8) (string, error) {
	if len(key) != 32 {
		return "", &Error{Code: ErrInvalid, Message: "key must be exactly 32 bytes"}
	}
	f := begin()
	defer f.end()
	h, err := b.live()
	if err != nil {
		return "", err
	}
	k := f.put(key, 0)
	out := f.alloc(4)
	if rc := int32(api.DecodeU32(b.call(f, "seal", uint64(h), uint64(k), uint64(keyID), uint64(out)))); rc != 0 {
		return "", fail(f, rc, false)
	}
	return f.takeString(f.u32(out)), nil
}

func putKey(f *frame, key *string) (uint32, error) {
	if key == nil {
		return 0, nil
	}
	if strings.ContainsRune(*key, 0) {
		return 0, &Error{Code: ErrInvalid, Message: "key must not contain NUL"}
	}
	return f.put([]byte(*key), 1), nil
}

// Session draws without replacement from 0..n-1 and continues where it stopped.
// Drawing in several steps, with a save and a restore in between, gives the same order as one draw.
// A session must not be used by two goroutines at the same time in a way that depends on the order.
type Session struct{ *base }

// NewSession creates a session over 0..n-1. Without WithSeed it runs a cryptographic stream.
func NewSession(n uint32, opts ...Option) (*Session, error) {
	c := options(opts)
	f := begin()
	defer f.end()
	out := f.alloc(4)
	if rc := int32(api.DecodeU32(f.h.call("sortition_session_new", uint64(n), uint64(f.putSeed(c.seed)), uint64(out)))); rc != 0 {
		return nil, fail(f, rc, false)
	}
	return &Session{newBase(f.u32(out), "sortition_session_")}, nil
}

// LoadSession restores a session from Save.
func LoadSession(state string) (*Session, error) {
	f := begin()
	defer f.end()
	text := f.put([]byte(state), 0)
	out := f.alloc(4)
	if rc := int32(api.DecodeU32(f.h.call("sortition_session_load", uint64(text), uint64(len(state)), uint64(out)))); rc != 0 {
		return nil, fail(f, rc, true)
	}
	return &Session{newBase(f.u32(out), "sortition_session_")}, nil
}

// UnsealSession restores a session from a token made by Seal. A malformed token, a wrong key and a
// changed token all give ErrState.
func UnsealSession(token string, key []byte) (*Session, error) {
	if len(key) != 32 {
		return nil, &Error{Code: ErrInvalid, Message: "key must be exactly 32 bytes"}
	}
	f := begin()
	defer f.end()
	t := f.put([]byte(token), 0)
	k := f.put(key, 0)
	out := f.alloc(4)
	if rc := int32(api.DecodeU32(f.h.call("sortition_session_unseal", uint64(t), uint64(len(token)), uint64(k), uint64(out)))); rc != 0 {
		return nil, fail(f, rc, true)
	}
	return &Session{newBase(f.u32(out), "sortition_session_")}, nil
}

func (s *Session) draw(k uint32, key *string) ([]uint32, error) {
	f := begin()
	defer f.end()
	h, err := s.live()
	if err != nil {
		return nil, err
	}
	keyPtr, err := putKey(f, key)
	if err != nil {
		return nil, err
	}
	out := f.alloc(4 * k)
	if rc := int32(api.DecodeU32(f.h.call("sortition_session_draw_keyed", uint64(h), uint64(k), uint64(keyPtr), uint64(out), 0))); rc != 0 {
		return nil, fail(f, rc, false)
	}
	return f.u32s(out, k), nil
}

// Draw draws the next k numbers. More than Remaining gives ErrInvalid and leaves the state unchanged.
func (s *Session) Draw(k uint32) ([]uint32, error) { return s.draw(k, nil) }

// DrawKeyed draws with an idempotency key (1 to 128 bytes). The same key and the same k again does
// not change the state and returns the earlier result; the same key with another k gives ErrConflict.
func (s *Session) DrawKeyed(k uint32, key string) ([]uint32, error) { return s.draw(k, &key) }

// ItemSession is a session over a fixed set of candidates.
type ItemSession struct{ *base }

// NewItemSession creates a session over a fixed set of candidates.
func NewItemSession[T Item](items []T, opts ...Option) (*ItemSession, error) {
	c := options(opts)
	f := begin()
	defer f.end()
	data, lens, count := f.putItems(chunks(items))
	out := f.alloc(4)
	if rc := int32(api.DecodeU32(f.h.call("sortition_item_session_new", uint64(data), uint64(lens), uint64(count), uint64(f.putSeed(c.seed)), uint64(out)))); rc != 0 {
		return nil, fail(f, rc, false)
	}
	return &ItemSession{newBase(f.u32(out), "sortition_item_session_")}, nil
}

// LoadItemSession restores a session from Save and the same set of candidates (the order may differ;
// another set gives ErrState). Indexes of later draws refer to the items you pass here.
func LoadItemSession[T Item](state string, items []T) (*ItemSession, error) {
	f := begin()
	defer f.end()
	text := f.put([]byte(state), 0)
	data, lens, count := f.putItems(chunks(items))
	out := f.alloc(4)
	if rc := int32(api.DecodeU32(f.h.call("sortition_item_session_load", uint64(text), uint64(len(state)), uint64(data), uint64(lens), uint64(count), uint64(out)))); rc != 0 {
		return nil, fail(f, rc, true)
	}
	return &ItemSession{newBase(f.u32(out), "sortition_item_session_")}, nil
}

// UnsealItemSession restores a session from a token made by Seal and the same set of candidates.
func UnsealItemSession[T Item](token string, key []byte, items []T) (*ItemSession, error) {
	if len(key) != 32 {
		return nil, &Error{Code: ErrInvalid, Message: "key must be exactly 32 bytes"}
	}
	f := begin()
	defer f.end()
	t := f.put([]byte(token), 0)
	k := f.put(key, 0)
	data, lens, count := f.putItems(chunks(items))
	out := f.alloc(4)
	if rc := int32(api.DecodeU32(f.h.call("sortition_item_session_unseal", uint64(t), uint64(len(token)), uint64(k), uint64(data), uint64(lens), uint64(count), uint64(out)))); rc != 0 {
		return nil, fail(f, rc, true)
	}
	return &ItemSession{newBase(f.u32(out), "sortition_item_session_")}, nil
}

func (s *ItemSession) draw(k uint32, key *string) ([]Pick, error) {
	f := begin()
	defer f.end()
	h, err := s.live()
	if err != nil {
		return nil, err
	}
	keyPtr, err := putKey(f, key)
	if err != nil {
		return nil, err
	}
	indexes, ids := f.alloc(4*k), f.alloc(32*k)
	if rc := int32(api.DecodeU32(f.h.call("sortition_item_session_draw_keyed", uint64(h), uint64(k), uint64(keyPtr), uint64(indexes), uint64(ids), 0))); rc != 0 {
		return nil, fail(f, rc, false)
	}
	return readPicks(f, indexes, ids, k), nil
}

// Draw draws the next k candidates. More than Remaining gives ErrInvalid and leaves the state unchanged.
func (s *ItemSession) Draw(k uint32) ([]Pick, error) { return s.draw(k, nil) }

// DrawKeyed draws with an idempotency key, like Session.DrawKeyed.
func (s *ItemSession) DrawKeyed(k uint32, key string) ([]Pick, error) { return s.draw(k, &key) }
