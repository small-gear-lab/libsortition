package sortition

import (
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"slices"
	"strconv"
	"testing"
)

// The shared test vectors (docs/SPEC.md; test-vectors/vectors.json): every binding must give the same values.
type pickJSON struct {
	Index uint32 `json:"index"`
	ID    string `json:"id"`
}

type vectors struct {
	SeedFromText []struct {
		Text, Digest, Seed string
	} `json:"seed_from_text"`
	Draw []struct {
		N, K   uint32
		Seed   string
		Result []uint32
	} `json:"draw"`
	ItemSets []struct {
		Name        string
		ItemsHex    []string `json:"items_hex"`
		IDs         []string `json:"ids"`
		Fingerprint string
	} `json:"item_sets"`
	DrawItems []struct {
		Set     string
		K       uint32
		Seed    string
		Winners []pickJSON
	} `json:"draw_items"`
	Stages []struct {
		Set      string
		SeedText string `json:"seed_text"`
		Stages   []struct {
			Name    string
			Count   uint32
			Winners []pickJSON
		}
	} `json:"stages"`
	ChaCha20Session struct {
		State    json.RawMessage
		Draws    []uint32
		Expected [][]uint32
	} `json:"chacha20_session"`
	Seal struct {
		Key      string
		KeyID    uint8 `json:"key_id"`
		Token    string
		Draws    []uint32
		Expected [][]uint32
	}
}

func loadVectors(t *testing.T) *vectors {
	t.Helper()
	dir, _ := os.Getwd()
	for {
		raw, err := os.ReadFile(filepath.Join(dir, "test-vectors", "vectors.json"))
		if err == nil {
			var v vectors
			if err := json.Unmarshal(raw, &v); err != nil {
				t.Fatal(err)
			}
			return &v
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			t.Fatal("test-vectors/vectors.json was not found above the working directory")
		}
		dir = parent
	}
}

func seedOf(t *testing.T, s string) uint64 {
	t.Helper()
	v, err := strconv.ParseUint(s, 10, 64)
	if err != nil {
		t.Fatal(err)
	}
	return v
}

func must[T any](t *testing.T) func(T, error) T {
	return func(v T, err error) T {
		t.Helper()
		if err != nil {
			t.Fatal(err)
		}
		return v
	}
}

func hexItems(t *testing.T, items []string) [][]byte {
	t.Helper()
	out := make([][]byte, len(items))
	for i, h := range items {
		b, err := hex.DecodeString(h)
		if err != nil {
			t.Fatal(err)
		}
		out[i] = b
	}
	return out
}

func picks(in []pickJSON) []Pick {
	out := make([]Pick, len(in))
	for i, p := range in {
		out[i] = Pick{p.Index, p.ID}
	}
	return out
}

func TestVectorSeedFromText(t *testing.T) {
	for _, v := range loadVectors(t).SeedFromText {
		if got := SeedFromText(v.Text); got != seedOf(t, v.Seed) {
			t.Errorf("SeedFromText(%q) = %d, want %s", v.Text, got, v.Seed)
		}
		if got := SeedCommit(v.Text); got != v.Digest {
			t.Errorf("SeedCommit(%q) = %s, want %s", v.Text, got, v.Digest)
		}
	}
}

func TestVectorDraw(t *testing.T) {
	for _, v := range loadVectors(t).Draw {
		got := must[[]uint32](t)(Draw(v.N, v.K, WithSeed(seedOf(t, v.Seed))))
		if !slices.Equal(got, v.Result) {
			t.Errorf("Draw(%d, %d, %s) = %v, want %v", v.N, v.K, v.Seed, got, v.Result)
		}
	}
}

func TestVectorItemIDsAndFingerprints(t *testing.T) {
	for _, v := range loadVectors(t).ItemSets {
		items := hexItems(t, v.ItemsHex)
		if got := must[[]string](t)(ItemIDs(items)); !slices.Equal(got, v.IDs) {
			t.Errorf("%s: ids = %v, want %v", v.Name, got, v.IDs)
		}
		if got := must[string](t)(ItemFingerprint(items)); got != v.Fingerprint {
			t.Errorf("%s: fingerprint = %s, want %s", v.Name, got, v.Fingerprint)
		}
	}
}

func sets(t *testing.T, v *vectors) map[string][][]byte {
	out := map[string][][]byte{}
	for _, s := range v.ItemSets {
		out[s.Name] = hexItems(t, s.ItemsHex)
	}
	return out
}

func TestVectorDrawItems(t *testing.T) {
	v := loadVectors(t)
	all := sets(t, v)
	for _, c := range v.DrawItems {
		got := must[[]Pick](t)(DrawItems(all[c.Set], c.K, WithSeed(seedOf(t, c.Seed))))
		if !slices.Equal(got, picks(c.Winners)) {
			t.Errorf("%s k=%d seed=%s: got %v, want %v", c.Set, c.K, c.Seed, got, c.Winners)
		}
	}
}

func TestVectorStages(t *testing.T) {
	v := loadVectors(t)
	all := sets(t, v)
	for _, c := range v.Stages {
		plan := make([]Stage, len(c.Stages))
		for i, s := range c.Stages {
			plan[i] = Stage{s.Name, s.Count}
		}
		got := must[[]StageResult](t)(DrawStages(all[c.Set], plan, WithSeed(SeedFromText(c.SeedText))))
		if len(got) != len(c.Stages) {
			t.Fatalf("got %d stages, want %d", len(got), len(c.Stages))
		}
		for i, s := range c.Stages {
			if got[i].Name != s.Name || got[i].Count != s.Count || !slices.Equal(got[i].Winners, picks(s.Winners)) {
				t.Errorf("stage %q: got %+v, want %+v", s.Name, got[i], s)
			}
		}
	}
}

func drawAll(t *testing.T, s *Session, draws []uint32, expected [][]uint32) {
	t.Helper()
	for i, k := range draws {
		if got := must[[]uint32](t)(s.Draw(k)); !slices.Equal(got, expected[i]) {
			t.Errorf("draw %d: got %v, want %v", i, got, expected[i])
		}
	}
}

func TestVectorSessionRestoredFromAChaCha20State(t *testing.T) {
	v := loadVectors(t).ChaCha20Session
	s := must[*Session](t)(LoadSession(string(v.State)))
	defer s.Close()
	drawAll(t, s, v.Draws, v.Expected)
}

func TestVectorSealedToken(t *testing.T) {
	v := loadVectors(t).Seal
	if got := must[uint8](t)(SealKeyID(v.Token)); got != v.KeyID {
		t.Errorf("key id = %d, want %d", got, v.KeyID)
	}
	key, _ := hex.DecodeString(v.Key)
	s := must[*Session](t)(UnsealSession(v.Token, key))
	defer s.Close()
	drawAll(t, s, v.Draws, v.Expected)
}
