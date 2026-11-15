import React from "react";
import ReactDOM from "react-dom/client";
import "./styles.css";

async function mount() {
  const halo = "__TAURI_INTERNALS__" in window && (await import("@tauri-apps/api/window")).getCurrentWindow().label === "halo";
  // Each WebView loads only its own surface; the halo doesn't need the app's
  // interaction logic or status icons.
  const Surface = halo
    ? (await import("./components/QuotaHalo")).QuotaHaloSurface
    : (await import("./App")).default;
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode><Surface /></React.StrictMode>,
  );
}
void mount();
