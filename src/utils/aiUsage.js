// Cache rates are weighted by input tokens, only across calls reporting cache
// usage. Missing usage is unknown, never an invented zero or a token estimate.
export function conversationUsage(conversation) {
  const stats = conversation?.usageStats;
  if (!stats) return { totalTokens: 0, partial: !!conversation?.messages?.length, cacheRate: null, cachePartial: false };
  const calls = stats.reportedCalls + stats.unreportedCalls;
  return {
    totalTokens: stats.totalTokens,
    partial: stats.partialHistory || stats.unreportedCalls > 0,
    cacheRate: stats.cacheInputTokens > 0 && stats.cacheReportedCalls > 0
      ? stats.cachedInputTokens / stats.cacheInputTokens * 100 : null,
    cachePartial: stats.cacheReportedCalls < calls || stats.partialHistory,
  };
}
