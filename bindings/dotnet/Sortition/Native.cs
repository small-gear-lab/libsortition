using System.Runtime.InteropServices;
using System.Text;

namespace Sortition;

/// <summary>The C ABI of libsortition (<c>core/include/sortition.h</c>).</summary>
internal static unsafe partial class Native
{
    private const string Lib = "sortition";

    static Native() => NativeLibrary.SetDllImportResolver(typeof(Native).Assembly, Resolve);

    /// <summary>Finds the shared library: <c>LIBSORTITION_PATH</c>, next to the program, in a <c>target/release</c>
    /// above the program or the working directory, and then the usual places of the system.</summary>
    private static IntPtr Resolve(string name, System.Reflection.Assembly assembly, DllImportSearchPath? searchPath)
    {
        if (name != Lib) return IntPtr.Zero;
        var file = OperatingSystem.IsWindows() ? "sortition.dll" : OperatingSystem.IsMacOS() ? "libsortition.dylib" : "libsortition.so";

        var explicitPath = Environment.GetEnvironmentVariable("LIBSORTITION_PATH");
        if (!string.IsNullOrEmpty(explicitPath) && NativeLibrary.TryLoad(explicitPath, out var fromEnv)) return fromEnv;

        var beside = Path.Combine(AppContext.BaseDirectory, file);
        if (File.Exists(beside) && NativeLibrary.TryLoad(beside, out var fromBeside)) return fromBeside;

        foreach (var start in new[] { AppContext.BaseDirectory, Directory.GetCurrentDirectory() })
        {
            for (var dir = new DirectoryInfo(start); dir != null; dir = dir.Parent)
            {
                var candidate = Path.Combine(dir.FullName, "target", "release", file);
                if (File.Exists(candidate) && NativeLibrary.TryLoad(candidate, out var fromTarget)) return fromTarget;
            }
        }
        return NativeLibrary.TryLoad(Lib, assembly, searchPath, out var fallback) ? fallback : IntPtr.Zero;
    }

    [LibraryImport(Lib)] internal static partial int sortition_draw(uint n, uint k, ulong* seed, uint* output);
    [LibraryImport(Lib)] internal static partial long sortition_draw_text(byte* text, nuint len, uint k, ulong* seed, byte* output, nuint outCap);
    [LibraryImport(Lib)] internal static partial int sortition_draw_data(int format, byte* text, nuint len, byte* path, uint k, ulong* seed, byte** output);
    [LibraryImport(Lib)] internal static partial void sortition_free(byte* s);
    [LibraryImport(Lib)] internal static partial byte* sortition_last_error();
    [LibraryImport(Lib)] internal static partial byte* sortition_version();
    [LibraryImport(Lib)] internal static partial int sortition_seed_from_text(byte* text, nuint len, ulong* outSeed, byte* outDigest);
    [LibraryImport(Lib)] internal static partial int sortition_seal_key_id(byte* token, nuint len, byte* outKeyId);

    [LibraryImport(Lib)] internal static partial int sortition_session_new(uint n, ulong* seed, nint* output);
    [LibraryImport(Lib)] internal static partial int sortition_session_draw_keyed(nint s, uint k, byte* key, uint* output, int* replayed);
    [LibraryImport(Lib)] internal static partial uint sortition_session_remaining(nint s);
    [LibraryImport(Lib)] internal static partial uint sortition_session_drawn_count(nint s);
    [LibraryImport(Lib)] internal static partial int sortition_session_save(nint s, byte** output);
    [LibraryImport(Lib)] internal static partial int sortition_session_load(byte* text, nuint len, nint* output);
    [LibraryImport(Lib)] internal static partial ulong sortition_session_revision(nint s);
    [LibraryImport(Lib)] internal static partial int sortition_session_id(nint s, byte* output);
    [LibraryImport(Lib)] internal static partial int sortition_session_seal(nint s, byte* key, byte keyId, byte** output);
    [LibraryImport(Lib)] internal static partial int sortition_session_unseal(byte* token, nuint len, byte* key, nint* output);
    [LibraryImport(Lib)] internal static partial void sortition_session_free(nint s);

    [LibraryImport(Lib)] internal static partial int sortition_item_ids(byte* data, nuint* lens, nuint count, byte* outIds);
    [LibraryImport(Lib)] internal static partial int sortition_item_fingerprint(byte* data, nuint* lens, nuint count, byte* outFingerprint);
    [LibraryImport(Lib)] internal static partial int sortition_items_draw(byte* data, nuint* lens, nuint count, uint k, ulong* seed, uint* outIndex, byte* outIds);

    [LibraryImport(Lib)] internal static partial int sortition_item_session_new(byte* data, nuint* lens, nuint count, ulong* seed, nint* output);
    [LibraryImport(Lib)] internal static partial int sortition_item_session_draw_keyed(nint s, uint k, byte* key, uint* outIndex, byte* outIds, int* replayed);
    [LibraryImport(Lib)] internal static partial uint sortition_item_session_remaining(nint s);
    [LibraryImport(Lib)] internal static partial uint sortition_item_session_drawn_count(nint s);
    [LibraryImport(Lib)] internal static partial int sortition_item_session_save(nint s, byte** output);
    [LibraryImport(Lib)] internal static partial int sortition_item_session_load(byte* state, nuint stateLen, byte* data, nuint* lens, nuint count, nint* output);
    [LibraryImport(Lib)] internal static partial ulong sortition_item_session_revision(nint s);
    [LibraryImport(Lib)] internal static partial int sortition_item_session_id(nint s, byte* output);
    [LibraryImport(Lib)] internal static partial int sortition_item_session_seal(nint s, byte* key, byte keyId, byte** output);
    [LibraryImport(Lib)] internal static partial int sortition_item_session_unseal(byte* token, nuint len, byte* key, byte* data, nuint* lens, nuint count, nint* output);
    [LibraryImport(Lib)] internal static partial void sortition_item_session_free(nint s);

    private static readonly Dictionary<int, string> Messages = new()
    {
        [-1] = "invalid argument (k is more than the number of candidates, or an argument is out of range)",
        [-2] = "the operating system's random source failed",
        [-3] = "text is not valid UTF-8",
        [-4] = "parse error",
        [-5] = "path not found or not an array",
        [-6] = "unsupported format",
        [-7] = "internal error",
        [-8] = "invalid session state",
        [-9] = "idempotency key was already used with a different request",
    };

    /// <summary>Throws a <see cref="SortitionException"/> for a failure code. The detail that the library keeps for the
    /// thread is used when there is one (the failures of the functions that parse text, states and tokens).</summary>
    internal static void Check(int rc)
    {
        if (rc >= 0) return;
        var detail = LastError();
        var message = detail is { Length: > 0 } && rc is -4 or -5 or -6 or -8 ? detail : Messages.GetValueOrDefault(rc, $"error {rc}");
        throw new SortitionException(rc, message);
    }

    internal static string? LastError()
    {
        var p = sortition_last_error();
        return p == null ? null : Marshal.PtrToStringUTF8((nint)p);
    }

    /// <summary>Reads a string that the library allocated (UTF-8, NUL-terminated) and frees it.</summary>
    internal static string TakeString(byte* p)
    {
        try { return Marshal.PtrToStringUTF8((nint)p) ?? ""; }
        finally { sortition_free(p); }
    }

    /// <summary>UTF-8 bytes of a string with a NUL at the end, for the functions that take a C string.</summary>
    internal static byte[] CString(string s)
    {
        var bytes = new byte[Encoding.UTF8.GetByteCount(s) + 1];
        Encoding.UTF8.GetBytes(s, bytes);
        return bytes;
    }
}
