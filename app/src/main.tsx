import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "@/App";
import type { KogWorkspace } from "@/lib/kog";
import "@/index.css";

const root = document.getElementById("root");
if (!root) throw new Error("missing #root");

function fail(target: HTMLElement, message: string): void {
  target.replaceChildren();
  const box = document.createElement("div");
  box.className = "flex h-full items-center justify-center px-6 text-center text-signal";
  box.textContent = message;
  target.append(box);
}

// The scan is served by the binary that produced it: one fetch, and it either
// works or the server is not the one we think it is.
fetch("/graph.json")
  .then(async (response) => {
    if (!response.ok) throw new Error(`graph.json returned ${response.status}`);
    return (await response.json()) as KogWorkspace;
  })
  .then((workspace) => {
    if (!workspace.projects?.length) {
      fail(root, "This scan holds no project. Run `kog scan` to see what it found.");
      return;
    }
    createRoot(root).render(
      <StrictMode>
        <App workspace={workspace} />
      </StrictMode>,
    );
  })
  .catch((error: unknown) => {
    fail(root, `Could not load the scan: ${String(error)}`);
  });
