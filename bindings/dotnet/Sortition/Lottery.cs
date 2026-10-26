using System.Text;

namespace Sortition;

/// <summary>The format of structured data for <see cref="Lottery.DrawData"/>.</summary>
public enum DataFormat
{
    /// <summary>JSON.</summary>
    Json = 0,
    /// <summary>YAML (aliases, custom tags and multiple documents are rejected).</summary>
    Yaml = 1,
    /// <summary>TOML (it needs a path, because its root is a table).</summary>
    Toml = 2,
}

/// <summary>A winner: its position in the list that was passed, and its ID (32 bytes of SHA-256, as 64 lowercase hex characters).</summary>
public readonly record struct Pick(uint Index, string Id);

/// <summary>One prize stage of a plan: a name and how many winners.</summary>
public readonly record struct Stage(string Name, uint Count);

/// <summary>The winners of one prize stage.</summary>
public sealed record StageResult(string Name, uint Count, IReadOnlyList<Pick> Winners);

/// <summary>
/// Fair, repeatable and verifiable random lotteries. Use the candidate functions (<see cref="DrawItems(IEnumerable{string}, uint, ulong?)"/>,
/// <see cref="DrawStages(IEnumerable{string}, IEnumerable{Stage}, ulong?)"/>, <see cref="ItemSession"/>) when the winners must not
/// depend on the order of your list: each candidate gets an ID from its bytes, and the draw works on the IDs in sorted order.
/// <see cref="Draw"/>, <see cref="DrawText"/> and <see cref="DrawData"/> return positions in the list as given.
/// </summary>
public static unsafe class Lottery
{
    /// <summary>The version of the native library.</summary>
    public static string Version => System.Runtime.InteropServices.Marshal.PtrToStringUTF8((nint)Native.sortition_version()) ?? "";

    private static (ulong Seed, byte[] Digest) SeedInfo(ReadOnlySpan<byte> text)
    {
        ulong seed;
        var digest = new byte[32];
        fixed (byte* t = text)
        fixed (byte* d = digest)
            Native.Check(Native.sortition_seed_from_text(t, (nuint)text.Length, &seed, d));
        return (seed, digest);
    }

    /// <summary>The seed of a seed string: the first 8 bytes of its SHA-256, read as a little-endian number.</summary>
    public static ulong SeedFromText(string text) => SeedInfo(Encoding.UTF8.GetBytes(text)).Seed;

    /// <summary>The seed of a seed string given as bytes.</summary>
    public static ulong SeedFromText(ReadOnlySpan<byte> text) => SeedInfo(text).Seed;

    /// <summary>The SHA-256 of a seed string (64 lowercase hex characters): the commitment to publish before the draw.</summary>
    public static string SeedCommit(string text) => Convert.ToHexStringLower(SeedInfo(Encoding.UTF8.GetBytes(text)).Digest);

    /// <summary>Draws <paramref name="k"/> distinct numbers from 0..n-1 in the order drawn. With a seed the result is repeatable.</summary>
    public static uint[] Draw(uint n, uint k, ulong? seed = null)
    {
        var result = new uint[k];
        var s = seed.GetValueOrDefault();
        fixed (uint* o = result)
            Native.Check(Native.sortition_draw(n, k, seed.HasValue ? &s : null, o));
        return result;
    }

    /// <summary>Draws <paramref name="k"/> lines from text (one candidate per line; each line is trimmed and empty lines are
    /// skipped; identical lines are separate candidates) and returns the chosen lines.</summary>
    public static string[] DrawText(string text, uint k, ulong? seed = null)
    {
        var input = Encoding.UTF8.GetBytes(text);
        var output = new byte[input.Length + 1];
        var s = seed.GetValueOrDefault();
        long written;
        fixed (byte* t = input)
        fixed (byte* o = output)
            written = Native.sortition_draw_text(t, (nuint)input.Length, k, seed.HasValue ? &s : null, o, (nuint)output.Length);
        Native.Check((int)Math.Max(written, int.MinValue));
        return written == 0 ? [] : Encoding.UTF8.GetString(output, 0, (int)written).Split('\n');
    }

    /// <summary>Draws <paramref name="k"/> elements from an array inside JSON, YAML or TOML data and returns the chosen
    /// elements as a JSON array string (parse it with your JSON library). <paramref name="path"/> says where the array
    /// is ("a.b.0"); empty means the root.</summary>
    public static string DrawData(string text, uint k, DataFormat format, string path = "", ulong? seed = null)
    {
        var input = Encoding.UTF8.GetBytes(text);
        var cpath = Native.CString(path);
        var s = seed.GetValueOrDefault();
        byte* json = null;
        int rc;
        fixed (byte* t = input)
        fixed (byte* p = cpath)
            rc = Native.sortition_draw_data((int)format, t, (nuint)input.Length, p, k, seed.HasValue ? &s : null, &json);
        Native.Check(rc);
        return Native.TakeString(json);
    }

    // ---- candidates (arbitrary bytes) ----

    internal readonly record struct Packed(byte[] Data, nuint[] Lens);

    internal static Packed Pack(IReadOnlyList<ReadOnlyMemory<byte>> items)
    {
        var lens = new nuint[items.Count];
        long total = 0;
        for (var i = 0; i < items.Count; i++)
        {
            lens[i] = (nuint)items[i].Length;
            total += items[i].Length;
        }
        var data = new byte[checked((int)total)];
        var at = 0;
        foreach (var item in items)
        {
            item.Span.CopyTo(data.AsSpan(at));
            at += item.Length;
        }
        return new Packed(data, lens);
    }

    internal static IReadOnlyList<ReadOnlyMemory<byte>> Utf8(IEnumerable<string> items) =>
        items.Select(s => (ReadOnlyMemory<byte>)Encoding.UTF8.GetBytes(s)).ToList();

    internal static IReadOnlyList<ReadOnlyMemory<byte>> Bytes(IEnumerable<byte[]> items) =>
        items.Select(b => (ReadOnlyMemory<byte>)b).ToList();

    private static string[] IdsCore(Packed p)
    {
        var ids = new byte[checked(32 * p.Lens.Length)];
        fixed (byte* d = p.Data)
        fixed (nuint* l = p.Lens)
        fixed (byte* o = ids)
            Native.Check(Native.sortition_item_ids(d, l, (nuint)p.Lens.Length, o));
        return Hex(ids, 0, p.Lens.Length);
    }

    internal static string[] Hex(byte[] ids, int first, int count) =>
        Enumerable.Range(first, count).Select(i => Convert.ToHexStringLower(ids.AsSpan(32 * i, 32))).ToArray();

    /// <summary>The ID of each candidate (strings are UTF-8), in the order of the input. An ID is
    /// <c>SHA-256("libsortition/candidate/v1" || 0x00 || bytes || LE64(occurrence))</c>, so it does not depend on the
    /// position, and identical entries stay separate candidates.</summary>
    public static string[] ItemIds(IEnumerable<string> items) => IdsCore(Pack(Utf8(items)));

    /// <summary>The ID of each candidate, in the order of the input.</summary>
    public static string[] ItemIds(IEnumerable<byte[]> items) => IdsCore(Pack(Bytes(items)));

    private static string FingerprintCore(Packed p)
    {
        var fp = new byte[32];
        fixed (byte* d = p.Data)
        fixed (nuint* l = p.Lens)
        fixed (byte* o = fp)
            Native.Check(Native.sortition_item_fingerprint(d, l, (nuint)p.Lens.Length, o));
        return Convert.ToHexStringLower(fp);
    }

    /// <summary>The fingerprint of a set of candidates (64 lowercase hex characters). It does not depend on their order:
    /// publish it with the commitment before the draw.</summary>
    public static string ItemFingerprint(IEnumerable<string> items) => FingerprintCore(Pack(Utf8(items)));

    /// <summary>The fingerprint of a set of candidates.</summary>
    public static string ItemFingerprint(IEnumerable<byte[]> items) => FingerprintCore(Pack(Bytes(items)));

    internal static Pick[] DrawItemsCore(Packed p, uint k, ulong? seed)
    {
        var index = new uint[k];
        var ids = new byte[checked((int)(32L * k))];
        var s = seed.GetValueOrDefault();
        fixed (byte* d = p.Data)
        fixed (nuint* l = p.Lens)
        fixed (uint* oi = index)
        fixed (byte* oid = ids)
            Native.Check(Native.sortition_items_draw(d, l, (nuint)p.Lens.Length, k, seed.HasValue ? &s : null, oi, oid));
        return Picks(index, ids, (int)k);
    }

    internal static Pick[] Picks(uint[] index, byte[] ids, int k)
    {
        var hex = Hex(ids, 0, k);
        return Enumerable.Range(0, k).Select(i => new Pick(index[i], hex[i])).ToArray();
    }

    /// <summary>Draws <paramref name="k"/> candidates (strings are UTF-8) without replacement. The same set of candidates
    /// and the same seed give the same winners whatever the order of the list.</summary>
    public static Pick[] DrawItems(IEnumerable<string> items, uint k, ulong? seed = null) =>
        DrawItemsCore(Pack(Utf8(items)), k, seed);

    /// <summary>Draws <paramref name="k"/> candidates without replacement.</summary>
    public static Pick[] DrawItems(IEnumerable<byte[]> items, uint k, ulong? seed = null) =>
        DrawItemsCore(Pack(Bytes(items)), k, seed);

    private static StageResult[] StagesCore(Packed p, IEnumerable<Stage> stages, ulong? seed)
    {
        var plan = stages.ToList();
        ulong total = 0;
        foreach (var stage in plan)
        {
            ArgumentNullException.ThrowIfNull(stage.Name);
            total += stage.Count;
        }
        if (total > uint.MaxValue) throw new ArgumentException("the stages need too many winners", nameof(stages));
        var picks = DrawItemsCore(p, (uint)total, seed);
        var result = new List<StageResult>();
        var at = 0;
        foreach (var stage in plan)
        {
            result.Add(new StageResult(stage.Name, stage.Count, picks[at..(at + (int)stage.Count)]));
            at += (int)stage.Count;
        }
        return result.ToArray();
    }

    /// <summary>Draws prize stages in order (nobody wins twice): the total number of winners is drawn at once and cut up in
    /// order. Fails with a <see cref="SortitionException"/> when the stages need more winners than there are candidates.</summary>
    public static StageResult[] DrawStages(IEnumerable<string> items, IEnumerable<Stage> stages, ulong? seed = null) =>
        StagesCore(Pack(Utf8(items)), stages, seed);

    /// <summary>Draws prize stages in order (nobody wins twice).</summary>
    public static StageResult[] DrawStages(IEnumerable<byte[]> items, IEnumerable<Stage> stages, ulong? seed = null) =>
        StagesCore(Pack(Bytes(items)), stages, seed);

    /// <summary>The key id written in a sealed token, to choose the key before opening it.</summary>
    public static byte SealKeyId(string token)
    {
        var t = Encoding.UTF8.GetBytes(token);
        byte id;
        fixed (byte* p = t)
            Native.Check(Native.sortition_seal_key_id(p, (nuint)t.Length, &id));
        return id;
    }
}
