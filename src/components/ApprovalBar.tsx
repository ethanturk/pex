import { useEffect, useState } from "preact/hooks";
import { activeOrg } from "@/lib/signals";

interface Props {
  onVote: (vote: number) => void;
}

// Azure DevOps exposes a 4-level reviewer vote.
const ADO_ACTIONS = [
  { label: "Approve", vote: 10, class: "bg-green-600 hover:bg-green-700 text-white", prompt: "Approve this PR?" },
  { label: "Approve w/ Suggestions", vote: 5, class: "bg-green-500 hover:bg-green-600 text-white", prompt: "Approve with suggestions?" },
  { label: "Wait for Author", vote: -5, class: "bg-yellow-500 hover:bg-yellow-600 text-white", prompt: "Wait for author?" },
  { label: "Reject", vote: -10, class: "bg-red-600 hover:bg-red-700 text-white", prompt: "Reject this PR?" },
];

// GitHub reviews only support three outcomes; the backend maps these int votes
// to APPROVE / REQUEST_CHANGES / COMMENT review events.
const GITHUB_ACTIONS = [
  { label: "Approve", vote: 10, class: "bg-green-600 hover:bg-green-700 text-white", prompt: "Approve this PR?" },
  { label: "Request Changes", vote: -10, class: "bg-red-600 hover:bg-red-700 text-white", prompt: "Request changes on this PR?" },
  { label: "Comment", vote: 5, class: "bg-blue-600 hover:bg-blue-700 text-white", prompt: "Submit a review comment?" },
];

function reviewActions() {
  return activeOrg.value?.provider === "github" ? GITHUB_ACTIONS : ADO_ACTIONS;
}

export function ApprovalBar({ onVote }: Props) {
  const actions = reviewActions();
  const [confirming, setConfirming] = useState<number | null>(null);
  const pending = confirming !== null ? actions.find((a) => a.vote === confirming) : null;

  const cancel = () => setConfirming(null);
  const confirm = () => {
    if (confirming !== null) {
      onVote(confirming);
      setConfirming(null);
    }
  };

  useEffect(() => {
    if (!pending) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") cancel();
      else if (e.key === "Enter") confirm();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [pending, confirming]);

  return (
    <>
      <div class="flex items-center gap-1.5">
        {actions.map((a) => (
          <button
            key={a.vote}
            onClick={() => setConfirming(a.vote)}
            class={`px-3 py-1 rounded text-xs font-medium ${a.class}`}
          >
            {a.label}
          </button>
        ))}
      </div>

      {pending && (
        <div
          class="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
          onClick={cancel}
        >
          <div
            class="bg-white dark:bg-gray-900 border border-gray-200 dark:border-gray-700 rounded-lg shadow-xl p-5 w-[320px] max-w-[90vw]"
            onClick={(e) => e.stopPropagation()}
          >
            <div class="text-sm font-medium text-gray-900 dark:text-gray-100 mb-4">
              {pending.prompt}
            </div>
            <div class="flex justify-end gap-2">
              <button
                onClick={cancel}
                class="px-3 py-1.5 rounded text-xs font-medium text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-800"
              >
                Cancel
              </button>
              <button
                onClick={confirm}
                autofocus
                class={`px-3 py-1.5 rounded text-xs font-medium ${pending.class}`}
              >
                Confirm
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}

export function MobileReviewActions() {
  const actions = reviewActions();
  const [open, setOpen] = useState(false);
  const [confirming, setConfirming] = useState<number | null>(null);
  const pending = confirming !== null ? actions.find((a) => a.vote === confirming) : null;

  const cancel = () => setConfirming(null);
  const confirm = () => {
    if (confirming === null) return;
    window.dispatchEvent(
      new CustomEvent("pex:review-vote", { detail: { vote: confirming } }),
    );
    setConfirming(null);
  };

  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    window.addEventListener("click", close);
    return () => window.removeEventListener("click", close);
  }, [open]);

  useEffect(() => {
    if (!pending) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") cancel();
      else if (e.key === "Enter") confirm();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [pending, confirming]);

  return (
    <>
      <div class="relative">
        <button
          class="inline-flex items-center justify-center w-9 h-9 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-green-700 dark:text-green-400 font-semibold hover:bg-gray-50 dark:hover:bg-gray-700"
          onClick={(e) => {
            e.stopPropagation();
            setOpen((v) => !v);
          }}
          title="Review actions"
          aria-label="Review actions"
          aria-expanded={open}
        >
          ✓
        </button>
        {open && (
          <div
            class="absolute right-0 mt-2 w-52 rounded-lg border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-900 shadow-lg z-50 overflow-hidden"
            onClick={(e) => e.stopPropagation()}
          >
            {actions.map((a) => (
              <button
                key={a.vote}
                class="w-full text-left px-3 py-2 text-sm hover:bg-gray-100 dark:hover:bg-gray-800"
                onClick={() => {
                  setOpen(false);
                  setConfirming(a.vote);
                }}
              >
                {a.label}
              </button>
            ))}
          </div>
        )}
      </div>

      {pending && (
        <div
          class="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
          onClick={cancel}
        >
          <div
            class="bg-white dark:bg-gray-900 border border-gray-200 dark:border-gray-700 rounded-lg shadow-xl p-5 w-[320px] max-w-[90vw]"
            onClick={(e) => e.stopPropagation()}
          >
            <div class="text-sm font-medium text-gray-900 dark:text-gray-100 mb-4">
              {pending.prompt}
            </div>
            <div class="flex justify-end gap-2">
              <button
                onClick={cancel}
                class="px-3 py-1.5 rounded text-xs font-medium text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-800"
              >
                Cancel
              </button>
              <button
                onClick={confirm}
                autofocus
                class={`px-3 py-1.5 rounded text-xs font-medium ${pending.class}`}
              >
                Confirm
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
