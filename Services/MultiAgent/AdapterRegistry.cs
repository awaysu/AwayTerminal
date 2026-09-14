namespace AwayTerminal.Services.MultiAgent;

internal static class AdapterRegistry
{
    public static readonly ICodingAgentAdapter[] All =
    {
        new ClaudeCodeAdapter(), new CodexAdapter(), new OpenCodeAdapter(), new GeminiCliAdapter()
    };

    public static ICodingAgentAdapter? ByKey(string? key) =>
        All.FirstOrDefault(a => string.Equals(a.Key, key, StringComparison.OrdinalIgnoreCase));
}
