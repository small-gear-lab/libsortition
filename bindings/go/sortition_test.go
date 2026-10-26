package sortition

import (
	"bytes"
	"encoding/json"
	"errors"
	"runtime"
	"slices"
	"strings"
	"sync"
	"testing"
)

var names = []string{"alice", "bob", "carol", "dave", "erin", "frank", "grace", "heidi"}

func code(t *testing.T, err error) int {
	t.Helper()
	var e *Error
	if !errors.As(err, &e) {
		t.Fatalf("want *Error, got %v", err)
	}
	return e.Code
}

func ids(p []Pick) []string {
	out := make([]string, len(p))
	for i, x := range p {
		out[i] = x.ID
	}
	return out
}

func TestQuickstartPrintsTheSameWinnersAsEveryOtherBinding(t *testing.T) {
	people := []string{"alice", "bob", "carol", "dave"}
	picks := must[[]Pick](t)(DrawItems(people, 2, WithSeed(SeedFromText("my seed"))))
	var got []string
	for _, p := range picks {
		got = append(got, people[p.Index])
	}
	if !slices.Equal(got, []string{"dave", "bob"}) {
		t.Errorf("winners = %v", got)
	}
	if c := SeedCommit("my seed"); c != "7efa9b064a86e6a99f9cff6c838e61c005ad9aac30e74efc50a0b9ec8270e7ad" {
		t.Errorf("commit = %s", c)
	}
}

func TestWinnersDoNotDependOnTheOrderOfTheList(t *testing.T) {
	seed := WithSeed(SeedFromText("order"))
	reversed := slices.Clone(names)
	slices.Reverse(reversed)
	a := must[[]Pick](t)(DrawItems(names, 5, seed))
	b := must[[]Pick](t)(DrawItems(reversed, 5, seed))
	if !slices.Equal(ids(a), ids(b)) {
		t.Error("ids differ")
	}
	for i := range a {
		if names[a[i].Index] != reversed[b[i].Index] {
			t.Errorf("pick %d: %s vs %s", i, names[a[i].Index], reversed[b[i].Index])
		}
	}
	if must[string](t)(ItemFingerprint(names)) != must[string](t)(ItemFingerprint(reversed)) {
		t.Error("fingerprints differ")
	}
}

func TestBytesAndStringsAreTheSameCandidates(t *testing.T) {
	a := must[[]string](t)(ItemIDs([][]byte{{0x61, 0x62}, {0x63}, {}}))
	b := must[[]string](t)(ItemIDs([]string{"ab", "c", ""}))
	if !slices.Equal(a, b) {
		t.Error("ids differ")
	}
	same := must[[]string](t)(ItemIDs([]string{"x", "x", "x"}))
	if len(slices.Compact(slices.Sorted(slices.Values(same)))) != 3 {
		t.Error("identical candidates must keep distinct ids")
	}
}

func TestErrorsCarryTheCode(t *testing.T) {
	_, err := Draw(3, 4)
	if code(t, err) != ErrInvalid || !strings.Contains(err.Error(), "code -1") {
		t.Errorf("Draw(3, 4): %v", err)
	}
	_, err = DrawItems([]string{"a"}, 2)
	if code(t, err) != ErrInvalid {
		t.Error(err)
	}
	_, err = LoadSession("{}")
	if code(t, err) != ErrState {
		t.Error(err)
	}
	_, err = SealKeyID("junk")
	if code(t, err) != ErrState {
		t.Error(err)
	}
}

func TestTextAndData(t *testing.T) {
	lines := must[[]string](t)(DrawText("  a \n\nb\nc\n", 3, WithSeed(1)))
	slices.Sort(lines)
	if !slices.Equal(lines, []string{"a", "b", "c"}) {
		t.Errorf("lines = %v", lines)
	}
	if got := must[[]string](t)(DrawText("", 0)); len(got) != 0 {
		t.Errorf("empty = %v", got)
	}
	out := must[string](t)(DrawData(JSON, `{"members": ["a", "b", "c"]}`, 2, "members", WithSeed(7)))
	var arr []string
	if err := json.Unmarshal([]byte(out), &arr); err != nil || len(arr) != 2 {
		t.Errorf("DrawData = %s (%v)", out, err)
	}
	_, err := DrawData(JSON, "{}", 1, "nope")
	var e *Error
	if !errors.As(err, &e) || e.Code != -5 || e.Message == "" {
		t.Errorf("want a detailed -5, got %v", err)
	}
}

func TestSessionSplitDrawsEqualOneDrawAndSurviveSaveAndLoad(t *testing.T) {
	a := must[*Session](t)(NewSession(200, WithSeed(9)))
	defer a.Close()
	got := must[[]uint32](t)(a.Draw(20))
	b := must[*Session](t)(LoadSession(must[string](t)(a.Save())))
	defer b.Close()
	got = append(got, must[[]uint32](t)(b.Draw(30))...)
	if want := must[[]uint32](t)(Draw(200, 50, WithSeed(9))); !slices.Equal(got, want) {
		t.Errorf("got %v, want %v", got, want)
	}
	if r, d := must[uint32](t)(b.Remaining()), must[uint32](t)(b.DrawnCount()); r != 150 || d != 50 {
		t.Errorf("remaining %d, drawn %d", r, d)
	}
	if id := must[string](t)(b.ID()); len(id) != 32 {
		t.Errorf("id = %q", id)
	}
	if rev := must[uint64](t)(b.Revision()); rev != 2 { // one draw before the save, one after the restore
		t.Errorf("revision = %d", rev)
	}
	_, err := b.Draw(151)
	if code(t, err) != ErrInvalid {
		t.Error(err)
	}
}

func TestIdempotencyKeysReplayAndConflict(t *testing.T) {
	s := must[*Session](t)(NewSession(50))
	defer s.Close()
	first := must[[]uint32](t)(s.DrawKeyed(3, "req-1"))
	state := must[string](t)(s.Save())
	if again := must[[]uint32](t)(s.DrawKeyed(3, "req-1")); !slices.Equal(first, again) {
		t.Error("replay differs")
	}
	if must[string](t)(s.Save()) != state {
		t.Error("a replay must not change the state")
	}
	_, err := s.DrawKeyed(4, "req-1")
	if code(t, err) != ErrConflict {
		t.Error(err)
	}
	_, err = s.DrawKeyed(1, "a\x00b")
	if code(t, err) != ErrInvalid {
		t.Error(err)
	}
	_, err = s.DrawKeyed(1, "")
	if code(t, err) != ErrInvalid {
		t.Error(err)
	}
}

func TestAnOldStateSentAgainGivesTheSameResult(t *testing.T) {
	a := must[*Session](t)(NewSession(100))
	defer a.Close()
	must[[]uint32](t)(a.DrawKeyed(4, "a"))
	stale := must[string](t)(a.Save())
	want := must[[]uint32](t)(a.DrawKeyed(6, "b"))
	old := must[*Session](t)(LoadSession(stale))
	defer old.Close()
	if got := must[[]uint32](t)(old.DrawKeyed(6, "b")); !slices.Equal(got, want) {
		t.Error("an old state must give the same result")
	}
	if must[string](t)(a.Save()) != must[string](t)(old.Save()) {
		t.Error("states differ")
	}
}

func TestSealingHidesTheStateAndDetectsChanges(t *testing.T) {
	key := bytes.Repeat([]byte{7}, 32)
	a := must[*Session](t)(NewSession(50))
	defer a.Close()
	must[[]uint32](t)(a.DrawKeyed(3, "r"))
	token := must[string](t)(a.Seal(key, 9))
	if !strings.HasPrefix(token, "sortition1.") || must[uint8](t)(SealKeyID(token)) != 9 {
		t.Errorf("token = %s", token)
	}
	b := must[*Session](t)(UnsealSession(token, key))
	defer b.Close()
	if must[string](t)(a.Save()) != must[string](t)(b.Save()) {
		t.Error("states differ")
	}
	last := token[len(token)-1]
	swap := byte('A')
	if last == 'A' {
		swap = 'B'
	}
	if _, err := UnsealSession(token[:len(token)-1]+string(swap), key); code(t, err) != ErrState {
		t.Error("a changed token must not open")
	}
	if _, err := UnsealSession(token, make([]byte, 32)); code(t, err) != ErrState {
		t.Error("a wrong key must not open")
	}
	if _, err := a.Seal(make([]byte, 31), 0); code(t, err) != ErrInvalid {
		t.Error("a short key must be refused")
	}
}

func TestItemSessionsResumeWithTheSameSetInAnyOrder(t *testing.T) {
	lot := must[*ItemSession](t)(NewItemSession(names, WithSeed(11)))
	defer lot.Close()
	got := must[[]Pick](t)(lot.DrawKeyed(3, "k1"))
	if again := must[[]Pick](t)(lot.DrawKeyed(3, "k1")); !slices.Equal(got, again) {
		t.Error("replay differs")
	}
	if rev := must[uint64](t)(lot.Revision()); rev != 1 {
		t.Errorf("revision = %d", rev)
	}
	state := must[string](t)(lot.Save())
	reversed := slices.Clone(names)
	slices.Reverse(reversed)
	again := must[*ItemSession](t)(LoadItemSession(state, reversed))
	defer again.Close()
	rest := must[[]Pick](t)(again.Draw(5))
	want := must[[]Pick](t)(DrawItems(names, 8, WithSeed(11)))
	if !slices.Equal(ids(want), append(ids(got), ids(rest)...)) {
		t.Error("a resumed draw must continue the same order")
	}
	reversedIDs := must[[]string](t)(ItemIDs(reversed))
	for _, p := range rest {
		if reversedIDs[p.Index] != p.ID {
			t.Errorf("index %d does not refer to the list that was passed again", p.Index)
		}
	}
	if _, err := LoadItemSession(state, names[1:]); code(t, err) != ErrState {
		t.Error("another set must be refused")
	}

	key := make([]byte, 32)
	token := must[string](t)(lot.Seal(key, 1))
	rotated := append(slices.Clone(names[2:]), names[:2]...)
	unsealed := must[*ItemSession](t)(UnsealItemSession(token, key, rotated))
	defer unsealed.Close()
	if !slices.Equal(ids(must[[]Pick](t)(lot.Draw(5))), ids(must[[]Pick](t)(unsealed.Draw(5)))) {
		t.Error("an unsealed session must continue the same order")
	}
}

func TestStagesCutOneDrawInOrderAndCheckTheirInput(t *testing.T) {
	seed := WithSeed(SeedFromText("stages"))
	got := must[[]StageResult](t)(DrawStages(names, []Stage{{"副賞", 3}, {"大賞", 1}}, seed))
	if got[0].Name != "副賞" || got[1].Name != "大賞" {
		t.Errorf("names = %v", got)
	}
	var all []Pick
	for _, r := range got {
		all = append(all, r.Winners...)
	}
	if !slices.Equal(all, must[[]Pick](t)(DrawItems(names, 4, seed))) {
		t.Error("stages must cut one draw")
	}
	var first []string
	for _, p := range got[0].Winners {
		first = append(first, names[p.Index])
	}
	if !slices.Equal(first, []string{"carol", "heidi", "bob"}) {
		t.Errorf("first stage = %v", first)
	}
	if r := must[[]StageResult](t)(DrawStages(names, nil)); len(r) != 0 {
		t.Error("no stages, no results")
	}
	if r := must[[]StageResult](t)(DrawStages(names, []Stage{{"none", 0}})); len(r[0].Winners) != 0 {
		t.Error("an empty stage has no winners")
	}
	_, err := DrawStages(names, []Stage{{"too many", 99}})
	if code(t, err) != ErrInvalid {
		t.Error(err)
	}
}

func TestAClosedSessionCannotBeUsed(t *testing.T) {
	s := must[*Session](t)(NewSession(5))
	if err := s.Close(); err != nil {
		t.Fatal(err)
	}
	if err := s.Close(); err != nil {
		t.Fatal(err)
	}
	if _, err := s.Draw(1); err == nil {
		t.Error("Draw on a closed session must fail")
	}
	if _, err := s.Remaining(); err == nil {
		t.Error("Remaining on a closed session must fail")
	}
}

func TestUsedFromManyGoroutines(t *testing.T) {
	var wg sync.WaitGroup
	for g := range 8 {
		wg.Go(func() {
			s, err := NewSession(100, WithSeed(uint64(g)))
			if err != nil {
				t.Error(err)
				return
			}
			defer s.Close()
			got, err := s.Draw(10)
			want, _ := Draw(100, 10, WithSeed(uint64(g)))
			if err != nil || !slices.Equal(got, want) {
				t.Errorf("goroutine %d: %v %v", g, got, err)
			}
		})
	}
	wg.Wait()
}

func TestForgottenSessionsAreFreed(t *testing.T) {
	for range 200 {
		if _, err := NewSession(1000); err != nil {
			t.Fatal(err)
		}
	}
	runtime.GC()
}

func TestTheCoreVersionIsReported(t *testing.T) {
	if v := Version(); v != "0.1.0" {
		t.Errorf("version = %q", v)
	}
}

func TestWithoutASeedTheResultsDiffer(t *testing.T) {
	a := must[[]uint32](t)(Draw(1_000_000, 8))
	b := must[[]uint32](t)(Draw(1_000_000, 8))
	if slices.Equal(a, b) {
		t.Error("two unseeded draws gave the same eight numbers")
	}
}
