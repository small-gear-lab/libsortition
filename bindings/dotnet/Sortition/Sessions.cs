using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;

namespace Sortition;

/// <summary>Owns a native session so that it is freed even if <c>Dispose</c> was forgotten.</summary>
internal sealed class SessionHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    public SessionHandle() : base(true) { }
    public static SessionHandle Own(nint p) { var h = new SessionHandle(); h.SetHandle(p); return h; }
    protected override bool ReleaseHandle() { Native.sortition_session_free(handle); return true; }
}

/// <summary>Owns a native candidate session.</summary>
internal sealed class ItemSessionHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    public ItemSessionHandle() : base(true) { }
    public static ItemSessionHandle Own(nint p) { var h = new ItemSessionHandle(); h.SetHandle(p); return h; }
    protected override bool ReleaseHandle() { Native.sortition_item_session_free(handle); return true; }
}

internal static unsafe class Args
{
    /// <summary>A 32-byte secret key for sealing.</summary>
    public static void CheckKey(byte[] key)
    {
        ArgumentNullException.ThrowIfNull(key);
        if (key.Length != 32) throw new ArgumentException("the key must be exactly 32 bytes", nameof(key));
    }

    /// <summary>An idempotency key as a C string (null means no key).</summary>
    public static byte[]? IdemKey(string? key)
    {
        if (key is null) return null;
        if (key.Contains('\0')) throw new ArgumentException("the key must not contain NUL", nameof(key));
        return Native.CString(key);
    }

    public static string ReadId(Func<nint, byte[], int> read, nint handle)
    {
        var buf = new byte[33];
        Native.Check(read(handle, buf));
        return Encoding.ASCII.GetString(buf, 0, 32);
    }
}

/// <summary>
/// A draw over 0..n-1 without replacement that continues where it stopped. Its state can be saved as JSON and restored, a
/// retried request with an idempotency key never draws twice, and <see cref="Seal"/> turns the state into a token that a party you do not
/// trust can keep without being able to read or change it. Not thread-safe.
/// </summary>
public sealed unsafe class Session : IDisposable
{
    private readonly SessionHandle _handle;

    private Session(SessionHandle handle) => _handle = handle;

    private nint H => _handle.IsClosed ? throw new ObjectDisposedException(nameof(Session)) : _handle.DangerousGetHandle();

    /// <summary>Creates a session over <paramref name="n"/> positions. Without a seed it uses a cryptographic random stream
    /// (ChaCha20 with a key from the operating system, saved in the state); with a seed it is repeatable.</summary>
    public Session(uint n, ulong? seed = null)
    {
        var s = seed.GetValueOrDefault();
        nint p;
        Native.Check(Native.sortition_session_new(n, seed.HasValue ? &s : null, &p));
        _handle = SessionHandle.Own(p);
    }

    /// <summary>Restores a session from the output of <see cref="Save"/>. An invalid state throws a <see cref="SortitionException"/> (code -8).</summary>
    public static Session Load(string state)
    {
        var bytes = Encoding.UTF8.GetBytes(state);
        nint p;
        fixed (byte* t = bytes)
            Native.Check(Native.sortition_session_load(t, (nuint)bytes.Length, &p));
        return new Session(SessionHandle.Own(p));
    }

    /// <summary>Restores a session from a token made by <see cref="Seal"/>. A malformed token, a wrong key and tampering
    /// all throw a <see cref="SortitionException"/> (code -8).</summary>
    public static Session Unseal(string token, byte[] key)
    {
        Args.CheckKey(key);
        var t = Encoding.UTF8.GetBytes(token);
        nint p;
        fixed (byte* tp = t)
        fixed (byte* kp = key)
            Native.Check(Native.sortition_session_unseal(tp, (nuint)t.Length, kp, &p));
        return new Session(SessionHandle.Own(p));
    }

    /// <summary>Draws the next <paramref name="k"/> numbers. Sending the same <paramref name="key"/> (an idempotency key, 1 to 128
    /// bytes) with the same <paramref name="k"/> again changes nothing and returns the earlier result; the same key with another
    /// <paramref name="k"/> throws a <see cref="SortitionException"/> (code -9). Fails when <paramref name="k"/> is more than what is left.</summary>
    public uint[] Draw(uint k, string? key = null)
    {
        var result = new uint[k];
        var ck = Args.IdemKey(key);
        fixed (uint* o = result)
        fixed (byte* kp = ck)
            Native.Check(Native.sortition_session_draw_keyed(H, k, ck is null ? null : kp, o, null));
        GC.KeepAlive(this);
        return result;
    }

    /// <summary>How many are not drawn yet.</summary>
    public uint Remaining { get { var v = Native.sortition_session_remaining(H); GC.KeepAlive(this); return v; } }

    /// <summary>How many were drawn.</summary>
    public uint DrawnCount { get { var v = Native.sortition_session_drawn_count(H); GC.KeepAlive(this); return v; } }

    /// <summary>The number of draws that changed the state. A store can update a record only when the revision is the one
    /// that was read, so that two updates made from the same state do not overwrite each other.</summary>
    public ulong Revision { get { var v = Native.sortition_session_revision(H); GC.KeepAlive(this); return v; } }

    /// <summary>The session id: 32 lowercase hex characters taken from the operating system when it was created.</summary>
    public string Id
    {
        get
        {
            var h = H;
            var id = Args.ReadId((handle, buf) => { fixed (byte* b = buf) return Native.sortition_session_id(handle, b); }, h);
            GC.KeepAlive(this);
            return id;
        }
    }

    /// <summary>The state as a JSON string. <b>It contains the internal state of the random generator</b>: whoever has it can
    /// predict the next winners, so give it only to parties you trust (see <see cref="Seal"/>).</summary>
    public string Save()
    {
        byte* json;
        Native.Check(Native.sortition_session_save(H, &json));
        GC.KeepAlive(this);
        return Native.TakeString(json);
    }

    /// <summary>The state sealed into a token (XChaCha20-Poly1305): only the holder of the 32-byte <paramref name="key"/> can
    /// open it, and any change makes it unopenable. <paramref name="keyId"/> labels the key for rotation.</summary>
    public string Seal(byte[] key, byte keyId = 0)
    {
        Args.CheckKey(key);
        byte* token;
        fixed (byte* kp = key)
            Native.Check(Native.sortition_session_seal(H, kp, keyId, &token));
        GC.KeepAlive(this);
        return Native.TakeString(token);
    }

    /// <summary>Frees the native session.</summary>
    public void Dispose() => _handle.Dispose();
}

/// <summary>
/// A draw over a fixed set of candidates (strings or byte arrays) without replacement, that continues where it stopped. The
/// winners do not depend on the order of the list. The state holds the IDs of the winners and the fingerprint of the set, and can be
/// restored only with the same set of candidates (in any order). Not thread-safe.
/// </summary>
public sealed unsafe class ItemSession : IDisposable
{
    private readonly ItemSessionHandle _handle;

    private ItemSession(ItemSessionHandle handle) => _handle = handle;

    private nint H => _handle.IsClosed ? throw new ObjectDisposedException(nameof(ItemSession)) : _handle.DangerousGetHandle();

    private static ItemSession Create(Lottery.Packed p, ulong? seed)
    {
        var s = seed.GetValueOrDefault();
        nint h;
        fixed (byte* d = p.Data)
        fixed (nuint* l = p.Lens)
            Native.Check(Native.sortition_item_session_new(d, l, (nuint)p.Lens.Length, seed.HasValue ? &s : null, &h));
        return new ItemSession(ItemSessionHandle.Own(h));
    }

    /// <summary>Creates a session over the candidates (strings are UTF-8).</summary>
    public ItemSession(IEnumerable<string> items, ulong? seed = null) : this(Create(Lottery.Pack(Lottery.Utf8(items)), seed)._handle) { }

    /// <summary>Creates a session over the candidates.</summary>
    public ItemSession(IEnumerable<byte[]> items, ulong? seed = null) : this(Create(Lottery.Pack(Lottery.Bytes(items)), seed)._handle) { }

    private static ItemSession LoadCore(string state, Lottery.Packed p)
    {
        var s = Encoding.UTF8.GetBytes(state);
        nint h;
        fixed (byte* sp = s)
        fixed (byte* d = p.Data)
        fixed (nuint* l = p.Lens)
            Native.Check(Native.sortition_item_session_load(sp, (nuint)s.Length, d, l, (nuint)p.Lens.Length, &h));
        return new ItemSession(ItemSessionHandle.Own(h));
    }

    /// <summary>Restores a session from the output of <see cref="Save"/> and the same set of candidates (the order may differ).
    /// A different set, or an invalid state, throws a <see cref="SortitionException"/> (code -8).</summary>
    public static ItemSession Load(string state, IEnumerable<string> items) => LoadCore(state, Lottery.Pack(Lottery.Utf8(items)));

    /// <summary>Restores a session from the output of <see cref="Save"/> and the same set of candidates.</summary>
    public static ItemSession Load(string state, IEnumerable<byte[]> items) => LoadCore(state, Lottery.Pack(Lottery.Bytes(items)));

    private static ItemSession UnsealCore(string token, byte[] key, Lottery.Packed p)
    {
        Args.CheckKey(key);
        var t = Encoding.UTF8.GetBytes(token);
        nint h;
        fixed (byte* tp = t)
        fixed (byte* kp = key)
        fixed (byte* d = p.Data)
        fixed (nuint* l = p.Lens)
            Native.Check(Native.sortition_item_session_unseal(tp, (nuint)t.Length, kp, d, l, (nuint)p.Lens.Length, &h));
        return new ItemSession(ItemSessionHandle.Own(h));
    }

    /// <summary>Restores a session from a token made by <see cref="Seal"/> and the same set of candidates (the order may differ).</summary>
    public static ItemSession Unseal(string token, byte[] key, IEnumerable<string> items) => UnsealCore(token, key, Lottery.Pack(Lottery.Utf8(items)));

    /// <summary>Restores a session from a token made by <see cref="Seal"/> and the same set of candidates.</summary>
    public static ItemSession Unseal(string token, byte[] key, IEnumerable<byte[]> items) => UnsealCore(token, key, Lottery.Pack(Lottery.Bytes(items)));

    /// <summary>Draws the next <paramref name="k"/> candidates. <see cref="Pick.Index"/> is the position in the list that was passed
    /// (after a restore: in the list passed again). The idempotency <paramref name="key"/> works as in <see cref="Session.Draw"/>.</summary>
    public Pick[] Draw(uint k, string? key = null)
    {
        var index = new uint[k];
        var ids = new byte[checked((int)(32L * k))];
        var ck = Args.IdemKey(key);
        fixed (uint* oi = index)
        fixed (byte* oid = ids)
        fixed (byte* kp = ck)
            Native.Check(Native.sortition_item_session_draw_keyed(H, k, ck is null ? null : kp, oi, oid, null));
        GC.KeepAlive(this);
        return Lottery.Picks(index, ids, (int)k);
    }

    /// <summary>How many candidates are not drawn yet.</summary>
    public uint Remaining { get { var v = Native.sortition_item_session_remaining(H); GC.KeepAlive(this); return v; } }

    /// <summary>How many candidates were drawn.</summary>
    public uint DrawnCount { get { var v = Native.sortition_item_session_drawn_count(H); GC.KeepAlive(this); return v; } }

    /// <summary>The number of draws that changed the state.</summary>
    public ulong Revision { get { var v = Native.sortition_item_session_revision(H); GC.KeepAlive(this); return v; } }

    /// <summary>The session id: 32 lowercase hex characters.</summary>
    public string Id
    {
        get
        {
            var h = H;
            var id = Args.ReadId((handle, buf) => { fixed (byte* b = buf) return Native.sortition_item_session_id(handle, b); }, h);
            GC.KeepAlive(this);
            return id;
        }
    }

    /// <summary>The state as a JSON string, with the IDs of the winners and the fingerprint of the set. It contains the internal
    /// state of the random generator, so keep it from parties you do not trust (see <see cref="Seal"/>).</summary>
    public string Save()
    {
        byte* json;
        Native.Check(Native.sortition_item_session_save(H, &json));
        GC.KeepAlive(this);
        return Native.TakeString(json);
    }

    /// <summary>The state sealed into a token. The arguments are the same as for <see cref="Session.Seal"/>.</summary>
    public string Seal(byte[] key, byte keyId = 0)
    {
        Args.CheckKey(key);
        byte* token;
        fixed (byte* kp = key)
            Native.Check(Native.sortition_item_session_seal(H, kp, keyId, &token));
        GC.KeepAlive(this);
        return Native.TakeString(token);
    }

    /// <summary>Frees the native session.</summary>
    public void Dispose() => _handle.Dispose();
}
