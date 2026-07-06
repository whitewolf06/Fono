import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles/globals.css";

// Tauri webview decides which "view" to render based on the query parameter
// ?view=overlay / ?view=settings / ?view=onboarding (default).
// Backend creates separate windows pointing at these URLs.
const params = new URLSearchParams(window.location.search);
const view = params.get("view") ?? "settings";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App view={view as AppView} />
  </React.StrictMode>,
);

export type AppView = "settings" | "overlay" | "onboarding";
