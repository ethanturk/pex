import { useState } from "preact/hooks";

interface Props {
  onVote: (vote: number) => void;
}

const ACTIONS = [
  { label: "Approve", vote: 10, class: "bg-green-600 hover:bg-green-700 text-white", prompt: "Approve this PR?" },
  { label: "Approve w/ Suggestions", vote: 5, class: "bg-green-500 hover:bg-green-600 text-white", prompt: "Approve with suggestions?" },
  { label: "Wait for Author", vote: -5, class: "bg-yellow-500 hover:bg-yellow-600 text-white", prompt: "Wait for author?" },
  { label: "Reject", vote: -10, class: "bg-red-600 hover:bg-red-700 text-white", prompt: "Reject this PR?" },
];

export function ApprovalBar({ onVote }: Props) {
  const [confirming, setConfirming] = useState<number | null>(null);

  const handleConfirm = () => {
    if (confirming !== null) {
      onVote(confirming);
      setConfirming(null);
    }
  };

  const pending = confirming !== null ? ACTIONS.find((a) => a.vote === confirming) : null;

  return (
    <div class="flex items-center gap-1.5">
      {pending ? (
        <div class="flex items-center gap-1.5">
          <span class="text-xs text-gray-500">{pending.prompt}</span>
          <button
            onClick={handleConfirm}
            class={`px-3 py-1 rounded text-xs font-medium ${pending.class}`}
          >
            Confirm
          </button>
          <button
            onClick={() => setConfirming(null)}
            class="px-3 py-1 rounded text-xs font-medium text-gray-500 hover:text-gray-700 dark:hover:text-gray-300"
          >
            Cancel
          </button>
        </div>
      ) : (
        ACTIONS.map((a) => (
          <button
            key={a.vote}
            onClick={() => setConfirming(a.vote)}
            class={`px-3 py-1 rounded text-xs font-medium ${a.class}`}
          >
            {a.label}
          </button>
        ))
      )}
    </div>
  );
}
