// Package sortition draws fair, repeatable and verifiable random lotteries: k of n without
// replacement, with seeds you can commit to, candidate IDs that do not depend on the order of your
// list, prize stages, and sessions that you can save and continue (see docs/SPEC.md).
//
// It has no cgo dependency: the Rust core is embedded as WebAssembly and runs with wazero.
// Everything is safe for concurrent use, but calls are serialized.
package sortition

import (
	"encoding/hex"
	"errors"
	"fmt"
	"strings"

	"github.com/tetratelabs/wazero/api"
)

// Error codes of the core.
const (
	ErrInvalid  = -1 // an argument is out of range (k > n, an empty key, ...)
	ErrState    = -8 // an invalid saved state or sealed token (or another candidate set)
	ErrConflict = -9 // an idempotency key was used with a different request
)

// Error is a failure reported by the core.
type Error struct {
	Code    int    // one of the Err constants, or another negative code from docs/SPEC.md
	Message string // details, when the core has them
}

func (e *Error) Error() string {
	if e.Message != "" {
		return fmt.Sprintf("sortition: %s (code %d)", e.Message, e.Code)
	}
	return fmt.Sprintf("sortition: error code %d", e.Code)
}

func fail(f *frame, rc int32, detailed bool) error {
	e := &Error{Code: int(rc)}
	if detailed {
		e.Message = f.lastError()
	}
	return e
}

// Item is a candidate: a string (its UTF-8 bytes) or a byte slice.
type Item interface{ ~string | ~[]byte }

// Pick is a winner: its position in the list that was passed, and its ID (SHA-256, 64 lowercase hex characters).
type Pick struct {
	Index uint32
	ID    string
}

// Stage is one prize stage of a plan.
type Stage struct {
	Name  string
	Count uint32
}

// StageResult holds the winners of one stage.
type StageResult struct {
	Name    string
	Count   uint32
	Winners []Pick
}

// DataFormat is the format of structured data for DrawData.
type DataFormat int32

// The formats of DrawData.
const (
	JSON DataFormat = 0
	YAML DataFormat = 1
	TOML DataFormat = 2
)

type config struct{ seed *uint64 }

// Option configures a draw or a session.
type Option func(*config)

// WithSeed makes the result repeatable (xoshiro256**). Without it, the draw uses the operating
// system's randomness. Use SeedFromText to derive a seed from a string you can commit to.
func WithSeed(seed uint64) Option { return func(c *config) { c.seed = &seed } }

func options(opts []Option) config {
	var c config
	for _, o := range opts {
		o(&c)
	}
	return c
}

func chunks[T Item](items []T) [][]byte {
	out := make([][]byte, len(items))
	for i, it := range items {
		out[i] = []byte(it)
	}
	return out
}

func seedInfo(text []byte) (uint64, []byte, error) {
	f := begin()
	defer f.end()
	t := f.put(text, 0)
	seed := f.alloc(8)
	digest := f.alloc(32)
	rc := int32(api.DecodeU32(f.h.call("sortition_seed_from_text", uint64(t), uint64(len(text)), uint64(seed), uint64(digest))))
	if rc != 0 {
		return 0, nil, fail(f, rc, false)
	}
	v, _ := f.h.mem.ReadUint64Le(seed)
	return v, f.bytes(digest, 32), nil
}

// SeedFromText derives a seed from a seed string: the first 8 bytes of its SHA-256, as a little-endian number.
func SeedFromText(text string) uint64 {
	seed, _, err := seedInfo([]byte(text))
	if err != nil {
		panic(err) // cannot happen: any bytes are valid
	}
	return seed
}

// SeedCommit is the SHA-256 of a seed string (64 lowercase hex characters): the commitment to publish before the draw.
func SeedCommit(text string) string {
	_, digest, err := seedInfo([]byte(text))
	if err != nil {
		panic(err)
	}
	return hex.EncodeToString(digest)
}

// Version is the version of the core.
func Version() string {
	f := begin()
	defer f.end()
	return f.cString(api.DecodeU32(f.h.call("sortition_version")))
}

// Draw draws k distinct numbers from 0..n-1, in the order drawn.
func Draw(n, k uint32, opts ...Option) ([]uint32, error) {
	c := options(opts)
	f := begin()
	defer f.end()
	out := f.alloc(4 * k)
	rc := int32(api.DecodeU32(f.h.call("sortition_draw", uint64(n), uint64(k), uint64(f.putSeed(c.seed)), uint64(out))))
	if rc != 0 {
		return nil, fail(f, rc, false)
	}
	return f.u32s(out, k), nil
}

// DrawText draws k lines from text (one candidate per line; lines are trimmed, empty lines are not candidates).
func DrawText(text string, k uint32, opts ...Option) ([]string, error) {
	c := options(opts)
	f := begin()
	defer f.end()
	t := f.put([]byte(text), 0)
	capacity := uint32(len(text)) + 1
	out := f.alloc(capacity)
	n := int64(f.h.call("sortition_draw_text", uint64(t), uint64(len(text)), uint64(k), uint64(f.putSeed(c.seed)), uint64(out), uint64(capacity)))
	if n < 0 {
		return nil, fail(f, int32(n), false)
	}
	if n == 0 {
		return []string{}, nil
	}
	return strings.Split(string(f.bytes(out, uint32(n))), "\n"), nil
}

// DrawData draws k elements of an array inside JSON, YAML or TOML data and returns the chosen
// elements as a JSON array. path says where the array is ("a.b.0"); "" is the root.
func DrawData(format DataFormat, text string, k uint32, path string, opts ...Option) (string, error) {
	c := options(opts)
	if strings.ContainsRune(path, 0) {
		return "", &Error{Code: ErrInvalid, Message: "path must not contain NUL"}
	}
	f := begin()
	defer f.end()
	t := f.put([]byte(text), 0)
	p := f.put([]byte(path), 1)
	out := f.alloc(4)
	rc := int32(api.DecodeU32(f.h.call("sortition_draw_data", uint64(format), uint64(t), uint64(len(text)), uint64(p), uint64(k), uint64(f.putSeed(c.seed)), uint64(out))))
	if rc != 0 {
		return "", fail(f, rc, true)
	}
	return f.takeString(f.u32(out)), nil
}

// SealKeyID reads the key ID of a sealed token (to choose the key before opening it).
func SealKeyID(token string) (uint8, error) {
	f := begin()
	defer f.end()
	t := f.put([]byte(token), 0)
	out := f.alloc(1)
	rc := int32(api.DecodeU32(f.h.call("sortition_seal_key_id", uint64(t), uint64(len(token)), uint64(out))))
	if rc != 0 {
		return 0, fail(f, rc, true)
	}
	b, _ := f.h.mem.ReadByte(out)
	return b, nil
}

func hexAt(f *frame, p uint32) string { return hex.EncodeToString(f.bytes(p, 32)) }

func readPicks(f *frame, indexes, ids, k uint32) []Pick {
	out := make([]Pick, k)
	for i := range out {
		out[i] = Pick{Index: f.u32(indexes + 4*uint32(i)), ID: hexAt(f, ids+32*uint32(i))}
	}
	return out
}

// ItemIDs returns the ID of each candidate, in the order of items.
func ItemIDs[T Item](items []T) ([]string, error) {
	f := begin()
	defer f.end()
	data, lens, count := f.putItems(chunks(items))
	out := f.alloc(32 * count)
	rc := int32(api.DecodeU32(f.h.call("sortition_item_ids", uint64(data), uint64(lens), uint64(count), uint64(out))))
	if rc != 0 {
		return nil, fail(f, rc, false)
	}
	ids := make([]string, count)
	for i := range ids {
		ids[i] = hexAt(f, out+32*uint32(i))
	}
	return ids, nil
}

// ItemFingerprint is the fingerprint of the set of candidates (64 lowercase hex characters). It does
// not depend on the order of items: publish it with the commitment, before the draw.
func ItemFingerprint[T Item](items []T) (string, error) {
	f := begin()
	defer f.end()
	data, lens, count := f.putItems(chunks(items))
	out := f.alloc(32)
	rc := int32(api.DecodeU32(f.h.call("sortition_item_fingerprint", uint64(data), uint64(lens), uint64(count), uint64(out))))
	if rc != 0 {
		return "", fail(f, rc, false)
	}
	return hexAt(f, out), nil
}

// DrawItems draws k candidates without replacement. The same set of candidates and the same seed give
// the same winners whatever the order of items.
func DrawItems[T Item](items []T, k uint32, opts ...Option) ([]Pick, error) {
	c := options(opts)
	f := begin()
	defer f.end()
	data, lens, count := f.putItems(chunks(items))
	indexes, ids := f.alloc(4*k), f.alloc(32*k)
	rc := int32(api.DecodeU32(f.h.call("sortition_items_draw", uint64(data), uint64(lens), uint64(count), uint64(k), uint64(f.putSeed(c.seed)), uint64(indexes), uint64(ids))))
	if rc != 0 {
		return nil, fail(f, rc, false)
	}
	return readPicks(f, indexes, ids, k), nil
}

// DrawStages draws prize stages in order without giving anyone two prizes. It draws the total once
// and cuts it up in the order of stages, which has the same probabilities as drawing stage by stage.
func DrawStages[T Item](items []T, stages []Stage, opts ...Option) ([]StageResult, error) {
	var total uint64
	for _, s := range stages {
		total += uint64(s.Count)
	}
	if total > 0xFFFFFFFF {
		return nil, &Error{Code: ErrInvalid, Message: "the stages add up to too many winners"}
	}
	picks, err := DrawItems(items, uint32(total), opts...)
	if err != nil {
		return nil, err
	}
	out := make([]StageResult, len(stages))
	offset := uint32(0)
	for i, s := range stages {
		out[i] = StageResult{Name: s.Name, Count: s.Count, Winners: picks[offset : offset+s.Count : offset+s.Count]}
		offset += s.Count
	}
	return out, nil
}

var errClosed = errors.New("sortition: session is closed")
