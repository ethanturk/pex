interface Props {
  onVote: (vote: number) => void;
}

const ACTIONS = [
  { label: "Approve", vote: 10, class: "bg-green-600 hover:bg-green-700 text-white" },
  { label: "Approve w/ Suggestions", vote: 5, class: "bg-green-500 hover:bg-green-600 text-white" },
  { label: "Wait for Author", vote: -5, class: "bg-yellow-500 hover:bg-yellow-600 text-white" },
  { label: "Reject", vote: -10, class: "bg-red-600 hover:bg-red-700 text-white" },
];

export function ApprovalBar({ onVote }: Props) {
  return (
    <div class="flex items-center gap-1.5">
      {ACTIONS.map((a) => (
        <button
          key={a.vote}
          onClick={() => onVote(a.vote)}
          class={`px-3 py-1 rounded text-xs font-medium ${a.class}`}
        >
          {a.label}
        </button>
      ))}
    </div>
  );
}
