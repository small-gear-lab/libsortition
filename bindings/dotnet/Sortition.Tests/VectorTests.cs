using System.Text.Json;

namespace Sortition.Tests;

/// <summary>The shared test vectors (docs/SPEC.md; test-vectors/vectors.json): every binding must give the same values.</summary>
public class VectorTests
{
    private static readonly JsonElement V = Load();

    private static JsonElement Load()
    {
        for (var dir = new DirectoryInfo(AppContext.BaseDirectory); dir != null; dir = dir.Parent)
        {
            var path = Path.Combine(dir.FullName, "test-vectors", "vectors.json");
            if (File.Exists(path)) return JsonDocument.Parse(File.ReadAllText(path)).RootElement.Clone();
        }
        throw new FileNotFoundException("test-vectors/vectors.json was not found above " + AppContext.BaseDirectory);
    }

    private static List<byte[]> Items(JsonElement set) =>
        set.GetProperty("items_hex").EnumerateArray().Select(h => Convert.FromHexString(h.GetString()!)).ToList();

    private static Dictionary<string, List<byte[]>> Sets() =>
        V.GetProperty("item_sets").EnumerateArray().ToDictionary(s => s.GetProperty("name").GetString()!, Items);

    private static List<string> Strings(JsonElement array) => array.EnumerateArray().Select(e => e.GetString()!).ToList();

    private static List<uint> Numbers(JsonElement array) => array.EnumerateArray().Select(e => e.GetUInt32()).ToList();

    private static void AssertPicks(JsonElement expected, IEnumerable<Pick> got) =>
        Assert.Equal(
            expected.EnumerateArray().Select(w => (w.GetProperty("index").GetUInt32(), w.GetProperty("id").GetString()!)).ToList(),
            got.Select(p => (p.Index, p.Id)).ToList());

    [Fact]
    public void SeedFromText()
    {
        foreach (var v in V.GetProperty("seed_from_text").EnumerateArray())
        {
            var text = v.GetProperty("text").GetString()!;
            Assert.Equal(ulong.Parse(v.GetProperty("seed").GetString()!), Lottery.SeedFromText(text));
            Assert.Equal(v.GetProperty("digest").GetString(), Lottery.SeedCommit(text));
        }
    }

    [Fact]
    public void Draw()
    {
        foreach (var v in V.GetProperty("draw").EnumerateArray())
        {
            var seed = ulong.Parse(v.GetProperty("seed").GetString()!);
            Assert.Equal(Numbers(v.GetProperty("result")), Lottery.Draw(v.GetProperty("n").GetUInt32(), v.GetProperty("k").GetUInt32(), seed).ToList());
        }
    }

    [Fact]
    public void ItemIdsAndFingerprints()
    {
        foreach (var v in V.GetProperty("item_sets").EnumerateArray())
        {
            Assert.Equal(Strings(v.GetProperty("ids")), Lottery.ItemIds(Items(v)).ToList());
            Assert.Equal(v.GetProperty("fingerprint").GetString(), Lottery.ItemFingerprint(Items(v)));
        }
    }

    [Fact]
    public void DrawItems()
    {
        var sets = Sets();
        foreach (var v in V.GetProperty("draw_items").EnumerateArray())
        {
            var seed = ulong.Parse(v.GetProperty("seed").GetString()!);
            AssertPicks(v.GetProperty("winners"), Lottery.DrawItems(sets[v.GetProperty("set").GetString()!], v.GetProperty("k").GetUInt32(), seed));
        }
    }

    [Fact]
    public void Stages()
    {
        var sets = Sets();
        foreach (var v in V.GetProperty("stages").EnumerateArray())
        {
            var plan = v.GetProperty("stages").EnumerateArray().Select(s => new Stage(s.GetProperty("name").GetString()!, s.GetProperty("count").GetUInt32())).ToList();
            var got = Lottery.DrawStages(sets[v.GetProperty("set").GetString()!], plan, Lottery.SeedFromText(v.GetProperty("seed_text").GetString()!));
            var expected = v.GetProperty("stages").EnumerateArray().ToList();
            Assert.Equal(expected.Count, got.Length);
            for (var i = 0; i < got.Length; i++)
            {
                Assert.Equal(expected[i].GetProperty("name").GetString(), got[i].Name);
                Assert.Equal(expected[i].GetProperty("count").GetUInt32(), got[i].Count);
                AssertPicks(expected[i].GetProperty("winners"), got[i].Winners);
            }
        }
    }

    [Fact]
    public void SessionRestoredFromAChaCha20State()
    {
        var v = V.GetProperty("chacha20_session");
        using var session = Session.Load(v.GetProperty("state").GetRawText());
        var draws = v.GetProperty("draws").EnumerateArray().Select(d => d.GetUInt32()).ToList();
        var expected = v.GetProperty("expected").EnumerateArray().Select(Numbers).ToList();
        for (var i = 0; i < draws.Count; i++) Assert.Equal(expected[i], session.Draw(draws[i]).ToList());
    }

    [Fact]
    public void SealedToken()
    {
        var v = V.GetProperty("seal");
        var token = v.GetProperty("token").GetString()!;
        Assert.Equal(v.GetProperty("key_id").GetByte(), Lottery.SealKeyId(token));
        using var session = Session.Unseal(token, Convert.FromHexString(v.GetProperty("key").GetString()!));
        var draws = v.GetProperty("draws").EnumerateArray().Select(d => d.GetUInt32()).ToList();
        var expected = v.GetProperty("expected").EnumerateArray().Select(Numbers).ToList();
        for (var i = 0; i < draws.Count; i++) Assert.Equal(expected[i], session.Draw(draws[i]).ToList());
    }
}
