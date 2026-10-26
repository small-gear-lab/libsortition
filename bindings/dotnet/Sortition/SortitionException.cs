namespace Sortition;

/// <summary>An error reported by libsortition. <see cref="Code"/> is the <c>SORTITION_ERR_*</c> value (a negative number).</summary>
public sealed class SortitionException : ArgumentException
{
    /// <summary>The error code: -1 invalid argument, -2 random source failed, -3 not UTF-8, -4 parse error,
    /// -5 path not found, -6 unsupported format, -7 internal error, -8 invalid saved state or sealed token,
    /// -9 idempotency key reused with a different request.</summary>
    public int Code { get; }

    internal SortitionException(int code, string message) : base(message) => Code = code;
}
