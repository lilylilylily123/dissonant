import { useEffect, useState } from "react";
import { cleanCode, codeComplete, codeProblem, CURSOR_COLORS, formatCode, saveIdentity, savedIdentity } from "../collab";
import { useStore } from "../store";
import { Words } from "./JoinRequests";

function Identity({ name, color, onName, onColor }: { name: string; color: string; onName: (n: string) => void; onColor: (c: string) => void }) {
  return (
    <>
      <label className="sharefield">
        <span>your name</span>
        <input autoFocus type="text" value={name} maxLength={24} placeholder="what people see on your cursor" onChange={(e) => onName(e.target.value)} />
      </label>
      <div className="sharefield">
        <span>your color</span>
        <div className="swatches">
          {CURSOR_COLORS.map((c) => (
            <button key={c} className={c === color ? "on" : ""} style={{ background: c }} aria-label={`cursor color ${c}`} aria-pressed={c === color} onClick={() => onColor(c)} />
          ))}
        </div>
      </div>
    </>
  );
}

function Start() {
  const start = useStore((s) => s.startSharing);
  const join = useStore((s) => s.joinSession);
  const close = useStore((s) => s.openShare);
  const saved = savedIdentity();
  const [tab, setTab] = useState<"share" | "join">("share");
  const [name, setName] = useState(saved.name);
  const [color, setColor] = useState(saved.color);
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const problem = codeProblem(code);
  const ready = name.trim().length > 0 && (tab === "share" || codeComplete(code));

  const go = async () => {
    if (!ready || busy) return;
    saveIdentity(name.trim(), color);
    setBusy(true);
    await (tab === "share" ? start(name.trim(), color) : join(formatCode(code), name.trim(), color));
    setBusy(false);
  };

  return (
    <>
      <div className="seg sharetabs">
        <button className={tab === "share" ? "on" : ""} onClick={() => setTab("share")}>
          share this project
        </button>
        <button className={tab === "join" ? "on" : ""} onClick={() => setTab("join")}>
          join someone
        </button>
      </div>
      <Identity name={name} color={color} onName={setName} onColor={setColor} />
      {tab === "share" ? (
        <div className="mbody">
          you get a code to give to someone, and you let them in when they knock. they edit this project with you, live, and each of you plays it on your own. it stays on this computer, so the session ends when you leave.
        </div>
      ) : (
        <>
          <label className="sharefield">
            <span>code</span>
            <input
              className="mono sharecodein"
              type="text"
              value={formatCode(code)}
              placeholder="ABC-DEF"
              spellCheck={false}
              autoComplete="off"
              autoCapitalize="characters"
              aria-invalid={problem !== null}
              onChange={(e) => setCode(cleanCode(e.target.value))}
            />
          </label>
          {problem && (
            <div className="sharehint" role="alert">
              {problem}
            </div>
          )}
          <div className="mbody">ask the host for their code. they let you in once you both see the same three words. joining replaces what's open here with their project.</div>
        </>
      )}
      <div className="mbtns">
        <button className="quiet" onClick={() => close(false)}>
          cancel
        </button>
        <button className="solid" disabled={!ready || busy} onClick={() => void go()}>
          {busy ? (tab === "share" ? "going online…" : "connecting…") : tab === "share" ? "start sharing" : "join"}
        </button>
      </div>
    </>
  );
}

function People() {
  const session = useStore((s) => s.session);
  const remove = useStore((s) => s.removePeer);
  return (
    <ul className="sharepeople">
      {session.peers.map((p) => (
        <li key={p.id}>
          <i style={{ background: p.color }} />
          <span>
            {p.name}
            {p.id === session.you ? " (you)" : p.isHost ? " (host)" : ""}
          </span>
          {session.role === "host" && p.id !== session.you && (
            <button className="quiet" onClick={() => void remove(p.id)}>
              remove
            </button>
          )}
        </li>
      ))}
    </ul>
  );
}

/** "new code in 4:12", ticking. The backend swaps the code itself when it runs out. */
function Countdown({ until }: { until: number }) {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const tick = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(tick);
  }, []);
  const left = Math.max(0, Math.ceil((until - now) / 1000));
  return <span className="countdown">{left > 0 ? `new code in ${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")}` : "new code coming…"}</span>;
}

function Hosting() {
  const code = useStore((s) => s.session.code);
  const expiresAt = useStore((s) => s.session.codeExpiresAt);
  const open = useStore((s) => s.session.open);
  const count = useStore((s) => s.session.peers.length);
  const stop = useStore((s) => s.stopSharing);
  const newCode = useStore((s) => s.newCode);
  const setOpen = useStore((s) => s.setOpen);
  const close = useStore((s) => s.openShare);
  // The code that was copied, so a fresh code doesn't claim to be on the clipboard.
  const [copied, setCopied] = useState<string | null>(null);
  const copy = async (text: string) => {
    await navigator.clipboard.writeText(text);
    setCopied(text);
    setTimeout(() => setCopied(null), 1500);
  };
  return (
    <>
      {!open ? (
        <div className="mbody">closed to new people. nobody new can join until you let people in again. everyone already here stays.</div>
      ) : code === null ? (
        <div className="mbody">making a code…</div>
      ) : (
        <>
          <div className="mbody">give this code to whoever you want in. each code works once and changes every 5 minutes, and you say yes or no to each person.</div>
          <div className="sharecode" aria-label={`join code ${code.replace("-", " ")}`}>
            {code}
          </div>
          <div className="sharerow">
            <button className="solid" onClick={() => void copy(code)}>
              {copied === code ? "copied" : "copy code"}
            </button>
            <button className="quiet" onClick={() => void newCode()} title="this code stops working. people already here stay.">
              new code
            </button>
            {expiresAt !== null && <Countdown until={expiresAt} />}
          </div>
        </>
      )}
      <label className="sharetoggle">
        <input type="checkbox" checked={open} onChange={(e) => void setOpen(e.target.checked)} />
        let new people in
      </label>
      <People />
      <div className="mbtns">
        <button className="danger" onClick={() => void stop()}>
          {count > 1 ? `end session for ${count - 1} ${count === 2 ? "person" : "people"}` : "stop sharing"}
        </button>
        <button className="quiet" onClick={() => close(false)}>
          close
        </button>
      </div>
    </>
  );
}

/** A guest on the way in: finding the host, then waiting at the door with the words to check. */
function Joining() {
  const waiting = useStore((s) => s.session.waiting);
  const words = useStore((s) => s.session.words);
  const leave = useStore((s) => s.leaveSession);
  const close = useStore((s) => s.openShare);
  return (
    <>
      {waiting ? (
        <>
          <div className="mbody">waiting for the host to let you in.</div>
          {words && <Words words={words} />}
          <div className="mbody">check the host sees the same words. if theirs are different, cancel: someone may be in between you.</div>
        </>
      ) : (
        <div className="mbody">{words ? "you're in. getting the project…" : "connecting to the host…"}</div>
      )}
      <div className="mbtns">
        <button className="danger" onClick={() => void leave()}>
          cancel
        </button>
        <button className="quiet" onClick={() => close(false)}>
          close
        </button>
      </div>
    </>
  );
}

function Joined() {
  const session = useStore((s) => s.session);
  const leave = useStore((s) => s.leaveSession);
  const close = useStore((s) => s.openShare);
  if (!session.live && !session.reconnecting) return <Joining />;
  const status = session.reconnecting
    ? "lost the host. trying to get back, and what you do now will go through when it does."
    : `connected${session.path ? ` (${session.path})` : ""}. recording and importing audio are the host's for now.`;
  return (
    <>
      <div className="mbody">{status}</div>
      <People />
      <div className="mbtns">
        <button className="danger" onClick={() => void leave()}>
          leave session
        </button>
        <button className="quiet" onClick={() => close(false)}>
          close
        </button>
      </div>
    </>
  );
}

/** Starting, joining and looking after a session. */
export function ShareDialog() {
  const open = useStore((s) => s.shareOpen);
  const close = useStore((s) => s.openShare);
  const role = useStore((s) => s.session.role);
  const joining = useStore((s) => s.session.role === "guest" && !s.session.live && !s.session.reconnecting);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        close(false);
      }
      // Nothing typed here should reach the editors or the transport.
      e.stopPropagation();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [open, close]);

  if (!open) return null;
  return (
    <div className="modal-backdrop" onPointerDown={() => close(false)}>
      <div className="modal sharedlg" role="dialog" aria-modal="true" aria-label="session" onPointerDown={(e) => e.stopPropagation()}>
        <div className="mtitle">{role === "solo" ? "work together" : role === "host" ? "you're sharing this project" : joining ? "joining" : "you're in a session"}</div>
        {role === "solo" ? <Start /> : role === "host" ? <Hosting /> : <Joined />}
      </div>
    </div>
  );
}
