import { useState } from "preact/hooks";

interface Props {
  onVote: (vote: number) => void;
}

const ACTIONS = [
  { label: "Approve", vote: 10, class: "bg-green-600 hover:bg-green-700 text-white", confirm: false },
  { label: "Approve w/ Suggestions", vote: 5, class: "bg-green-500 hover:bg-green-600 text-white", confirm: false },
  { label: "Wait for Author", vote: -5, class: "bg-yellow-500 hover:bg-yellow-600 text-white", confirm: true },
  { label: "Reject", vote: -10, class: "bg-red-600 hover:bg-red-700 text-white", confirm: true },
];

export function ApprovalBar({ onVote }: Props) {
  const [confirming, setConfirming] = useState<number | null>(null);

  const handleClick = (vote: number, needsConfirm: boolean) => {
    if (needsConfirm) {
      setConfirming(vote);
    } else {
      onVote(vote);
    }
  };

  const handleConfirm = () => {
    if (confirming !== null) {
      onVote(confirming);
      setConfirming(null);
    }
  };

  return (
    <div class="flex items-center gap-1.5">
      {confirming !== null ? (
        <div class="flex items-center gap-1.5">
          <span class="text-xs text-gray-500">
            {confirming === -5 ? "Wait for author?" : "Reject this PR?"}
          </span>
          <button
            onClick={handleConfirm}
            class={`px-3 py-1 rounded text-xs font-medium ${confirming === -5 ? "bg-yellow-500 hover:bg-yellow-600" : "bg-red-600 hover:bg-red-700"} text-white`}
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
            onClick={() => handleClick(a.vote, a.confirm)}
            class={`px-3 py-1 rounded text-xs font-medium ${a.class}`}
          >
            {a.label}
          </button>
        ))
      )}
    </div>
  );
}
