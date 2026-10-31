import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import { applySkin, loadSkin } from "./skins";
import "./styles.css";

// Before the first paint, so the saved skin never flashes through Classic.
applySkin(loadSkin());

const root = document.getElementById("root");
if (!root) {
  throw new Error("index.html has no #root element");
}

ReactDOM.createRoot(root).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
