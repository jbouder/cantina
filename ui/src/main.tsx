import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import { StageWindow } from "./components/StageWindow";
import { onAnalysis } from "./lib/engine";
import { prefersReducedMotion, reducesFlashing } from "./lib/motion";
import { reportErrorsToShell } from "./lib/report";
import { isStageWindow } from "./lib/stage";
import { applySkin, currentSkin, loadSkin, skinUsesBeat } from "./skins";
import { createSkinBeat } from "./skins/beat";
import "./styles.css";

reportErrorsToShell();
// Before the first paint, so the saved skin never flashes through Classic.
applySkin(loadSkin());
// The skin's beat pulse, for the skins that have one.
const skinBeat = createSkinBeat(
  document.documentElement.style,
  () => skinUsesBeat(currentSkin()) && !prefersReducedMotion() && !reducesFlashing(),
);
onAnalysis((analysis) => skinBeat.push(analysis));

const root = document.getElementById("root");
if (!root) {
  throw new Error("index.html has no #root element");
}

ReactDOM.createRoot(root).render(
  <React.StrictMode>{isStageWindow() ? <StageWindow /> : <App />}</React.StrictMode>,
);
