import { useEffect, useRef, useState } from "react";

import { pool } from "../term/pool";
import { useStore } from "../state/store";
import { IconSearch, IconX } from "./Icons";

/** Search inside the active terminal (xterm addon). */
export function SearchBar() {
  const activeId = useStore((s) => s.activeId);
  const setDialog = useStore((s) => s.setDialog);
  const [text, setText] = useState("");
  const ref = useRef<HTMLInputElement>(null);

  useEffect(() => ref.current?.focus(), []);

  // Live search: re-find as the user types, without needing Enter.
  useEffect(() => {
    if (!activeId || !text) return;
    const t = window.setTimeout(() => pool.findNext(activeId, text), 150);
    return () => window.clearTimeout(t);
  }, [text, activeId]);

  if (!activeId) return null;

  const find = (dir: 1 | -1) => {
    if (!text) return;
    if (dir === 1) pool.findNext(activeId, text);
    else pool.findPrevious(activeId, text);
  };

  return (
    <div className="search-bar">
      <IconSearch width={13} height={13} />
      <input
        ref={ref}
        value={text}
        placeholder="Search…"
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") find(e.shiftKey ? -1 : 1);
          if (e.key === "Escape") {
            setDialog(null);
            pool.focus(activeId);
          }
        }}
      />
      <button className="icon-btn" onClick={() => find(-1)} title="Previous (Shift+Enter)">
        ↑
      </button>
      <button className="icon-btn" onClick={() => find(1)} title="Next (Enter)">
        ↓
      </button>
      <button
        className="icon-btn"
        onClick={() => {
          setDialog(null);
          pool.focus(activeId);
        }}
      >
        <IconX width={13} height={13} />
      </button>
    </div>
  );
}
