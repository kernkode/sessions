// Terminal pool: one xterm instance per session, living outside React.
//
// Performance decisions:
//  · Each terminal's `div` lives in a container of its own and is hidden with
//    `display:none` on tab switch. They are never re-mounted, so switching
//    sessions is instant and the emulator state is preserved.
//  · Instances of sessions that already ended are released by LRU
//    (`max_live_terminals`); reopening them rehydrates from the backend.
//  · PTY output goes straight to `term.write()`: React is not involved.
//  · Keeping a live session's terminal alive also matters because ConPTY waits
//    for the answer to `ESC[6n`, which the emulator itself produces.

import { Terminal, type ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { SerializeAddon } from "@xterm/addon-serialize";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebglAddon } from "@xterm/addon-webgl";
import "@xterm/xterm/css/xterm.css";

import { api, attachSession } from "../lib/ipc";
import type { TerminalConfig } from "../lib/types";

export const THEME: ITheme = {
  background: "#0e0f12",
  foreground: "#d7dae0",
  cursor: "#e8663d",
  cursorAccent: "#0e0f12",
  selectionBackground: "#2b3a55",
  black: "#12141a",
  red: "#f4614a",
  green: "#4fc08d",
  yellow: "#e3b341",
  blue: "#5aa2f0",
  magenta: "#c678dd",
  cyan: "#43c7d6",
  white: "#c6cad3",
  brightBlack: "#5b616e",
  brightRed: "#ff7f6b",
  brightGreen: "#6ee7a8",
  brightYellow: "#f2ce5c",
  brightBlue: "#7fb8ff",
  brightMagenta: "#dd9bef",
  brightCyan: "#6be0ec",
  brightWhite: "#f0f2f5",
};

interface Entry {
  id: string;
  term: Terminal;
  fit: FitAddon;
  search: SearchAddon;
  webgl?: WebglAddon;
  el: HTMLDivElement;
  attached: boolean;
  lastUsed: number;
  cols: number;
  rows: number;
}

/** How a session should be rehydrated when its terminal is created. */
export interface AttachOptions {
  /** `false` when the process already ended: the history is replayed. */
  live: boolean;
  /** Size the session had, used to replay a TUI faithfully. */
  cols?: number;
  rows?: number;
}

/**
 * Rebuilds the final screen of a finished session.
 *
 * Raw PTY output cannot be written straight into a terminal of a different size:
 * TUIs draw with absolute positioning, so the result is scrambled, and sequences
 * such as `CSI ?25l` would leave the cursor hidden. Instead the bytes are replayed
 * into an off-screen emulator of the original size and its buffer is serialised:
 * the emulator interprets, and the serializer emits only content plus colour.
 */
export async function rebuildScreen(raw: Uint8Array, cols: number, rows: number): Promise<string> {
  const shadow = new Terminal({
    cols: Math.max(20, cols || 120),
    rows: Math.max(4, rows || 32),
    scrollback: 5000,
    allowProposedApi: true,
  });
  const serializer = new SerializeAddon();
  shadow.loadAddon(serializer);
  await new Promise<void>((resolve) => shadow.write(raw, resolve));
  const text = serializer.serialize({ scrollback: 5000 });
  shadow.dispose();
  return text;
}

class TerminalPool {
  private entries = new Map<string, Entry>();
  private host: HTMLElement | null = null;
  private cfg: TerminalConfig | null = null;
  private maxLive = 6;
  private active: string | null = null;
  private observer: ResizeObserver | null = null;
  private resizeTimer: number | null = null;
  private fontLoadKey = "";
  /// Called when input cannot be delivered (the session already ended).
  private onInputRejected: ((id: string) => void) | null = null;

  setHost(host: HTMLElement | null) {
    this.host = host;
    this.observer?.disconnect();
    this.observer = null;
    if (!host) return;
    // A single observer for the whole terminal area.
    this.observer = new ResizeObserver(() => this.scheduleFit());
    this.observer.observe(host);
    for (const e of this.entries.values()) {
      if (e.el.parentElement !== host) host.appendChild(e.el);
    }
  }

  setConfig(cfg: TerminalConfig, maxLive: number) {
    this.cfg = cfg;
    this.maxLive = Math.max(1, maxLive);
    for (const e of this.entries.values()) this.applyConfig(e);
    this.scheduleFit();
    void this.preloadFonts();
  }

  /**
   * Activates the terminal's font faces before the renderer rasterises them.
   *
   * Web fonts load lazily: the first bold output reaches the WebGL atlas while
   * the 700 face is still downloading, so those glyphs are baked with a
   * fallback font and the bold run looks uneven until a redraw (e.g. a
   * selection) repaints it. Loading the faces explicitly and clearing the
   * atlas once they are active makes the first paint already uniform.
   */
  private async preloadFonts() {
    const family = this.cfg?.font_family ?? "monospace";
    const size = this.cfg?.font_size ?? 13;
    const key = `${family}@${size}`;
    if (key === this.fontLoadKey) return;
    this.fontLoadKey = key;

    const names = family
      .split(",")
      .map((f) => f.trim().replace(/^["']|["']$/g, ""))
      .filter((f) => f.length > 0 && !/^(monospace|serif|sans-serif)$/i.test(f));
    const specs: string[] = [];
    for (const name of names) {
      for (const weight of ["400", "700"]) {
        specs.push(`${weight} ${size}px "${name}"`);
        specs.push(`${weight} italic ${size}px "${name}"`);
      }
    }
    try {
      await Promise.all(specs.map((s) => document.fonts.load(s)));
    } catch {
      // A missing face is fine: the browser falls back per glyph.
    }
    for (const e of this.entries.values()) {
      try {
        e.webgl?.clearTextureAtlas();
        e.term.refresh(0, e.term.rows - 1);
      } catch {
        // The renderer may not be active yet.
      }
    }
  }

  setInputRejectedHandler(cb: (id: string) => void) {
    this.onInputRejected = cb;
  }

  private applyConfig(e: Entry) {
    if (!this.cfg) return;
    const o = e.term.options;
    o.fontFamily = this.cfg.font_family;
    o.fontSize = this.cfg.font_size;
    o.lineHeight = this.cfg.line_height;
    o.scrollback = this.cfg.scrollback;
    o.cursorBlink = this.cfg.cursor_blink;
    o.cursorStyle = (this.cfg.cursor_style as "bar" | "block" | "underline") ?? "bar";
  }

  has(id: string) {
    return this.entries.has(id);
  }

  /** Creates (if needed) a session's terminal and connects it to the backend. */
  async ensure(id: string, opts: AttachOptions = { live: true }): Promise<void> {
    const existing = this.entries.get(id);
    if (existing) {
      existing.lastUsed = Date.now();
      return;
    }

    void this.preloadFonts();

    const el = document.createElement("div");
    el.className = "term-instance";
    el.style.display = "none";
    this.host?.appendChild(el);

    const term = new Terminal({
      allowProposedApi: true,
      fontFamily: this.cfg?.font_family ?? "monospace",
      fontSize: this.cfg?.font_size ?? 13,
      lineHeight: this.cfg?.line_height ?? 1.25,
      scrollback: this.cfg?.scrollback ?? 8000,
      cursorBlink: this.cfg?.cursor_blink ?? true,
      cursorStyle: (this.cfg?.cursor_style as "bar" | "block" | "underline") ?? "bar",
      theme: THEME,
      convertEol: false,
      drawBoldTextInBrightColors: true,
      smoothScrollDuration: 0,
      macOptionIsMeta: true,
      minimumContrastRatio: 1,
      allowTransparency: false,
    });

    const fit = new FitAddon();
    const search = new SearchAddon();
    term.loadAddon(fit);
    term.loadAddon(search);
    const unicode = new Unicode11Addon();
    term.loadAddon(unicode);
    term.unicode.activeVersion = "11";

    term.open(el);

    let webgl: WebglAddon | undefined;
    if ((this.cfg?.renderer ?? "webgl") === "webgl") {
      try {
        webgl = new WebglAddon();
        webgl.onContextLoss(() => {
          webgl?.dispose();
          webgl = undefined;
        });
        term.loadAddon(webgl);
      } catch {
        // Without WebGL the default DOM renderer is used.
        webgl = undefined;
      }
    }

    const entry: Entry = {
      id,
      term,
      fit,
      search,
      webgl,
      el,
      attached: false,
      lastUsed: Date.now(),
      cols: term.cols,
      rows: term.rows,
    };
    this.entries.set(id, entry);

    // Keyboard and paste → PTY. A session that already ended rejects the write:
    // the UI is told so it can explain it instead of swallowing the keystroke.
    const sendInput = (d: string) => {
      void api.sessionInput(id, d).catch(() => this.onInputRejected?.(id));
    };
    term.onData(sendInput);
    // Emulator answers to program queries (including ESC[6n).
    term.onBinary(sendInput);
    term.onResize(({ cols, rows }) => {
      if (cols === entry.cols && rows === entry.rows) return;
      entry.cols = cols;
      entry.rows = rows;
      void api.sessionResize(id, cols, rows).catch(() => {});
    });

    const scrollback = await attachSession(id, (chunk) => {
      entry.term.write(chunk);
    });
    entry.attached = true;
    if (scrollback.byteLength > 0) {
      if (opts.live) {
        // Live session: the emulator needs the bytes as they are, including
        // answering ConPTY's `ESC[6n`, which is what unblocks the process.
        entry.term.write(scrollback);
      } else {
        const screen = await rebuildScreen(scrollback, opts.cols ?? 0, opts.rows ?? 0);
        entry.term.write(screen.replace(/\n/g, "\r\n"));
      }
    }
  }

  /** Shows one session and hides the rest. */
  async show(id: string, opts: AttachOptions = { live: true }): Promise<void> {
    await this.ensure(id, opts);
    this.active = id;
    for (const [key, e] of this.entries) {
      const visible = key === id;
      e.el.style.display = visible ? "block" : "none";
      if (visible) e.lastUsed = Date.now();
    }
    // The fit must happen once the div is visible.
    requestAnimationFrame(() => {
      this.fit(id);
      this.entries.get(id)?.term.focus();
    });
  }

  fit(id?: string) {
    const e = this.entries.get(id ?? this.active ?? "");
    if (!e || e.el.style.display === "none") return;
    try {
      e.fit.fit();
    } catch {
      // The container may have zero size during transitions.
    }
  }

  private scheduleFit() {
    if (this.resizeTimer !== null) window.clearTimeout(this.resizeTimer);
    // Groups resize bursts (dragging the window edge).
    this.resizeTimer = window.setTimeout(() => {
      this.resizeTimer = null;
      this.fit();
    }, 60);
  }

  /** Recomputes metrics for every terminal: called when the fonts finish
   *  loading or the font size changes. */
  refreshMetrics() {
    for (const e of this.entries.values()) {
      try {
        e.term.options.fontFamily = this.cfg?.font_family ?? e.term.options.fontFamily;
        e.webgl?.clearTextureAtlas();
      } catch {
        // The renderer may not be active yet.
      }
    }
    this.scheduleFit();
  }

  clear(id: string) {
    this.entries.get(id)?.term.clear();
  }

  /** Buffer dump as text: diagnostics and UI tests. */
  dump(id: string, maxLines = 200): string {
    const e = this.entries.get(id);
    if (!e) return "";
    const buf = e.term.buffer.active;
    const total = Math.min(buf.length, maxLines);
    const from = Math.max(0, buf.length - total);
    const out: string[] = [];
    for (let i = from; i < buf.length; i++) {
      out.push(buf.getLine(i)?.translateToString(true) ?? "");
    }
    return out.join("\n").replace(/\n{3,}/g, "\n\n").trim();
  }

  findNext(id: string, text: string) {
    return this.entries.get(id)?.search.findNext(text, { decorations: undefined }) ?? false;
  }

  findPrevious(id: string, text: string) {
    return this.entries.get(id)?.search.findPrevious(text) ?? false;
  }

  focus(id: string) {
    this.entries.get(id)?.term.focus();
  }

  write(id: string, text: string) {
    this.entries.get(id)?.term.write(text);
  }

  dispose(id: string) {
    const e = this.entries.get(id);
    if (!e) return;
    void api.sessionDetach(id).catch(() => {});
    try {
      e.webgl?.dispose();
      e.term.dispose();
    } catch {
      // Already released.
    }
    e.el.remove();
    this.entries.delete(id);
    if (this.active === id) this.active = null;
  }

  /**
   * Releases terminals of sessions that already ended once the limit is
   * exceeded. Live sessions are never released: their emulator must keep
   * answering.
   */
  collect(aliveIds: Set<string>) {
    const candidates = [...this.entries.values()]
      .filter((e) => e.id !== this.active && !aliveIds.has(e.id))
      .sort((a, b) => a.lastUsed - b.lastUsed);
    const excess = this.entries.size - this.maxLive;
    for (let i = 0; i < Math.min(excess, candidates.length); i++) {
      this.dispose(candidates[i].id);
    }
  }

  disposeAll() {
    for (const id of [...this.entries.keys()]) this.dispose(id);
    this.observer?.disconnect();
  }
}

export const pool = new TerminalPool();
