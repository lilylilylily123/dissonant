import { useEffect, useRef, useState } from "react";
import { useStore } from "../store";

/** The three words both ends of a connection see, big enough to read out loud. */
export function Words({ words }: { words: string[] }) {
  return (
    <div className="sharewords">
      {words.map((w, i) => (
        <span key={i}>{w}</span>
      ))}
    </div>
  );
}

/**
 * Someone is at the door: the host lets them in or turns them away. Shown over everything, the
 * share dialog open or not, one person at a time, oldest first. It can't be waved away: the
 * person is waiting on an answer.
 */
export function JoinRequests() {
  const request = useStore((s) => (s.session.role === "host" ? s.session.requests[0] : undefined));
  const behind = useStore((s) => s.session.requests.length - 1);
  const answer = useStore((s) => s.answerJoin);
  // The request already answered, while the session catches up.
  const [answered, setAnswered] = useState<string | null>(null);
  const card = useRef<HTMLDivElement>(null);
  const id = request?.id;

  useEffect(() => {
    if (id === undefined) return;
    card.current?.focus();
    // Nothing typed reaches the editors or the transport while someone waits on an answer.
    const onKey = (e: KeyboardEvent) => e.stopPropagation();
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [id]);

  if (!request) return null;
  const busy = answered === request.id;
  const choose = async (allow: boolean) => {
    setAnswered(request.id);
    // Answered: the request goes with the next session update. Failed: let them try again.
    if (!(await answer(request.id, allow))) setAnswered(null);
  };
  return (
    <div className="modal-backdrop">
      <div ref={card} tabIndex={-1} className="modal joinask" role="alertdialog" aria-modal="true" aria-labelledby="joinask-title" aria-describedby="joinask-words joinask-check">
        <div className="mtitle" id="joinask-title">
          <i style={{ background: request.color }} />
          {request.name} wants to join
        </div>
        <div id="joinask-words">
          <Words words={request.words} />
        </div>
        <div className="mbody" id="joinask-check">
          ask them to check they see the same words. if theirs are different, deny: someone may be in between you.
        </div>
        {behind > 0 && <div className="mbody joinask-more">{behind === 1 ? "1 more person waiting after them" : `${behind} more people waiting after them`}</div>}
        <div className="mbtns">
          <button className="danger" disabled={busy} onClick={() => void choose(false)}>
            deny
          </button>
          <button className="solid" disabled={busy} onClick={() => void choose(true)}>
            allow
          </button>
        </div>
      </div>
    </div>
  );
}
