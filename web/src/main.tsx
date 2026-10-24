import "@fontsource/ibm-plex-sans-condensed/400.css";
import "@fontsource/ibm-plex-sans-condensed/600.css";
import "@fontsource/ibm-plex-mono/400.css";
import "./styles.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

// Handy from the browser console: clipdeck.engine.state, clipdeck.store.get().
import { engine } from "./audio/host";
import { store } from "./state/store";
(window as unknown as { clipdeck: unknown }).clipdeck = { engine, store };
