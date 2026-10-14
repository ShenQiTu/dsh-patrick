import React from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { api, native } from "./api";
import "./style.css";
import "./network.css";
import "./compact.css";
import "./controls.css";
import "./overview.css";
import "./settings.css";
import "./calls.css";

createRoot(document.getElementById("root")!).render(<App />);
if (native)
  api
    .snapshot()
    .then(async (state) => {
      if (state.testMode) {
        const { runSmoke } = await import("./smoke");
        await runSmoke();
      }
    })
    .catch(console.error);
