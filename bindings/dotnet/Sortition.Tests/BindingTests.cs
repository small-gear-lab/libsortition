using System.Text.Json;

namespace Sortition.Tests;

public class BindingTests
{
    private static readonly string[] Names = ["alice", "bob", "carol", "dave", "erin", "frank", "grace", "heidi"];

    [Fact]
    public void Quickstart_prints_the_same_winners_as_every_other_binding()
    {
        string[] people = ["alice", "bob", "carol", "dave"];
        var picks = Lottery.DrawItems(people, 2, Lottery.SeedFromText("my seed"));
        Assert.Equal(["dave", "bob"], picks.Select(p => people[p.Index]));
        Assert.Equal("7efa9b064a86e6a99f9cff6c838e61c005ad9aac30e74efc50a0b9ec8270e7ad", Lottery.SeedCommit("my seed"));
    }

    [Fact]
    public void Winners_do_not_depend_on_the_order_of_the_list()
    {
        var seed = Lottery.SeedFromText("order");
        var a = Lottery.DrawItems(Names, 5, seed);
        var reversed = Names.Reverse().ToArray();
        var b = Lottery.DrawItems(reversed, 5, seed);
        Assert.Equal(a.Select(p => p.Id), b.Select(p => p.Id));
        Assert.Equal(a.Select(p => Names[p.Index]), b.Select(p => reversed[p.Index]));
        Assert.Equal(Lottery.ItemFingerprint(Names), Lottery.ItemFingerprint(reversed));
    }

    [Fact]
    public void Byte_arrays_and_strings_are_the_same_candidates()
    {
        byte[][] bytes = [[0x61, 0x62], [0x63], []];
        Assert.Equal(Lottery.ItemIds(new[] { "ab", "c", "" }), Lottery.ItemIds(bytes));
        Assert.Equal(3, Lottery.ItemIds(new[] { "x", "x", "x" }).Distinct().Count());
    }

    [Fact]
    public void Errors_carry_the_code()
    {
        var tooMany = Assert.Throws<SortitionException>(() => Lottery.Draw(3, 4));
        Assert.Equal(-1, tooMany.Code);
        Assert.IsAssignableFrom<ArgumentException>(tooMany);
        Assert.Equal(-1, Assert.Throws<SortitionException>(() => Lottery.DrawItems(["a"], 2)).Code);
        Assert.Equal(-8, Assert.Throws<SortitionException>(() => Session.Load("{}")).Code);
        Assert.Equal(-8, Assert.Throws<SortitionException>(() => Lottery.SealKeyId("junk")).Code);
    }

    [Fact]
    public void Text_and_data()
    {
        var lines = Lottery.DrawText("  a \n\nb\nc\n", 3, 1);
        Assert.Equal(["a", "b", "c"], lines.Order());
        Assert.Empty(Lottery.DrawText("", 0));
        var json = Lottery.DrawData("""{"members": ["a", "b", "c"]}""", 2, DataFormat.Json, "members", 7);
        Assert.Equal(2, JsonDocument.Parse(json).RootElement.GetArrayLength());
        Assert.Equal(-5, Assert.Throws<SortitionException>(() => Lottery.DrawData("{}", 1, DataFormat.Json, "nope")).Code);
    }

    [Fact]
    public void Session_split_draws_equal_one_draw_and_survive_save_and_load()
    {
        using var a = new Session(200, 9);
        var got = a.Draw(20).ToList();
        using var b = Session.Load(a.Save());
        got.AddRange(b.Draw(30));
        Assert.Equal(Lottery.Draw(200, 50, 9), got);
        Assert.Equal((150u, 50u), (b.Remaining, b.DrawnCount));
        Assert.Matches("^[0-9a-f]{32}$", b.Id);
        Assert.Equal(2UL, b.Revision); // one draw before the save, one after the restore
        Assert.Equal(-1, Assert.Throws<SortitionException>(() => b.Draw(151)).Code);
    }

    [Fact]
    public void Idempotency_keys_replay_and_conflict()
    {
        using var s = new Session(50);
        var first = s.Draw(3, "req-1");
        var state = s.Save();
        Assert.Equal(first, s.Draw(3, "req-1"));
        Assert.Equal(state, s.Save());
        Assert.Equal(-9, Assert.Throws<SortitionException>(() => s.Draw(4, "req-1")).Code);
        Assert.Throws<ArgumentException>(() => s.Draw(1, "a\0b"));
        Assert.Equal(-1, Assert.Throws<SortitionException>(() => s.Draw(1, "")).Code);
    }

    [Fact]
    public void An_old_state_sent_again_gives_the_same_result()
    {
        using var a = new Session(100);
        a.Draw(4, "a");
        var stale = a.Save();
        var want = a.Draw(6, "b");
        using var old = Session.Load(stale);
        Assert.Equal(want, old.Draw(6, "b"));
        Assert.Equal(a.Save(), old.Save());
    }

    [Fact]
    public void Sealing_hides_the_state_and_detects_changes()
    {
        var key = new byte[32];
        Array.Fill(key, (byte)7);
        using var a = new Session(50);
        a.Draw(3, "r");
        var token = a.Seal(key, 9);
        Assert.StartsWith("sortition1.", token);
        Assert.Equal(9, Lottery.SealKeyId(token));
        using var b = Session.Unseal(token, key);
        Assert.Equal(a.Save(), b.Save());
        var tampered = token[..^1] + (token[^1] == 'A' ? 'B' : 'A');
        Assert.Equal(-8, Assert.Throws<SortitionException>(() => Session.Unseal(tampered, key)).Code);
        Assert.Equal(-8, Assert.Throws<SortitionException>(() => Session.Unseal(token, new byte[32])).Code);
        Assert.Throws<ArgumentException>(() => a.Seal(new byte[31]));
    }

    [Fact]
    public void Item_sessions_resume_with_the_same_set_in_any_order()
    {
        using var lot = new ItemSession(Names, 11);
        var got = lot.Draw(3, "k1");
        Assert.Equal(got, lot.Draw(3, "k1"));
        Assert.Equal(1UL, lot.Revision);
        var state = lot.Save();
        var reversed = Names.Reverse().ToArray();
        using var again = ItemSession.Load(state, reversed);
        var rest = again.Draw(5);
        Assert.Equal(Lottery.DrawItems(Names, 8, 11).Select(p => p.Id), got.Concat(rest).Select(p => p.Id));
        Assert.All(rest, p => Assert.Equal(Lottery.ItemIds(reversed)[p.Index], p.Id));
        Assert.Equal(-8, Assert.Throws<SortitionException>(() => ItemSession.Load(state, Names[1..])).Code);

        var key = new byte[32];
        var token = lot.Seal(key, 1);
        using var unsealed = ItemSession.Unseal(token, key, Names.Skip(2).Concat(Names.Take(2)));
        Assert.Equal(lot.Draw(5).Select(p => p.Id), unsealed.Draw(5).Select(p => p.Id));
    }

    [Fact]
    public void Stages_cut_one_draw_in_order_and_check_their_input()
    {
        var seed = Lottery.SeedFromText("stages");
        var got = Lottery.DrawStages(Names, [new Stage("副賞", 3), new Stage("大賞", 1)], seed);
        Assert.Equal(["副賞", "大賞"], got.Select(r => r.Name));
        Assert.Equal(Lottery.DrawItems(Names, 4, seed), got.SelectMany(r => r.Winners));
        Assert.Equal(["carol", "heidi", "bob"], got[0].Winners.Select(p => Names[p.Index]));
        Assert.Empty(Lottery.DrawStages(Names, []));
        Assert.Empty(Lottery.DrawStages(Names, [new Stage("none", 0)])[0].Winners);
        Assert.Equal(-1, Assert.Throws<SortitionException>(() => Lottery.DrawStages(Names, [new Stage("too many", 99)])).Code);
    }

    [Fact]
    public void A_disposed_session_cannot_be_used()
    {
        var s = new Session(5);
        s.Dispose();
        s.Dispose();
        Assert.Throws<ObjectDisposedException>(() => s.Draw(1));
        Assert.Throws<ObjectDisposedException>(() => s.Remaining);
    }

    [Fact]
    public void The_native_version_is_reported()
    {
        Assert.Equal("0.1.1", Lottery.Version);
    }
}
