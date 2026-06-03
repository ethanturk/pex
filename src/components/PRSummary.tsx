import { useState } from "preact/hooks";
import type { PullRequest, PRCheck, Reviewer, VoteHistoryEntry, CommentThread } from "@/lib/api";
import { prFiles } from "@/lib/signals";
import { getPrCheckRollup } from "@/lib/prChecks";

type Provider = "ado" | "github" | undefined;

interface ChecksState {
  loading: boolean;
  checks: PRCheck[];
  error: string;
}

interface Props {
  pullRequest: PullRequest | null;
  prId: number;
  provider: Provider;
  iterationCount: number;
  voteHistory: VoteHistoryEntry[];
  currentUserId: string;
  checksEnabled: boolean;
  checksState: ChecksState;
  onRefreshChecks: () => void;
  commentThreads: CommentThread[];
}

function branchName(refName: string): string {
  return refName.replace(/^refs\/heads\//, "");
}

function formatDate(value: string): string {
  if (!value) return "Unknown";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}

function sameIdentity(a: string, b: string): boolean {
  return !!a && !!b && a.trim().toLowerCase() === b.trim().toLowerCase();
}

function historyForReviewer(
  reviewer: Reviewer,
  history: VoteHistoryEntry[],
): VoteHistoryEntry[] {
  return history.filter((event) => {
    if (sameIdentity(event.reviewerId, reviewer.id)) return true;
    if (sameIdentity(event.reviewerName, reviewer.displayName)) return true;
    return false;
  });
}

function reviewerHadResetApproval(reviewer: Reviewer, history: VoteHistoryEntry[]): boolean {
  const events = historyForReviewer(reviewer, history);
  if (reviewer.vote !== 0 || events.length < 2) return false;
  const latest = events[events.length - 1];
  return latest?.vote === 0 && events.slice(0, -1).some((event) => event.vote === 10);
}

function statusClass(status: string): string {
  switch (status) {
    case "active":
      return "bg-green-100 text-green-800 dark:bg-green-900/40 dark:text-green-300";
    case "completed":
      return "bg-purple-100 text-purple-800 dark:bg-purple-900/40 dark:text-purple-300";
    case "abandoned":
      return "bg-red-100 text-red-800 dark:bg-red-900/40 dark:text-red-300";
    default:
      return "bg-gray-100 text-gray-700 dark:bg-gray-700 dark:text-gray-200";
  }
}

function threadStatusLabel(status: string | undefined): string {
  const value = status?.trim();
  if (!value) return "No status";
  return value.replace(/([a-z])([A-Z])/g, "$1 $2");
}

function threadStatusClass(status: string | undefined): string {
  switch (status?.trim().toLowerCase()) {
    case "active":
      return "bg-green-100 text-green-800 dark:bg-green-900/40 dark:text-green-300";
    case "fixed":
    case "closed":
    case "completed":
      return "bg-blue-100 text-blue-800 dark:bg-blue-900/40 dark:text-blue-300";
    case "wontfix":
    case "won't fix":
      return "bg-yellow-100 text-yellow-800 dark:bg-yellow-900/40 dark:text-yellow-300";
    default:
      return "bg-gray-100 text-gray-700 dark:bg-gray-700 dark:text-gray-200";
  }
}

function threadLocation(thread: CommentThread): string {
  if (!thread.filePath) return "PR-level thread";
  if (thread.lineStart > 0) {
    const line =
      thread.lineStart === thread.lineEnd || thread.lineEnd <= 0
        ? `line ${thread.lineStart}`
        : `lines ${thread.lineStart}-${thread.lineEnd}`;
    return `${thread.filePath} - ${line}`;
  }
  return `${thread.filePath} - file-level thread`;
}

function voteInfo(vote: number, provider: Provider) {
  switch (vote) {
    case 10:
      return {
        label: "Approved",
        shortLabel: "Approved",
        className: "bg-green-100 text-green-800 dark:bg-green-900/40 dark:text-green-300",
      };
    case 5:
      return {
        label: provider === "github" ? "Commented" : "Approved with suggestions",
        shortLabel: provider === "github" ? "Commented" : "Suggestions",
        className: "bg-emerald-100 text-emerald-800 dark:bg-emerald-900/40 dark:text-emerald-300",
      };
    case -5:
      return {
        label: "Waiting for author",
        shortLabel: "Waiting",
        className: "bg-yellow-100 text-yellow-800 dark:bg-yellow-900/40 dark:text-yellow-300",
      };
    case -10:
      return {
        label: provider === "github" ? "Changes requested" : "Rejected",
        shortLabel: provider === "github" ? "Changes" : "Rejected",
        className: "bg-red-100 text-red-800 dark:bg-red-900/40 dark:text-red-300",
      };
    case 0:
      return {
        label: "No vote",
        shortLabel: "No vote",
        className: "bg-gray-100 text-gray-700 dark:bg-gray-700 dark:text-gray-200",
      };
    default:
      return {
        label: `Vote ${vote}`,
        shortLabel: String(vote),
        className: "bg-gray-100 text-gray-700 dark:bg-gray-700 dark:text-gray-200",
      };
  }
}

function votePriority(vote: number): number {
  switch (vote) {
    case -10: return 0;
    case -5: return 1;
    case 10: return 2;
    case 5: return 3;
    case 0: return 4;
    default: return 5;
  }
}

function voteDotClass(vote: number): string {
  switch (vote) {
    case 10: return "bg-green-500";
    case 5: return "bg-emerald-500";
    case -5: return "bg-yellow-500";
    case -10: return "bg-red-500";
    default: return "bg-gray-400";
  }
}

function ReviewerRow({
  reviewer,
  provider,
  history,
}: {
  reviewer: Reviewer;
  provider: Provider;
  history: VoteHistoryEntry[];
}) {
  const vote = voteInfo(reviewer.vote, provider);
  const resetApproval = reviewerHadResetApproval(reviewer, history);
  return (
    <li class="flex items-center justify-between gap-3 px-3 py-2 border-b border-gray-100 dark:border-gray-800 last:border-0">
      <div class="min-w-0">
        <div class="text-sm font-medium text-gray-900 dark:text-gray-100 truncate">
          {reviewer.displayName}
        </div>
        {(resetApproval || reviewer.isRequired) && (
          <div class="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
            {resetApproval ? "Previously approved; vote reset" : "Required reviewer"}
          </div>
        )}
      </div>
      <span class={`shrink-0 text-xs px-2 py-0.5 rounded-full font-medium ${vote.className}`}>
        {vote.label}
      </span>
    </li>
  );
}

function VoteHistoryPanel({
  history,
  provider,
}: {
  history: VoteHistoryEntry[];
  provider: Provider;
}) {
  const timelines = Object.values(
    history.reduce<Record<string, { key: string; name: string; events: VoteHistoryEntry[] }>>(
      (acc, event) => {
        const reviewerName = event.reviewerName.trim();
        const reviewerId = event.reviewerId.trim();
        const key = reviewerName
          ? `name:${reviewerName.toLowerCase()}`
          : reviewerId
            ? `id:${reviewerId.toLowerCase()}`
            : `thread-${event.threadId}`;
        const name = reviewerName || reviewerId || "Unknown reviewer";
        if (!acc[key]) acc[key] = { key, name, events: [] };
        acc[key].events.push(event);
        if (reviewerName) acc[key].name = reviewerName;
        return acc;
      },
      {},
    ),
  )
    .map((timeline) => ({
      ...timeline,
      events: [...timeline.events].sort((a, b) => a.publishedDate.localeCompare(b.publishedDate)),
    }))
    .sort((a, b) => {
      const aLast = a.events[a.events.length - 1]?.publishedDate ?? "";
      const bLast = b.events[b.events.length - 1]?.publishedDate ?? "";
      return bLast.localeCompare(aLast);
    });

  return (
    <div class="rounded border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-900 p-4">
      <div class="flex items-center justify-between gap-3 mb-3">
        <h2 class="text-sm font-semibold text-gray-900 dark:text-gray-100">Historical votes</h2>
        <span class="text-xs text-gray-500 dark:text-gray-400">
          {history.length} event{history.length === 1 ? "" : "s"}
        </span>
      </div>
      {timelines.length > 0 ? (
        <div class="space-y-4">
          {timelines.map((timeline) => (
            <div key={timeline.key} class="rounded border border-gray-200 dark:border-gray-700 overflow-hidden">
              <div class="px-3 py-2 bg-gray-50 dark:bg-gray-800/70 border-b border-gray-200 dark:border-gray-700">
                <div class="text-sm font-semibold text-gray-900 dark:text-gray-100 truncate">
                  {timeline.name}
                </div>
                <div class="text-xs text-gray-500 dark:text-gray-400">
                  {timeline.events.length} vote event{timeline.events.length === 1 ? "" : "s"}
                </div>
              </div>
              <ol class="relative py-2">
                {timeline.events.map((event, index) => {
                  const vote = voteInfo(event.vote, provider);
                  const previous = index > 0 ? timeline.events[index - 1] : null;
                  const previousVote = previous ? voteInfo(previous.vote, provider) : null;
                  const label = event.vote === 0 ? "Vote reset" : vote.label;
                  const previousLabel = previous && previousVote
                    ? previous.vote === 0 ? "No vote" : previousVote.label
                    : "";
                  const transition = previousLabel
                    ? `${previousLabel} -> ${label}`
                    : label;
                  return (
                    <li
                      key={`${event.threadId}-${event.publishedDate}-${event.vote}`}
                      class="grid grid-cols-[1.25rem_minmax(0,1fr)_auto] gap-2 px-3 py-2 items-start"
                    >
                      <div class="relative flex justify-center h-full">
                        {index < timeline.events.length - 1 && (
                          <span class="absolute top-3 bottom-[-0.5rem] w-px bg-gray-200 dark:bg-gray-700" />
                        )}
                        <span class={`relative mt-1 w-2 h-2 rounded-full ${voteDotClass(event.vote)}`} />
                      </div>
                      <div class="min-w-0">
                        <div class="text-sm text-gray-900 dark:text-gray-100">
                          {transition}
                        </div>
                        <div class="text-xs text-gray-500 dark:text-gray-400">
                          {formatDate(event.publishedDate)}
                        </div>
                      </div>
                      <span class={`shrink-0 text-xs px-2 py-0.5 rounded-full font-medium ${vote.className}`}>
                        {event.vote === 0 ? "No vote" : vote.shortLabel}
                      </span>
                    </li>
                  );
                })}
              </ol>
            </div>
          ))}
        </div>
      ) : (
        <div class="text-sm text-gray-500 dark:text-gray-400">
          No historical vote events were found for this PR.
        </div>
      )}
    </div>
  );
}

function VoteSummary({
  reviewers,
  provider,
}: {
  reviewers: Reviewer[];
  provider: Provider;
}) {
  const buckets = [10, 5, -5, -10, 0].map((vote) => ({
    vote,
    count: reviewers.filter((r) => r.vote === vote).length,
    info: voteInfo(vote, provider),
  }));

  return (
    <div class="grid grid-cols-2 sm:grid-cols-5 gap-2">
      {buckets.map((bucket) => (
        <div
          key={bucket.vote}
          class="rounded border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-900 px-3 py-2 min-w-0"
        >
          <div class="text-lg font-semibold text-gray-900 dark:text-gray-100">
            {bucket.count}
          </div>
          <div class="text-xs text-gray-500 dark:text-gray-400 truncate">
            {bucket.info.shortLabel}
          </div>
        </div>
      ))}
    </div>
  );
}

function ChecksSummary({
  checksEnabled,
  checksState,
  onRefreshChecks,
}: {
  checksEnabled: boolean;
  checksState: ChecksState;
  onRefreshChecks: () => void;
}) {
  if (!checksEnabled) return null;

  if (checksState.loading) {
    return (
      <div class="rounded border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-900 px-3 py-2 text-sm text-gray-500 dark:text-gray-400">
        Loading build checks...
      </div>
    );
  }

  if (checksState.error) {
    return (
      <div class="rounded border border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-900/20 px-3 py-2 flex items-center gap-3">
        <div class="flex-1 min-w-0 text-sm text-red-700 dark:text-red-300 truncate" title={checksState.error}>
          Build checks unavailable
        </div>
        <button
          type="button"
          onClick={onRefreshChecks}
          class="shrink-0 text-xs px-2 py-1 rounded border border-red-300 dark:border-red-700 hover:bg-red-100 dark:hover:bg-red-900/40"
        >
          Refresh
        </button>
      </div>
    );
  }

  const rollup = getPrCheckRollup(checksState.checks);
  if (rollup.status === "none") {
    return (
      <div class="rounded border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-900 px-3 py-2 text-sm text-gray-500 dark:text-gray-400">
        No configured build checks
      </div>
    );
  }

  const tone =
    rollup.status === "fail"
      ? "text-red-600 dark:text-red-400"
      : rollup.status === "running"
        ? "text-blue-600 dark:text-blue-400"
        : "text-green-600 dark:text-green-400";

  return (
    <button
      type="button"
      onClick={onRefreshChecks}
      title={`${rollup.tooltip}\nClick to refresh build checks.`}
      class="w-full rounded border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-900 px-3 py-2 text-left hover:bg-gray-50 dark:hover:bg-gray-800"
    >
      <div class={`text-sm font-medium ${tone}`}>
        {rollup.requiredText}
      </div>
      {rollup.optionalText && (
        <div class="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
          {rollup.optionalText}
        </div>
      )}
    </button>
  );
}

function CommentsAccordion({ threads }: { threads: CommentThread[] }) {
  const [expanded, setExpanded] = useState(false);
  const commentCount = threads.reduce((sum, thread) => sum + thread.comments.length, 0);
  const sortedThreads = [...threads].sort((a, b) => {
    const aDate = a.comments[a.comments.length - 1]?.publishedDate ?? "";
    const bDate = b.comments[b.comments.length - 1]?.publishedDate ?? "";
    return bDate.localeCompare(aDate);
  });

  return (
    <section class="rounded border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-900 overflow-hidden">
      <button
        type="button"
        onClick={() => setExpanded((value) => !value)}
        aria-expanded={expanded}
        class="w-full px-4 py-3 flex items-center justify-between gap-3 text-left hover:bg-gray-50 dark:hover:bg-gray-800/70"
      >
        <div class="min-w-0">
          <h2 class="text-sm font-semibold text-gray-900 dark:text-gray-100">Comments</h2>
          <div class="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
            {commentCount} comment{commentCount === 1 ? "" : "s"} across {threads.length} thread{threads.length === 1 ? "" : "s"}
          </div>
        </div>
        <span class="shrink-0 text-gray-500 dark:text-gray-400" aria-hidden="true">
          {expanded ? "v" : ">"}
        </span>
      </button>

      {expanded && (
        <div class="border-t border-gray-200 dark:border-gray-700">
          {sortedThreads.length > 0 ? (
            <div class="divide-y divide-gray-100 dark:divide-gray-800">
              {sortedThreads.map((thread) => (
                <article key={thread.id} class="p-4">
                  <div class="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
                    <div class="min-w-0">
                      <div class="text-xs font-mono text-gray-500 dark:text-gray-400 break-all">
                        {threadLocation(thread)}
                      </div>
                      <div class="text-xs text-gray-400 dark:text-gray-500 mt-0.5">
                        Thread #{thread.id}
                      </div>
                    </div>
                    <span class={`shrink-0 self-start text-xs px-2 py-0.5 rounded-full font-medium ${threadStatusClass(thread.status)}`}>
                      {threadStatusLabel(thread.status)}
                    </span>
                  </div>

                  <div class="mt-3 space-y-3">
                    {thread.comments.map((comment) => (
                      <div
                        key={comment.id}
                        class="pl-3 border-l-2 border-gray-200 dark:border-gray-700"
                      >
                        <div class="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-xs">
                          <span class="font-medium text-gray-900 dark:text-gray-100">
                            {comment.author || "Unknown author"}
                          </span>
                          {comment.publishedDate && (
                            <span class="text-gray-400 dark:text-gray-500">
                              {formatDate(comment.publishedDate)}
                            </span>
                          )}
                        </div>
                        <div class="mt-1 text-sm text-gray-700 dark:text-gray-300 whitespace-pre-wrap break-words">
                          {comment.content || <span class="italic text-gray-400">(no content)</span>}
                        </div>
                      </div>
                    ))}
                  </div>
                </article>
              ))}
            </div>
          ) : (
            <div class="px-4 py-5 text-sm text-gray-500 dark:text-gray-400">
              No comments were found for this PR.
            </div>
          )}
        </div>
      )}
    </section>
  );
}

export function PRSummary({
  pullRequest,
  prId,
  provider,
  iterationCount,
  voteHistory,
  currentUserId,
  checksEnabled,
  checksState,
  onRefreshChecks,
  commentThreads,
}: Props) {
  if (!pullRequest) {
    return (
      <div class="h-full bg-gray-50 dark:bg-gray-800/50 flex items-center justify-center text-sm text-gray-500 dark:text-gray-400">
        Loading PR summary...
      </div>
    );
  }

  const reviewers = [...pullRequest.reviewers].sort((a, b) => {
    const voteDelta = votePriority(a.vote) - votePriority(b.vote);
    if (voteDelta !== 0) return voteDelta;
    if (a.isRequired !== b.isRequired) return a.isRequired ? -1 : 1;
    return a.displayName.localeCompare(b.displayName);
  });
  const requiredCount = reviewers.filter((r) => r.isRequired).length;
  const votedCount = reviewers.filter((r) => r.vote !== 0).length;
  const sourceBranch = branchName(pullRequest.sourceRefName);
  const targetBranch = branchName(pullRequest.targetRefName);
  const currentReviewer = reviewers.find((r) => sameIdentity(r.id, currentUserId));
  const myHistory = currentReviewer
    ? historyForReviewer(currentReviewer, voteHistory)
    : currentUserId
      ? voteHistory.filter((event) => sameIdentity(event.reviewerId, currentUserId))
      : [];
  const latestMyHistory = myHistory[myHistory.length - 1];
  const myApprovalWasReset =
    currentReviewer?.vote === 0 &&
    latestMyHistory?.vote === 0 &&
    myHistory.slice(0, -1).some((event) => event.vote === 10);

  return (
    <div class="h-full bg-gray-50 dark:bg-gray-800/50 overflow-y-auto">
      <div class="max-w-5xl mx-auto p-4 sm:p-6 space-y-4">
        <section class="rounded border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-900 p-4">
          <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
            <div class="min-w-0">
              <div class="flex flex-wrap items-center gap-2 mb-2">
                <span class="text-xs font-mono text-gray-500 dark:text-gray-400">#{prId}</span>
                {pullRequest.isDraft && (
                  <span class="text-xs px-2 py-0.5 rounded-full font-medium bg-gray-100 text-gray-700 dark:bg-gray-700 dark:text-gray-200">
                    Draft
                  </span>
                )}
                <span class={`text-xs px-2 py-0.5 rounded-full font-medium ${statusClass(pullRequest.status)}`}>
                  {pullRequest.status}
                </span>
              </div>
              <h1 class="text-lg sm:text-xl font-semibold text-gray-950 dark:text-gray-50 leading-snug">
                {pullRequest.title}
              </h1>
              <div class="mt-2 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-gray-500 dark:text-gray-400">
                <span>{pullRequest.createdBy.displayName}</span>
                <span class="hidden sm:inline">|</span>
                <span>{formatDate(pullRequest.creationDate)}</span>
                <span class="hidden sm:inline">|</span>
                <span class="font-mono min-w-0 truncate max-w-full">
                  {sourceBranch} -&gt; {targetBranch}
                </span>
              </div>
            </div>
            <div class="grid grid-cols-3 gap-2 shrink-0">
              <div class="rounded border border-gray-200 dark:border-gray-700 px-3 py-2 text-center">
                <div class="text-base font-semibold text-gray-900 dark:text-gray-100">{prFiles.value.length}</div>
                <div class="text-xs text-gray-500 dark:text-gray-400">Files</div>
              </div>
              <div class="rounded border border-gray-200 dark:border-gray-700 px-3 py-2 text-center">
                <div class="text-base font-semibold text-gray-900 dark:text-gray-100">{votedCount}</div>
                <div class="text-xs text-gray-500 dark:text-gray-400">Votes</div>
              </div>
              <div class="rounded border border-gray-200 dark:border-gray-700 px-3 py-2 text-center">
                <div class="text-base font-semibold text-gray-900 dark:text-gray-100">{requiredCount}</div>
                <div class="text-xs text-gray-500 dark:text-gray-400">Required</div>
              </div>
            </div>
          </div>
        </section>

        {myApprovalWasReset && (
          <section class="rounded border border-yellow-300 dark:border-yellow-700 bg-yellow-50 dark:bg-yellow-900/20 p-4">
            <div class="text-sm font-semibold text-yellow-900 dark:text-yellow-200">
              Your previous approval was reset
            </div>
            <div class="text-sm text-yellow-800 dark:text-yellow-300 mt-1">
              You approved this PR earlier, but your current vote is now no vote. Review the latest changes before voting again.
            </div>
          </section>
        )}

        <section class="grid gap-4 lg:grid-cols-[minmax(0,1fr)_20rem]">
          <div class="rounded border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-900 p-4 min-w-0">
            <div class="flex items-center justify-between gap-3 mb-3">
              <h2 class="text-sm font-semibold text-gray-900 dark:text-gray-100">Current votes</h2>
              <span class="text-xs text-gray-500 dark:text-gray-400">
                {votedCount}/{reviewers.length} reviewers voted
              </span>
            </div>
            <VoteSummary reviewers={reviewers} provider={provider} />
            <div class="mt-4 rounded border border-gray-200 dark:border-gray-700 overflow-hidden">
              {reviewers.length > 0 ? (
                <ul class="divide-y-0">
                  {reviewers.map((reviewer) => (
                    <ReviewerRow
                      key={reviewer.id}
                      reviewer={reviewer}
                      provider={provider}
                      history={voteHistory}
                    />
                  ))}
                </ul>
              ) : (
                <div class="px-3 py-4 text-sm text-gray-500 dark:text-gray-400">
                  No reviewers are assigned to this PR.
                </div>
              )}
            </div>
          </div>

          <div class="space-y-4">
            <div class="rounded border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-900 p-4">
              <h2 class="text-sm font-semibold text-gray-900 dark:text-gray-100 mb-3">PR state</h2>
              <dl class="space-y-3 text-sm">
                <div>
                  <dt class="text-xs text-gray-500 dark:text-gray-400">Merge status</dt>
                  <dd class="text-gray-900 dark:text-gray-100">{pullRequest.mergeStatus || "Unknown"}</dd>
                </div>
                <div>
                  <dt class="text-xs text-gray-500 dark:text-gray-400">Iteration</dt>
                  <dd class="text-gray-900 dark:text-gray-100">{iterationCount}</dd>
                </div>
              </dl>
            </div>
            <ChecksSummary
              checksEnabled={checksEnabled}
              checksState={checksState}
              onRefreshChecks={onRefreshChecks}
            />
          </div>
        </section>

        <VoteHistoryPanel history={voteHistory} provider={provider} />

        {pullRequest.description && (
          <section class="rounded border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-900 p-4">
            <h2 class="text-sm font-semibold text-gray-900 dark:text-gray-100 mb-3">Description</h2>
            <div class="text-sm text-gray-700 dark:text-gray-300 whitespace-pre-wrap break-words">
              {pullRequest.description}
            </div>
          </section>
        )}

        <CommentsAccordion threads={commentThreads} />
      </div>
    </div>
  );
}
