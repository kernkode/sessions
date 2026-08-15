import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

// Fonts bundled with the app: no CDN and no dependency on what is installed on
// the system, so metrics are identical on every machine.
import "@fontsource-variable/inter/wght.css";
import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/500.css";
import "@fontsource/jetbrains-mono/700.css";

import App from "./App";
import { pool, rebuildScreen } from "./term/pool";
import { useStore } from "./state/store";
import "./styles.css";

// Suppress the browser context menu: this is a native window.
window.addEventListener("contextmenu", (e) => e.preventDefault());

// xterm measures character width when it is created: if the font is not ready
// yet the column count would be wrong. Re-fit as soon as loading finishes.
void document.fonts.ready.then(() => pool.refreshMetrics());

// In development the state is exposed so the UI can be inspected and automated.
if (import.meta.env.DEV) {
  Object.assign(window, { __store: useStore, __pool: pool, __rebuildScreen: rebuildScreen });
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
